mod annex;
mod blc;
pub mod util;
use crate::annex::addurl::*;
use crate::annex::metadata::*;
use crate::annex::registerurl::*;
use crate::annex::*;
use crate::util::*;
use anyhow::Context;
use futures::sink::SinkExt;
use futures::stream::TryStreamExt;
use relative_path::RelativePathBuf;
use serde::Deserialize;
use std::collections::{hash_map::Entry, HashMap};
use std::fmt;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::from_utf8;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::fs::create_dir_all;
use tokio::process::Command;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use url::Url;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Downloadable {
    pub path: RelativePathBuf,
    pub url: Url,
    #[serde(default)]
    pub metadata: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub extra_urls: Vec<Url>,
}

#[derive(Debug)]
struct DownloadResult {
    downloadable: Downloadable,
    key: Option<String>,
}

pub struct Report {
    pub downloaded: usize,
    pub failed: usize,
}

pub enum Jobs {
    CPUs,
    Qty(NonZeroUsize),
}

impl fmt::Display for Jobs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Jobs::CPUs => write!(f, "cpus"),
            Jobs::Qty(n) => write!(f, "{n}"),
        }
    }
}

pub struct Gamdam {
    pub repo: PathBuf,
    pub addurl_options: Vec<String>,
    pub addurl_jobs: Jobs,
}

impl Gamdam {
    const ERR_TIMEOUT: Duration = Duration::from_secs(3);

    pub async fn download<I>(&self, items: I) -> Result<Report, anyhow::Error>
    where
        I: IntoIterator<Item = Downloadable>,
    {
        let (sender, receiver) = unbounded_channel();
        // TODO: If one of these fails, shutdown previously-started processes:
        let (addurl_stop, mut addurl_sink, mut addurl_stream) = self.addurl().await?.split();
        let mut metadata = self.metadata().await?;
        let mut registerurl = self.registerurl().await?;
        let in_progress = Arc::new(InProgress::new());
        let r = tokio::try_join!(
            self.feed_addurl(items, &mut addurl_sink, in_progress.clone()),
            self.read_addurl(&mut addurl_stream, in_progress, sender),
            self.add_metadata(receiver, &mut metadata, &mut registerurl),
        );
        // TODO: Log errors returned from shutdown()
        match r {
            Ok((_, report, _)) => {
                _ = tokio::join!(
                    addurl_stop.shutdown(None),
                    metadata.shutdown(None),
                    registerurl.shutdown(None),
                );
                log::info!("Downloaded {} files", report.downloaded);
                if report.failed > 0 {
                    log::error!("{} files failed to download", report.failed);
                }
                Ok(report)
            }
            Err(e) => {
                _ = tokio::join!(
                    addurl_stop.shutdown(Some(Gamdam::ERR_TIMEOUT)),
                    metadata.shutdown(Some(Gamdam::ERR_TIMEOUT)),
                    registerurl.shutdown(Some(Gamdam::ERR_TIMEOUT)),
                );
                Err(e)
            }
        }
    }

    async fn feed_addurl<I>(
        &self,
        items: I,
        addurl_sink: &mut AnnexSink<AddURLInput>,
        in_progress: Arc<InProgress>,
    ) -> Result<(), anyhow::Error>
    where
        I: IntoIterator<Item = Downloadable>,
    {
        for dl in items {
            if in_progress.add(&dl) {
                log::info!("Downloading {} to {}", dl.url, dl.path);
                addurl_sink
                    .send(AddURLInput {
                        url: dl.url.clone(),
                        path: dl.path.clone(),
                    })
                    .await?;
            } else {
                log::warn!(
                    "Multiple entries encountered downloading to {}; discarding extra",
                    dl.path,
                );
            }
        }
        log::debug!("Done feeding URLs to addurl");
        Ok(())
    }

