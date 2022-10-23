#![allow(dead_code)]
pub(crate) mod addurl;
pub(crate) mod metadata;
pub(crate) mod outputs;
pub(crate) mod registerurl;
use crate::blc::{BinaryLinesCodec, BinaryLinesCodecError};
use anyhow::Context;
use bytes::Bytes;
use futures::sink::SinkExt;
use futures::stream::{TryStream, TryStreamExt};
use log::{debug, warn};
use serde::Deserialize;
use std::ffi::OsStr;
use std::fmt;
use std::path::Path;
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time;
use tokio_serde::formats::Json;
use tokio_serde::{Framed, Serializer};
use tokio_util::codec::{FramedRead, FramedWrite};

pub(crate) type StdinTransport = FramedWrite<ChildStdin, BinaryLinesCodec>;
pub(crate) type StdoutTransport = FramedRead<ChildStdout, BinaryLinesCodec>;
pub(crate) type AnnexSink<Input> = Framed<StdinTransport, (), Input, AnnexCodec>;
pub(crate) type AnnexStream<Output> = Framed<StdoutTransport, Output, (), Json<Output, ()>>;

pub struct AnnexProcess<Input, Output> {
    name: String,
    p: Child,
    stdin: AnnexSink<Input>,
    stdout: AnnexStream<Output>,
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

    pub async fn chat(&mut self, value: Input) -> Result<Output, anyhow::Error>
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
                return Err(e)
                    .with_context(|| format!("Error writing to `git-annex {}`", self.name))
            }
        }
        match self
            .stdout
            .try_next()
            .await
            .with_context(|| format!("Error reading from `git-annex {}`", self.name))?
        {
            Some(r) => Ok(r),
            None => anyhow::bail!(
                "`git-annex {}` terminated before providing output",
                self.name
            ),
        }
    }

    pub fn split(self) -> (AnnexTerminator, AnnexSink<Input>, AnnexStream<Output>) {
        let terminator = AnnexTerminator {
            name: self.name,
            p: self.p,
        };
        (terminator, self.stdin, self.stdout)
    }

    pub async fn shutdown(self, timeout: Option<Duration>) -> Result<(), anyhow::Error> {
        // This drops stdin & stdout:
        let (terminator, _, _) = self.split();
        terminator.shutdown(timeout).await
    }
}

pub struct AnnexTerminator {
    name: String,
    p: Child,
}

impl AnnexTerminator {
    pub async fn shutdown(mut self, timeout: Option<Duration>) -> Result<(), anyhow::Error> {
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

#[derive(Debug)]
pub(crate) struct AnnexError(Vec<String>);

impl fmt::Display for AnnexError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0.len() {
            0 => write!(f, " <no error message>"),
            1 => write!(f, " {}", self.0[0]),
            _ => {
                write!(f, "\n\n")?;
                for m in &self.0 {
                    write!(f, "    {}", m)?;
                }
                writeln!(f)
            }
        }
    }
}

impl std::error::Error for AnnexError {}
