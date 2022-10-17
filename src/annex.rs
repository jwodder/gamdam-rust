#![allow(unused)]
mod addurl;
mod metadata;
mod outputs;
mod registerurl;
pub use addurl::*;
use async_trait::async_trait;
use bytes::Bytes;
use log::debug;
pub use metadata::*;
pub use registerurl::*;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

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
            .spawn()?;
        let stdin = p.stdin.take().expect("Child.stdin was unexpectedly None");
        let stdout = p.stdout.take().expect("Child.stdin was unexpectedly None");
        Ok(RawAnnexProcess {
            name: String::from(name),
            p,
            stdin,
            stdout,
        })
    }

    pub async fn writeline(&self, line: &[u8]) -> Result<(), anyhow::Error> {
        // This function is the one that adds the '\n'
        unimplemented!()
    }

    pub async fn readline(&self) -> Result<Bytes, anyhow::Error> {
        unimplemented!()
    }

    // TODO: Method(s) for closing/terminating/killing
}

#[async_trait]
pub trait AnnexProcess {
    type Input;
    type Output;

    fn process(&self) -> &RawAnnexProcess;

    // TODO: Method for passing to a Func and closing/terminating/killing on
    // return

    async fn send(&self, value: Self::Input) -> Result<(), anyhow::Error>
    where
        Self::Input: AnnexInput + Send,
    {
        self.process().writeline(&value.serialize()).await
    }

    async fn recv(&self) -> Result<Self::Output, anyhow::Error>
    where
        Self::Output: AnnexOutput,
    {
        Self::Output::deserialize(self.process().readline().await?)
    }

    async fn chat(&self, value: Self::Input) -> Result<Self::Output, anyhow::Error>
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
