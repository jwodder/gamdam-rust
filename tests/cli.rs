use gamdam::Downloadable;
use relative_path::RelativePathBuf;
use rstest::rstest;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::read_to_string;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::tempdir;

static DATA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data");

struct Annex {
    repo: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
struct MetadataOutput {
    success: bool,
    error_messages: Vec<String>,
    command: String,
    file: String,
    input: Vec<String>,
    fields: HashMap<String, Vec<String>>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
struct WhereisOutput {
    success: bool,
    error_messages: Vec<String>,
    command: String,
    file: String,
    input: Vec<String>,
    key: String,
    note: String,
    untrusted: Vec<WhereisLocation>,
    whereis: Vec<WhereisLocation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct WhereisLocation {
    description: String,
    here: bool,
    urls: Vec<String>,
    uuid: String,
}

impl Annex {
    fn new<P: AsRef<Path>>(repo: P) -> Self {
        Annex {
            repo: PathBuf::from(repo.as_ref()),
        }
    }

    fn is_clean(&self) -> bool {
        Command::new("git")
            .args(["diff", "--cached", "--quiet"])
            .current_dir(&self.repo)
            .status()
            .expect("Failed to run `git diff`")
            .success()
    }

    fn get_metadata(&self, path: &RelativePathBuf) -> HashMap<String, Vec<String>> {
        let r = Command::new("git-annex")
            .args(["metadata", "--json", "--"])
            .arg(path.as_str())
            .current_dir(&self.repo)
            .output()
            .expect("Failed to run `git-annex metadata`");
        assert!(r.status.success());
        serde_json::from_slice::<MetadataOutput>(&r.stdout)
            .expect("Error parsing `git-annex metadata` output")
            .fields
    }

    fn get_urls(&self, path: &RelativePathBuf) -> Vec<String> {
        let r = Command::new("git-annex")
            .args(["whereis", "--json", "--"])
            .arg(path.as_str())
            .current_dir(&self.repo)
            .output()
            .expect("Failed to run `git-annex whereis`");
        assert!(r.status.success());
        for loc in serde_json::from_slice::<WhereisOutput>(&r.stdout)
            .expect("Error parsing `git-annex whereis` output")
            .whereis
        {
            if loc.description == "web" {
                let mut urls = loc.urls;
                urls.sort();
                return urls;
            }
        }
        Vec::new()
    }
}

impl Drop for Annex {
    fn drop(&mut self) {
        let r = Command::new("git-annex")
            .args(["uninit"])
            .current_dir(&self.repo)
            .status();
        if !matches!(r, Ok(rc) if rc.success()) {
            eprintln!("WARNING: Failed to de-init git annex repo");
        }
    }
}

#[rstest]
#[case("simple.jsonl")]
#[case("metadata.jsonl")]
#[case("extra-urls.jsonl")]
#[case("successful.jsonl")]
fn test_gamdam_successful(#[case] infile: &str) {
    let tmpdir = tempdir().unwrap();
    let tmp_path = tmpdir.path();
    let infile = Path::new(DATA_DIR).join(infile);
    let items =
        serde_json::Deserializer::from_str(&read_to_string(&infile).expect("Error reading infile"))
            .into_iter::<Downloadable>()
            .collect::<Result<Vec<_>, _>>()
            .expect("Error parsing infile");
    let r = Command::new("git")
        .args(["init"])
        .current_dir(tmp_path)
        .status()
        .unwrap();
    assert!(r.success());
    let r = Command::new("git-annex")
        .args(["init"])
        .current_dir(tmp_path)
        .status()
        .unwrap();
    assert!(r.success());
    let annex = Annex::new(tmp_path);
    let r = Command::new(env!("CARGO_BIN_EXE_gamdam"))
        .args([
            "--log-level".as_ref(),
            "DEBUG".as_ref(),
            "-C".as_ref(),
            tmp_path,
            infile.as_ref(),
        ])
        .status()
        .expect("Failed to execute gamdam");
    assert!(r.success());
    assert!(annex.is_clean());
    for dl in items {
        assert!(tmp_path.join(dl.path.as_str()).exists());
        let md = annex.get_metadata(&dl.path);
        for (k, v) in dl.metadata {
            assert_eq!(md.get(&k), Some(&v));
        }
        let mut expected_urls = vec![dl.url.to_string()];
        for u in dl.extra_urls {
            expected_urls.push(u.to_string())
        }
        expected_urls.sort();
        assert_eq!(annex.get_urls(&dl.path), expected_urls);
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct AugmentedInput {
    item: Downloadable,
    success: bool,
}

#[test]
fn test_gamdam_failures() {
    let tmpdir = tempdir().unwrap();
    let tmp_path = tmpdir.path();
    let repo = tmp_path.join("repo");
    let infile = Path::new(DATA_DIR).join("mixed-meta.jsonl");
    let items =
        serde_json::Deserializer::from_str(&read_to_string(infile).expect("Error reading infile"))
            .into_iter::<AugmentedInput>()
            .collect::<Result<Vec<_>, _>>()
            .expect("Error parsing infile");
    let mut p = Command::new(env!("CARGO_BIN_EXE_gamdam"))
        .args([
            "--log-level".as_ref(),
            "DEBUG".as_ref(),
            "-C".as_ref(),
            repo.as_path(),
            "--failures".as_ref(),
            tmp_path.join("failures.jsonl").as_path(),
            "--no-save-on-fail".as_ref(),
        ])
        .stdin(Stdio::piped())
        .spawn()
        .expect("Failed to execute gamdam");
    {
        let mut stdin = p.stdin.take().expect("Child.stdin was unexpectedly None");
        for it in &items {
            serde_json::to_writer(&stdin, &it.item).expect("Error writing input to gamdam");
            _ = stdin.write(b"\n").unwrap();
        }
    }
    let r = p.wait().expect("Error waiting for gamdam");
    assert!(!r.success());
    let annex = Annex::new(repo.clone());
    assert!(!annex.is_clean());
    let mut expected_failures = Vec::new();
    for it in items {
        let dl = it.item;
        if it.success {
            assert!(repo.join(dl.path.as_str()).exists());
            let md = annex.get_metadata(&dl.path);
            for (k, v) in dl.metadata {
                assert_eq!(md.get(&k), Some(&v));
            }
            let mut expected_urls = vec![dl.url.to_string()];
            for u in dl.extra_urls {
                expected_urls.push(u.to_string())
            }
            expected_urls.sort();
            assert_eq!(annex.get_urls(&dl.path), expected_urls);
        } else {
            assert!(!repo.join(dl.path.as_str()).exists());
            expected_failures.push(serde_json::to_string(&dl).unwrap());
        }
    }
    expected_failures.sort();
    let failfile =
        read_to_string(tmp_path.join("failures.jsonl")).expect("Error reading failures.jsonl");
    let mut recorded_failures = failfile.lines().collect::<Vec<_>>();
    recorded_failures.sort();
    assert_eq!(expected_failures, recorded_failures);
}
