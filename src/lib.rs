mod annex;
pub mod blc;
pub mod cmd;
mod filepath;
use crate::annex::addurl::*;
use crate::annex::metadata::*;
use crate::annex::registerurl::*;
pub use crate::annex::*;
use crate::cmd::*;
pub use crate::filepath::*;
use anyhow::Context;
use futures_util::{SinkExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::Entry, HashMap};
use std::fmt;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::fs::create_dir_all;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use url::Url;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Downloadable {
    pub path: FilePath,
    pub url: Url,
    #[serde(default)]
    pub metadata: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub extra_urls: Vec<Url>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadResult {
    pub downloadable: Downloadable,
    pub download: Result<(), AnnexError>,
    pub key: Option<String>,
    pub metadata_added: Option<Result<(), AnnexError>>,
    pub urls_added: HashMap<Url, Result<(), AnnexError>>,
}

impl DownloadResult {
    pub fn success(&self) -> bool {
        self.download.is_ok()
            && !matches!(self.metadata_added, Some(Err(_)))
            && self.urls_added.values().all(Result::is_ok)
    }

    fn successful_download(downloadable: Downloadable, key: Option<String>) -> DownloadResult {
        DownloadResult {
            downloadable,
            download: Ok(()),
            key,
            metadata_added: None,
            urls_added: HashMap::new(),
        }
    }

    fn failed_download(downloadable: Downloadable, err: AnnexError) -> DownloadResult {
        DownloadResult {
            downloadable,
            download: Err(err),
            key: None,
            metadata_added: None,
            urls_added: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report {
    pub successful: Vec<DownloadResult>,
    pub failed: Vec<DownloadResult>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Jobs {
    CPUs,
    Qty(NonZeroUsize),
}

impl fmt::Display for Jobs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Jobs::CPUs => write!(f, "cpus"),
            Jobs::Qty(n) => write!(f, "{n}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gamdam {
    pub repo: PathBuf,
    pub addurl_options: Vec<String>,
    pub addurl_jobs: Jobs,
}

impl Gamdam {
    pub async fn download<I>(&self, items: I) -> Result<Report, anyhow::Error>
    where
        I: IntoIterator<Item = Downloadable> + Send,
        I::IntoIter: Send,
    {
        let r = self
            .addurl()?
            .in_context(|addurl| async {
                self.metadata()?
                    .in_context(|metadata| async move {
                        self.registerurl()?
                            .in_context(|registerurl| async move {
                                let in_progress = Arc::new(InProgress::new());
                                let (sender, receiver) = unbounded_channel();
                                let (addurl_sink, addurl_stream) = addurl.split();
                                tokio::try_join!(
                                    self.feed_addurl(items, addurl_sink, in_progress.clone()),
                                    self.read_addurl(addurl_stream, in_progress, sender),
                                    self.add_metadata(receiver, metadata, registerurl),
                                )
                            })
                            .await
                    })
                    .await
            })
            .await;
        match r {
            Ok(((), (), report)) => {
                log::info!("Downloaded {}", quantify(report.successful.len(), "file"));
                if !report.failed.is_empty() {
                    log::error!(
                        "{} failed to download",
                        quantify(report.failed.len(), "file")
                    );
                }
                Ok(report)
            }
            Err(e) => Err(e),
        }
    }

    async fn feed_addurl<I>(
        &self,
        items: I,
        mut addurl_sink: AnnexSink<AddURLInput>,
        in_progress: Arc<InProgress>,
    ) -> Result<(), anyhow::Error>
    where
        I: IntoIterator<Item = Downloadable> + Send,
        I::IntoIter: Send,
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
        mut addurl_stream: AnnexStream<AddURLOutput>,
        in_progress: Arc<InProgress>,
        sender: UnboundedSender<DownloadResult>,
    ) -> Result<(), anyhow::Error> {
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
                    let downloadable = in_progress.pop(&file)?;
                    let res = DownloadResult::successful_download(downloadable, key);
                    // TODO: Do something if send() fails
                    let _ = sender.send(res);
                }
                Err(e) => {
                    log::error!("{file}: download failed:{e}");
                    let downloadable = in_progress.pop(&file)?;
                    let res = DownloadResult::failed_download(downloadable, e);
                    // TODO: Do something if send() fails
                    let _ = sender.send(res);
                }
            }
        }
        log::debug!("Done reading from addurl");
        Ok(())
    }

    async fn add_metadata(
        &self,
        mut receiver: UnboundedReceiver<DownloadResult>,
        mut metadata: AnnexIO<MetadataInput, MetadataOutput>,
        mut registerurl: AnnexIO<RegisterURLInput, RegisterURLOutput>,
    ) -> Result<Report, anyhow::Error> {
        let mut successful = Vec::new();
        let mut failed = Vec::new();
        while let Some(mut r) = receiver.recv().await {
            let path = &r.downloadable.path;
            if r.download.is_err() {
                failed.push(r);
            } else if let Some(ref key) = r.key {
                let mut success = true;
                if !r.downloadable.metadata.is_empty() {
                    log::info!("Setting metadata for {path} ...");
                    let input = MetadataInput {
                        key: key.clone(),
                        fields: r.downloadable.metadata.clone(),
                    };
                    match metadata.chat(input).await?.check() {
                        Ok(_) => {
                            log::info!("Set metadata on {path}");
                            r.metadata_added = Some(Ok(()));
                        }
                        Err(e) => {
                            log::error!("{path}: setting metadata failed:{e}");
                            r.metadata_added = Some(Err(e));
                            success = false;
                        }
                    }
                }
                for u in &r.downloadable.extra_urls {
                    log::info!("Registering URL {u} for {path} ...");
                    let input = RegisterURLInput {
                        key: key.clone(),
                        url: u.clone(),
                    };
                    match registerurl.chat(input).await?.check() {
                        Ok(_) => {
                            log::info!("Registered URL {u} for {path}");
                            r.urls_added.insert(u.clone(), Ok(()));
                        }
                        Err(e) => {
                            log::error!("{path}: registering URL {u} failed:{e}");
                            r.urls_added.insert(u.clone(), Err(e));
                            success = false;
                        }
                    }
                }
                if success {
                    successful.push(r);
                } else {
                    failed.push(r);
                }
            } else {
                if !r.downloadable.metadata.is_empty() || !r.downloadable.extra_urls.is_empty() {
                    log::warn!("Cannot set metadata for {path} as it was not assigned a key");
                }
                successful.push(r);
            }
        }
        log::debug!("Done post-processing metadata");
        Ok(Report { successful, failed })
    }

    fn addurl(&self) -> Result<AnnexProcess<AddURLInput, AddURLOutput>, anyhow::Error> {
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
        AnnexProcess::new("addurl", args, &self.repo)
    }

    fn metadata(&self) -> Result<AnnexProcess<MetadataInput, MetadataOutput>, anyhow::Error> {
        AnnexProcess::new(
            "metadata",
            ["--batch", "--json", "--json-error-messages"],
            &self.repo,
        )
    }

    fn registerurl(
        &self,
    ) -> Result<AnnexProcess<RegisterURLInput, RegisterURLOutput>, anyhow::Error> {
        AnnexProcess::new(
            "registerurl",
            ["--batch", "--json", "--json-error-messages"],
            &self.repo,
        )
    }
}

struct InProgress {
    data: Mutex<HashMap<FilePath, Downloadable>>,
}

impl InProgress {
    fn new() -> Self {
        InProgress {
            data: Mutex::new(HashMap::new()),
        }
    }

    fn add(&self, dl: &Downloadable) -> bool {
        let mut data = self.data.lock().expect("Mutex should not be poisoned");
        match data.entry(dl.path.clone()) {
            Entry::Occupied(_) => false,
            Entry::Vacant(v) => {
                v.insert(dl.clone());
                true
            }
        }
    }

    fn pop(&self, file: &FilePath) -> Result<Downloadable, anyhow::Error> {
        let mut data = self.data.lock().expect("Mutex should not be poisoned");
        match data.remove(file) {
            Some(dl) => Ok(dl),
            None => anyhow::bail!("No record found for download of {file}"),
        }
    }
}

pub async fn ensure_annex_repo<P: AsRef<Path> + Send>(repo: P) -> Result<(), anyhow::Error> {
    let repo = repo.as_ref();
    create_dir_all(&repo)
        .await
        .with_context(|| format!("Error creating directory {}", repo.display()))?;
    let toplevel = LoggedCommand::new("git", ["rev-parse", "--show-toplevel"], repo)
        .check_output()
        .await;
    let repo: PathBuf = match toplevel {
        Ok(s) => s.trim().into(),
        Err(CommandOutputError::Exit { .. }) => {
            log::info!(
                "{} is not a Git repository; initializing ...",
                repo.display()
            );
            LoggedCommand::new("git", ["init"], repo).status().await?;
            repo.into()
        }
        Err(e) => return Err(e.into()),
    };
    log::debug!("Using {} as the repository root", repo.display());
    let git_dir: PathBuf = LoggedCommand::new("git", ["rev-parse", "--git-dir"], &repo)
        .check_output()
        .await?
        .trim()
        .into();
    if !repo.join(git_dir).join("annex").exists() {
        log::info!(
            "Repository at {} is not a git-annex repository; initializing ...",
            repo.display()
        );
        LoggedCommand::new("git-annex", ["init"], &repo)
            .status()
            .await?;
    }
    Ok(())
}

fn quantify(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
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
                path: FilePath::try_from("foo/bar/baz.txt").unwrap(),
                url: Url::parse("https://example.com/baz.txt").unwrap(),
                metadata: HashMap::new(),
                extra_urls: Vec::new(),
            }
        );
    }

    #[test]
    fn test_load_downloadable_absolute_path() {
        let s = r#"{"path": "/foo/bar/baz.txt", "url": "https://example.com/baz.txt"}"#;
        assert!(serde_json::from_str::<Downloadable>(s).is_err());
    }
}
