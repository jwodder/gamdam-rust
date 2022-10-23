use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::Path;
use std::process::ExitStatus;
use std::process::Stdio;
use tokio::process::Command;

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
                        source: e,
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

#[derive(Debug)]
pub enum CommandError {
    Startup {
        cmdline: String,
        source: std::io::Error,
    },
    Exit {
        cmdline: String,
        rc: ExitStatus,
    },
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CommandError::Startup { cmdline, source } => {
                write!(f, "Failed to run `{cmdline}`: {source}")
            }
            CommandError::Exit { cmdline, rc } => write!(f, "Command `{cmdline}` failed: {rc}"),
        }
    }
}

impl std::error::Error for CommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CommandError::Startup { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum CommandOutputError {
    Startup {
        cmdline: String,
        source: std::io::Error,
    },
    Wait {
        cmdline: String,
        source: std::io::Error,
    },
    Exit {
        cmdline: String,
        rc: ExitStatus,
    },
    Decode {
        cmdline: String,
        source: std::string::FromUtf8Error,
    },
}

impl fmt::Display for CommandOutputError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CommandOutputError::Startup { cmdline, source } => {
                write!(f, "Failed to run `{cmdline}`: {source}")
            }
            CommandOutputError::Wait { cmdline, source } => {
                write!(f, "Error getting output from `{cmdline}`: {source}")
            }
            CommandOutputError::Exit { cmdline, rc } => {
                write!(f, "Command `{cmdline}` failed: {rc}")
            }
            CommandOutputError::Decode { cmdline, source } => {
                write!(f, "Could not decode `{cmdline}` output: {source}")
            }
        }
    }
}

impl std::error::Error for CommandOutputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CommandOutputError::Startup { source, .. } => Some(source),
            CommandOutputError::Wait { source, .. } => Some(source),
            CommandOutputError::Decode { source, .. } => Some(source),
            _ => None,
        }
    }
}
