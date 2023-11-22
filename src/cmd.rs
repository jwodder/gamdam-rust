use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug)]
pub struct LoggedCommand {
    cmdline: String,
    cmd: Command,
}

impl LoggedCommand {
    pub fn new<I, S, P>(arg0: &str, args: I, cwd: P) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
        P: AsRef<Path>,
    {
        let args = args
            .into_iter()
            .map(|s| OsString::from(s.as_ref()))
            .collect::<Vec<_>>();
        let cmdline = format!(
            "{} {}",
            shell_words::quote(arg0),
            shell_words::join(args.iter().map(|s| s.to_string_lossy()))
        );
        let mut cmd = Command::new(arg0);
        cmd.args(args);
        cmd.current_dir(cwd);
        LoggedCommand { cmdline, cmd }
    }

    pub async fn status(mut self) -> Result<(), CommandError> {
        log::debug!("Running: {}", self.cmdline);
        match self.cmd.status().await {
            Ok(rc) if rc.success() => Ok(()),
            Ok(rc) => Err(CommandError::Exit {
                cmdline: self.cmdline,
                rc,
            }),
            Err(e) => Err(CommandError::Startup {
                cmdline: self.cmdline,
                source: e,
            }),
        }
    }

    pub async fn check_output(mut self) -> Result<String, CommandOutputError> {
        log::debug!("Running: {}", self.cmdline);
        // Use spawn() + wait_with_output() instead of output() so as not to
        // capture stderr
        let child = self
            .cmd
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn();
        match child {
            Ok(child) => match child.wait_with_output().await {
                Ok(output) if output.status.success() => match String::from_utf8(output.stdout) {
                    Ok(s) => Ok(s),
                    Err(e) => Err(CommandOutputError::Decode {
                        cmdline: self.cmdline,
                        source: e.utf8_error(),
                    }),
                },
                Ok(output) => Err(CommandOutputError::Exit {
                    cmdline: self.cmdline,
                    rc: output.status,
                }),
                Err(e) => Err(CommandOutputError::Wait {
                    cmdline: self.cmdline,
                    source: e,
                }),
            },
            Err(e) => Err(CommandOutputError::Startup {
                cmdline: self.cmdline,
                source: e,
            }),
        }
    }
}

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("failed to run `{cmdline}`: {source}")]
    Startup {
        cmdline: String,
        source: std::io::Error,
    },
    #[error("command `{cmdline}` failed: {rc}")]
    Exit { cmdline: String, rc: ExitStatus },
}

#[derive(Debug, Error)]
pub enum CommandOutputError {
    #[error("failed to run `{cmdline}`: {source}")]
    Startup {
        cmdline: String,
        source: std::io::Error,
    },
    #[error("error getting output from `{cmdline}`: {source}")]
    Wait {
        cmdline: String,
        source: std::io::Error,
    },
    #[error("command `{cmdline}` failed: {rc}")]
    Exit { cmdline: String, rc: ExitStatus },
    #[error("could not decode `{cmdline}` output: {source}")]
    Decode {
        cmdline: String,
        source: std::str::Utf8Error,
    },
}