    async fn read_addurl(
        &self,
        addurl_stream: &mut AnnexStream<AddURLOutput>,
        in_progress: Arc<InProgress>,
        sender: UnboundedSender<DownloadResult>,
    ) -> Result<Report, anyhow::Error> {
        let mut downloaded = 0;
        let mut failed = 0;
        while let Some(r) = addurl_stream
            .try_next()
            .await
            .context("Error reading from `git-annex addurl`")?
        {
            let file = match r.file() {
                Some(f) => f.clone(),
                None => anyhow::bail!("`git-annex addurl` outputted a line without a file"),
            };
            match r.check() {
                Ok(AddURLOutput::Progress {
                    byte_progress,
                    total_size,
                    percent_progress,
                    ..
                }) => log::info!(
                    "{}: Downloaded {} / {} bytes ({})",
                    file,
                    byte_progress,
                    total_size.map_or("???".into(), |i| i.to_string()),
                    percent_progress.unwrap_or_else(|| "??.??%".into()),
                ),
                Ok(AddURLOutput::Completion { key, .. }) => {
                    log::info!(
                        "Finished downloading {file} (key = {})",
                        key.clone().unwrap_or_else(|| "<none>".into())
                    );
                    downloaded += 1;
                    let downloadable = in_progress.pop(&file)?;
                    let res = DownloadResult { downloadable, key };
                    // TODO: Do something if send() fails
                    sender.send(res).unwrap();
                }
                Err(e) => {
                    log::error!("{file}: download failed:{e}");
                    failed += 1;
                    let _downloadable = in_progress.pop(&file)?;
                    /*
                    let res = DownloadResult {
                        downloadable,
                        success: false,
                        err: e,
                    };
                    // TODO: Do something if send() fails
                    sender.send(res).unwrap();
                    */
                }
            }
        }
        log::debug!("Done reading from addurl");
        Ok(Report { downloaded, failed })
    }

    async fn add_metadata(
        &self,
        mut receiver: UnboundedReceiver<DownloadResult>,
        metadata: &mut AnnexProcess<MetadataInput, MetadataOutput>,
        registerurl: &mut AnnexProcess<RegisterURLInput, RegisterURLOutput>,
    ) -> Result<(), anyhow::Error> {
        while let Some(r) = receiver.recv().await {
            // if !r.success {continue; }
            let path = r.downloadable.path;
            if !r.downloadable.metadata.is_empty() {
                log::debug!("Setting metadata for {path} ...");
                let input = MetadataInput {
                    file: path.clone(),
                    fields: r.downloadable.metadata,
                };
                match metadata.chat(input).await?.check() {
                    Ok(_) => log::info!("Set metadata on {}", path),
                    Err(e) => log::error!("{path}: setting metadata failed:{e}"),
                }
            }
            if let Some(key) = r.key {
                for u in r.downloadable.extra_urls {
                    log::info!("Registering URL {u} for {path} ...");
                    let input = RegisterURLInput {
                        key: key.clone(),
                        url: u.clone(),
                    };
                    match registerurl.chat(input).await?.check() {
                        Ok(_) => log::info!("Registered URL {u} for {path}"),
                        Err(e) => log::error!("{path}: registering URL {u} failed:{e}"),
                    }
                }
            }
        }
        log::debug!("Done post-processing metadata");
        Ok(())
    }

    async fn addurl(&self) -> Result<AnnexProcess<AddURLInput, AddURLOutput>, anyhow::Error> {
        let jobs = self.addurl_jobs.to_string();
        let mut args = vec![
            "--batch",
            "--with-files",
            "--jobs",
            &jobs,
            "--json",
            "--json-error-messages",
            "--json-progress",
        ];
        args.extend(self.addurl_options.iter().map(String::as_str));
        AnnexProcess::new("addurl", args, &self.repo).await
    }

    async fn metadata(&self) -> Result<AnnexProcess<MetadataInput, MetadataOutput>, anyhow::Error> {
        AnnexProcess::new(
            "metadata",
            ["--batch", "--json", "--json-error-messages"],
            &self.repo,
        )
        .await
    }

