pub(crate) mod addurl;
pub(crate) mod metadata;
pub(crate) mod outputs;
pub(crate) mod registerurl;
use crate::blc::{BinaryLinesCodec, BinaryLinesCodecError};
use anyhow::Context;
use bytes::Bytes;
use cfg_if::cfg_if;
use futures::sink::SinkExt;
use futures::stream::{TryStream, TryStreamExt};
use serde::Deserialize;
use std::ffi::OsStr;
use std::fmt;
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

cfg_if! {
    if #[cfg(unix)] {
        use nix::sys::signal::{kill, SIGTERM};
        use nix::unistd::Pid;
    }
}

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

    pub(crate) async fn new<I, S, P>(name: &str, args: I, repo: P) -> Result<Self, anyhow::Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
        P: AsRef<Path>,
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
        Func: FnOnce(AnnexIO<Input, Output>) -> F,
        F: Future<Output = Result<T, E>>,
    {
        let (mut terminator, io) = self.split();
        let r = func(io).await;
        if r.is_ok() {
            terminator.wait(None).await
        } else {
            terminator.terminate(Some(Self::ERR_TIMEOUT)).await
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
    pub(crate) async fn wait(&mut self, timeout: Option<Duration>) {
        log::debug!("Waiting for `git-annex {}` command to exit", self.name);
        let rc = match timeout {
            None => self.p.wait().await,
            Some(delta) => match time::timeout(delta, self.p.wait()).await {
                Ok(rc) => rc,
                Err(_) => {
                    log::warn!("`git-annex {}` did not exit in time; killing", self.name);
                    if let Err(e) = self.p.kill().await {
                        log::warn!("Error killing `git-annex {}` command: {}", self.name, e);
                    }
                    return;
                }
            },
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

    pub(crate) async fn terminate(&mut self, #[allow(unused)] timeout: Option<Duration>) {
        cfg_if! {
            if #[cfg(unix)] {
                log::debug!("Forcibly terminating `git-annex {}` command", self.name);
                if let Some(Ok(pid)) = self.p.id().map(TryInto::try_into) {
                    let pid = Pid::from_raw(pid);
                    if let Err(e) = kill(pid, SIGTERM) {
                        log::warn!("Error sending SIGTERM to `git-annex {}` command: {}", self.name, e);
                    } else {
                        self.wait(timeout).await
                    }
                } else {
                    log::warn!("Could not construct pid for `git-annex {}` command", self.name);
                }
            } else {
                log::debug!("Forcibly killing `git-annex {}` command", self.name);
                if let Err(e) = self.p.kill().await {
                    log::warn!("Error killing `git-annex {}` command: {}", self.name, e);
                }
            }
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
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0.len() {
            0 => write!(f, " <no error message>"),
            1 => write!(f, " {}", self.0[0]),
            _ => {
                write!(f, "\n\n")?;
                for m in &self.0 {
                    write!(f, "    {m}")?;
                    if !m.ends_with('\n') {
                        writeln!(f)?;
                    }
                }
                writeln!(f)
            }
        }
    }
}

impl std::error::Error for AnnexError {}
