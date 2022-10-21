#![allow(dead_code)]
pub(crate) mod addurl;
pub(crate) mod metadata;
pub(crate) mod outputs;
pub(crate) mod registerurl;
use crate::blc::{BinaryLinesCodec, BinaryLinesCodecError};
use anyhow::Context;
use bytes::Bytes;
use futures::sink::SinkExt;
use futures::stream::{StreamExt, TryStream};
use log::{debug, warn};
use serde::Deserialize;
use std::ffi::OsStr;
use std::path::Path;
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time;
use tokio_serde::formats::Json;
use tokio_serde::{Framed, Serializer};
use tokio_util::codec::{FramedRead, FramedWrite};

type StdinTransport = FramedWrite<ChildStdin, BinaryLinesCodec>;
type StdoutTransport = FramedRead<ChildStdout, BinaryLinesCodec>;

pub struct AnnexProcess<Input, Output> {
    name: String,
    p: Child,
    stdin: Framed<StdinTransport, (), Input, AnnexCodec>,
    stdout: Framed<StdoutTransport, Output, (), Json<Output, ()>>,
}

impl<Input, Output> AnnexProcess<Input, Output> {
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
        Ok(AnnexProcess {
            name: String::from(name),
            p,
            stdin: Framed::new(FramedWrite::new(stdin, BinaryLinesCodec::new()), AnnexCodec),
            stdout: Framed::new(
                FramedRead::new(
                    stdout,
                    BinaryLinesCodec::new_with_max_length(Self::MAX_INPUT_LEN),
                ),
                Json::default(),
            ),
        })
    }

    async fn chat(&mut self, value: Input) -> Option<Result<Output, anyhow::Error>>
    where
        Input: AnnexInput,
        <Input as AnnexInput>::Error: Into<BinaryLinesCodecError>,
        <StdoutTransport as TryStream>::Error: From<serde_json::Error>,
        Output: for<'a> Deserialize<'a> + std::marker::Unpin,
    {
        // send() always flushes
        match self.stdin.send(value).await {
            Ok(_) => (),
            Err(e) => {
                return Some(
                    Err(e).with_context(|| format!("Error writing to `git-annex {}`", self.name)),
                )
            }
        }
        // TODO: Error if next() returns None
        // TODO: Use futures::stream::TryStreamExt's try_next()
        self.stdout
            .next()
            .await
            .map(|r| r.with_context(|| format!("Error reading from `git-annex {}`", self.name)))
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
}

pub trait AnnexInput {
    type Error;

    fn for_input(&self) -> Result<Bytes, Self::Error>;
}

pub struct AnnexCodec;

impl<I: AnnexInput> Serializer<I> for AnnexCodec {
    type Error = I::Error;

    fn serialize(self: Pin<&mut Self>, item: &I) -> Result<Bytes, Self::Error> {
        item.for_input()
    }
}
