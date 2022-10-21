#![allow(unused)]
mod addurl;
mod metadata;
mod outputs;
mod registerurl;
pub use addurl::*;
use anyhow::Context;
use async_trait::async_trait;
use futures::sink::SinkExt;
use log::{debug, warn};
pub use metadata::*;
pub use registerurl::*;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time;
use tokio_stream::{Stream, StreamExt};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

pub struct RawAnnexProcess {
    name: String,
    p: Child,
    stdin: FramedWrite<ChildStdin, LinesCodec>,
    stdout: FramedRead<ChildStdout, LinesCodec>,
}

impl RawAnnexProcess {
    pub const MAX_INPUT_LEN: usize = 65535;

    pub async fn new<I, S, P>(name: &str, args: I, repo: P) -> Result<Self, anyhow::Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
        P: AsRef<Path>,
    {
        // TODO: Log full command line
        debug!("Running `git-annex {name}` command");
        let mut p = Command::new("git-annex")
            .arg(name)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .current_dir(repo)
            .spawn()
            .with_context(|| format!("Error spawning `git-annex {name}`"))?;
        let stdin = p.stdin.take().expect("Child.stdin was unexpectedly None");
        let stdout = p.stdout.take().expect("Child.stdout was unexpectedly None");
        Ok(RawAnnexProcess {
            name: String::from(name),
            p,
            stdin: FramedWrite::new(stdin, LinesCodec::new()),
            stdout: FramedRead::new(stdout, LinesCodec::new_with_max_length(Self::MAX_INPUT_LEN)),
        })
    }

    pub async fn shutdown(mut self, timeout: Option<Duration>) -> Result<(), anyhow::Error> {
        drop(self.stdin.into_inner());
        drop(self.stdout.into_inner());
        debug!("Waiting for `git-annex {}` to terminate", self.name);
        let rc = match timeout {
            None => self.p.wait().await,
            Some(delta) => match time::timeout(delta, self.p.wait()).await {
                Err(_) => {
                    warn!(
                        "`git-annex {}` did not terminate in time; killing",
                        self.name
                    );
                    self.p
                        .kill()
                        .await
                        .with_context(|| format!("Error killing `git-annex {}`", self.name))?;
                    return Ok(());
                }
                Ok(rc) => rc,
            },
        }
        .with_context(|| format!("Error waiting for `git-annex {}` to terminate", self.name))?;
        if !rc.success() {
            match rc.code() {
                Some(r) => warn!(
                    "`git-annex {}` command exited with return code {}",
                    self.name, r
                ),
                None => warn!("`git-annex {}` command was killed by a signal", self.name),
            }
        }
        Ok(())
    }

    pub async fn writeline(&mut self, line: &str) -> Result<(), anyhow::Error> {
        // The LinesCodec adds the '\n'
        // send() always flushes
        self.stdin
            .send(line)
            .await
            .with_context(|| format!("Error writing to `git-annex {}`", self.name))
    }

    pub async fn readline(&mut self) -> Option<Result<String, anyhow::Error>> {
        self.stdout
            .next()
            .await
            .map(|r| r.with_context(|| format!("Error reading from `git-annex {}`", self.name)))
    }
}

#[async_trait]
pub trait AnnexProcess {
    type Input;
    type Output;

    fn process(&mut self) -> &mut RawAnnexProcess;

    // TODO: Method for passing to a Func and closing/terminating/killing on
    // return

    async fn send(&mut self, value: Self::Input) -> Result<(), anyhow::Error>
    where
        Self::Input: AnnexInput + Send,
    {
        self.process().writeline(&value.serialize()).await
    }

    async fn recv(&mut self) -> Option<Result<Self::Output, anyhow::Error>>
    where
        Self::Output: AnnexOutput,
    {
        match self.process().readline().await {
            Some(Ok(v)) => Some(Self::Output::deserialize(&v)),
            Some(Err(e)) => Some(Err(e)),
            None => None,
        }
    }

    async fn chat(&mut self, value: Self::Input) -> Option<Result<Self::Output, anyhow::Error>>
    where
        Self::Input: AnnexInput + Send,
        Self::Output: AnnexOutput,
    {
        match self.send(value).await {
            Ok(_) => (),
            Err(e) => return Some(Err(e)),
        }
        // TODO: Error if recv() returns None
        self.recv().await
    }
}

pub trait AnnexInput {
    fn serialize(self) -> String;
}

pub trait AnnexOutput {
    fn deserialize(data: &str) -> Result<Self, anyhow::Error>
    where
        Self: Sized;
}