    async fn registerurl(
        &self,
    ) -> Result<AnnexProcess<RegisterURLInput, RegisterURLOutput>, anyhow::Error> {
        AnnexProcess::new(
            "registerurl",
            ["--batch", "--json", "--json-error-messages"],
            &self.repo,
        )
        .await
    }
}

struct InProgress {
    data: Mutex<HashMap<RelativePathBuf, Downloadable>>,
}

impl InProgress {
    fn new() -> Self {
        InProgress {
            data: Mutex::new(HashMap::new()),
        }
    }

    fn add(&self, dl: &Downloadable) -> bool {
        let mut data = self.data.lock().unwrap();
        match data.entry(dl.path.clone()) {
            Entry::Occupied(_) => false,
            Entry::Vacant(v) => {
                v.insert(dl.clone());
                true
            }
        }
    }

    fn pop(&self, file: &RelativePathBuf) -> Result<Downloadable, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        match data.remove(file) {
            Some(dl) => Ok(dl),
            None => anyhow::bail!("No record found for download of {file}"),
        }
    }
}

pub async fn ensure_annex_repo<P: AsRef<Path>>(repo: P) -> Result<(), anyhow::Error> {
    let repo = repo.as_ref();
    create_dir_all(&repo)
        .await
        .with_context(|| format!("Error creating directory {}", repo.display()))?;
    log::debug!("Running: git rev-parse --show-toplevel");
    let toplevel = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&repo)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        // Use spawn() + wait_with_output() instead of output() so as not to
        // capture stderr
        .spawn()
        .context("Error running `git rev-parse --show-toplevel`")?
        .wait_with_output()
        .await
        .context("Error getting output from `git rev-parse --show-toplevel`")?;
    let repo: PathBuf = if toplevel.status.success() {
        from_utf8(&toplevel.stdout)
            .with_context(|| {
                format!(
                    "Could not decode `git rev-parse --show-toplevel` output: {:?}",
                    toplevel.stdout
                )
            })?
            .trim()
            .into()
    } else {
        log::info!(
            "{} is not a Git repository; initializing ...",
            repo.display()
        );
        runcmd(["git", "init"], &repo).await?;
        repo.into()
    };
    log::debug!("Using {} as the repository root", repo.display());
    log::debug!("Running: git rev-parse --git-dir");
    let git_dir = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(&repo)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        // Use spawn() + wait_with_output() instead of output() so as not to
        // capture stderr
        .spawn()
        .context("Error running `git rev-parse --git-dir`")?
        .wait_with_output()
        .await
        .context("Error getting output from `git rev-parse --git-dir`")?;
    CommandStatusError::for_status(git_dir.status)
        .context("Command `git rev-parse --git-dir` failed")?;
    let mut path: PathBuf = from_utf8(&git_dir.stdout)
        .with_context(|| {
            format!(
                "Could not decode `git rev-parse --git-dir` output: {:?}",
                git_dir.stdout
            )
        })?
        .trim()
        .into();
    path.push("annex");
    if !path.exists() {
        log::info!(
            "Repository at {} is not a git-annex repository; initializing ...",
            repo.display()
        );
        runcmd(["git-annex", "init"], &repo).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_downloadable_defaults() {
        let s = r#"{"path": "foo/bar/baz.txt", "url": "https://example.com/baz.txt"}"#;
        let parsed = serde_json::from_str::<Downloadable>(s).unwrap();
        assert_eq!(
            parsed,
            Downloadable {
                path: RelativePathBuf::from_path("foo/bar/baz.txt").unwrap(),
                url: Url::parse("https://example.com/baz.txt").unwrap(),
                metadata: HashMap::new(),
                extra_urls: Vec::new(),
            }
        );
    }

    // <https://github.com/udoprog/relative-path/issues/41>
    /*
    #[test]
    fn test_load_downloadable_absolute_path() {
        let s = r#"{"path": "/foo/bar/baz.txt", "url": "https://example.com/baz.txt"}"#;
        match serde_json::from_str::<Downloadable>(s) {
            Err(_) => (),
            Ok(v) => panic!("Deserialization did not fail: {v:?}"),
        }
    }
    */
}
