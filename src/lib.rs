#![allow(dead_code)]
mod annex;
mod blc;
use crate::annex::addurl::*;
//use crate::annex::metadata::*;
use crate::annex::registerurl::*;
use crate::annex::*;
use relative_path::RelativePathBuf;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
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

pub enum Jobs {
    CPUs,
    Qty(u32),
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
    repo: PathBuf,
    addurl_options: Vec<String>,
    addurl_jobs: Jobs,
}

impl Gamdam {
    async fn addurl(&self) -> Result<AnnexProcess<AddURLInput, AddURLOutput>, anyhow::Error> {
        // TODO: Figure out how to do this without creating a bunch of Strings
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
