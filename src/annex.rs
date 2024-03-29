pub(crate) mod addurl;
pub(crate) mod metadata;
pub(crate) mod outputs;
pub(crate) mod registerurl;
use crate::blc::{BinaryLinesCodec, BinaryLinesCodecError};
use anyhow::Context;
use bytes::Bytes;
use futures_util::{SinkExt, TryStream, TryStreamExt};
use indenter::indented;
use serde::Deserialize;
use std::ffi::OsStr;
use std::fmt::{self, Write};
use std::future::Future;
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

pub(crate) struct AnnexProcess<Input, Output> {
    name: String,
    p: Child,
    stdin: AnnexSink<Input>,
    stdout: AnnexStream<Output>,
}

impl<Input, Output> AnnexProcess<Input, Output> {
    const MAX_INPUT_LEN: usize = 65535;
    const ERR_TIMEOUT: Duration = Duration::from_secs(3);

    pub(crate) fn new<I, S, P>(name: &str, args: I, repo: P) -> Result<Self, anyhow::Error>
    where
        I: IntoIterator<Item = S> + Send,
        S: AsRef<OsStr> + Send,
        P: AsRef<Path> + Send,
    {
        let args = args
            .into_iter()
            .map(|s| s.as_ref().to_os_string())
            .collect::<Vec<_>>();
        let cmdstr = format!(
            "git-annex {} {}",
            shell_words::quote(name),
            shell_words::join(args.iter().map(|s| s.to_string_lossy()))
        );
        log::debug!("Opening pipe to: {cmdstr}");
        let mut p = Command::new("git-annex")
            .arg(name)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .current_dir(repo)
            .spawn()
            .with_context(|| format!("Error spawning `{cmdstr}`"))?;
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

    pub(crate) async fn in_context<Func, F, T, E>(self, func: Func) -> Result<T, E>
    where
        Input: Send,
        Output: Send,
        Func: (FnOnce(AnnexIO<Input, Output>) -> F) + Send,
        F: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: Send,
    {
        let (terminator, io) = self.split();
        let r = func(io).await;
        if r.is_ok() {
            terminator.wait(None).await;
        } else {
            terminator.terminate(Some(Self::ERR_TIMEOUT)).await;
        }
        r
    }

    pub(crate) fn split(self) -> (AnnexTerminator, AnnexIO<Input, Output>) {
        let terminator = AnnexTerminator {
            name: self.name.clone(),
            p: self.p,
        };
        let io = AnnexIO {
            name: self.name,
            stdin: self.stdin,
            stdout: self.stdout,
        };
        (terminator, io)
    }
}

pub(crate) struct AnnexTerminator {
    name: String,
    p: Child,
}

impl AnnexTerminator {
    pub(crate) async fn wait(mut self, timeout: Option<Duration>) {
        log::debug!("Waiting for `git-annex {}` command to exit", self.name);
        let rc = match timeout {
            None => self.p.wait().await,
            Some(delta) => {
                if let Ok(rc) = time::timeout(delta, self.p.wait()).await {
                    rc
                } else {
                    log::warn!("`git-annex {}` did not exit in time; killing", self.name);
                    if let Err(e) = self.p.kill().await {
                        log::warn!("Error killing `git-annex {}` command: {}", self.name, e);
                    }
                    return;
                }
            }
        };
        match rc {
            Ok(rc) => {
                if !rc.success() {
                    log::warn!(
                        "Command `git-annex {}` exited unsuccessfully: {}",
                        self.name,
                        rc
                    );
                }
            }
            Err(e) => log::warn!(
                "Error waiting for `git-annex {}` command to terminate: {}",
                self.name,
                e
            ),
        }
    }

    #[cfg(unix)]
    #[allow(unused_mut)]
    pub(crate) async fn terminate(mut self, timeout: Option<Duration>) {
        use nix::{
            sys::signal::{kill, SIGTERM},
            unistd::Pid,
        };
        log::debug!("Forcibly terminating `git-annex {}` command", self.name);
        if let Some(Ok(pid)) = self.p.id().map(TryInto::try_into) {
            let pid = Pid::from_raw(pid);
            if let Err(e) = kill(pid, SIGTERM) {
                log::warn!(
                    "Error sending SIGTERM to `git-annex {}` command: {}",
                    self.name,
                    e
                );
            } else {
                self.wait(timeout).await;
            }
        } else {
            log::warn!(
                "Could not construct pid for `git-annex {}` command",
                self.name
            );
        }
    }

    #[cfg(not(unix))]
    #[allow(unused_variables)]
    pub(crate) async fn terminate(mut self, timeout: Option<Duration>) {
        log::debug!("Forcibly killing `git-annex {}` command", self.name);
        if let Err(e) = self.p.kill().await {
            log::warn!("Error killing `git-annex {}` command: {}", self.name, e);
        }
    }
}

pub(crate) struct AnnexIO<Input, Output> {
    name: String,
    stdin: AnnexSink<Input>,
    stdout: AnnexStream<Output>,
}

impl<Input, Output> AnnexIO<Input, Output> {
    pub(crate) fn split(self) -> (AnnexSink<Input>, AnnexStream<Output>) {
        (self.stdin, self.stdout)
    }

    pub(crate) async fn chat(&mut self, value: Input) -> Result<Output, anyhow::Error>
    where
        Input: AnnexInput + Send,
        <Input as AnnexInput>::Error: Into<BinaryLinesCodecError>,
        <StdoutTransport as TryStream>::Error: From<serde_json::Error>,
        Output: for<'a> Deserialize<'a> + Unpin + Send,
    {
        // send() always flushes
        self.stdin
            .send(value)
            .await
            .with_context(|| format!("Error writing to `git-annex {}`", self.name))?;
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
}

pub(crate) trait AnnexInput {
    type Error;

    fn for_input(&self) -> Result<Bytes, Self::Error>;
}

pub(crate) struct AnnexCodec;

impl<I: AnnexInput> Serializer<I> for AnnexCodec {
    type Error = I::Error;

    fn serialize(self: Pin<&mut Self>, item: &I) -> Result<Bytes, Self::Error> {
        item.for_input()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnexError(Vec<String>);

impl fmt::Display for AnnexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.len() {
            0 => write!(f, " <no error message>"),
            1 => write!(f, " {}", self.0[0]),
            _ => {
                let mut ff = indented(f).with_str("    ");
                write!(ff, "\n\n")?;
                for m in &self.0 {
                    write!(ff, "{m}")?;
                    if !m.ends_with('\n') {
                        writeln!(ff)?;
                    }
                }
                writeln!(ff)
            }
        }
    }
}

impl std::error::Error for AnnexError {}
