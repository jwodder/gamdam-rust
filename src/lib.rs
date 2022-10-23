#![allow(dead_code)]
mod annex;
mod blc;
mod util;
use crate::annex::addurl::*;
use crate::annex::metadata::*;
use crate::annex::registerurl::*;
use crate::annex::*;
use anyhow::Context;
use futures::sink::SinkExt;
use futures::stream::TryStreamExt;
use relative_path::RelativePathBuf;
use serde::Deserialize;
use std::collections::{hash_map::Entry, HashMap};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
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
    async fn feed_addurl<I, S>(
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
            let file = r.file().to_relative_path_buf();
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
                    let downloadable = in_progress.pop(&file);
                    let res = DownloadResult { downloadable, key };
                    // TODO: Do something if send() fails
                    sender.send(res).unwrap();
                }
                Err(e) => {
                    log::error!("{file}: download failed:{e}");
                    failed += 1;
                    let _downloadable = in_progress.pop(&file);
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
        self,
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

struct Report {
    downloaded: usize,
    failed: usize,
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

    fn pop(&self, file: &RelativePathBuf) -> Downloadable {
        let mut data = self.data.lock().unwrap();
        match data.remove(file) {
            Some(dl) => dl,
            None => panic!("No record for in-progress download {}", file),
        }
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
