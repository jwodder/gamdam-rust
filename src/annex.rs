#![allow(unused)]
mod addurl;
mod metadata;
mod outputs;
mod registerurl;
pub use addurl::*;
use anyhow::Context;
use async_trait::async_trait;
use bytes::Bytes;
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

pub struct RawAnnexProcess {
    name: String,
    p: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl RawAnnexProcess {
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
        let stdout = p.stdout.take().expect("Child.stdin was unexpectedly None");
        Ok(RawAnnexProcess {
            name: String::from(name),
            p,
            stdin,
            stdout,
        })
    }

    pub async fn shutdown(mut self, timeout: Option<Duration>) -> Result<(), anyhow::Error> {
        drop(self.stdin);
        drop(self.stdout);
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

    pub async fn writeline(&mut self, line: &[u8]) -> Result<(), anyhow::Error> {
        // This function is the one that adds the '\n'
        let mut buf = Vec::with_capacity(line.len() + 1);
        buf.extend_from_slice(line);
        buf.push(b'\n');
        self.stdin
            .write_all(&buf)
            .await
            .with_context(|| format!("Error writing to `git-annex {}`", self.name))?;
        Ok(())
    }

    pub async fn readline(&mut self) -> Result<Bytes, anyhow::Error> {
        unimplemented!()
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

    async fn recv(&mut self) -> Result<Self::Output, anyhow::Error>
    where
        Self::Output: AnnexOutput,
    {
        Self::Output::deserialize(self.process().readline().await?)
    }

    async fn chat(&mut self, value: Self::Input) -> Result<Self::Output, anyhow::Error>
    where
        Self::Input: AnnexInput + Send,
        Self::Output: AnnexOutput,
    {
        self.send(value).await?;
        self.recv().await
    }
}

pub trait AnnexInput {
    fn serialize(self) -> Bytes;
}

pub trait AnnexOutput {
    fn deserialize(data: Bytes) -> Result<Self, anyhow::Error>
    where
        Self: Sized;
}
