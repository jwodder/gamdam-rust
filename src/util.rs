use anyhow::Context;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::Path;
use std::process::ExitStatus;
use tokio::process::Command;

pub async fn runcmd<I, S, P>(argv: I, cwd: P) -> Result<(), anyhow::Error>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
    P: AsRef<Path>,
{
    let mut argiter = argv.into_iter().map(|s| OsString::from(s.as_ref()));
    let arg0 = argiter
        .next()
        .expect("runcmd() called without any command arguments");
    let args = argiter.collect::<Vec<_>>();
    let cmdstr = format!(
        "{} {}",
        shell_words::quote(&arg0.to_string_lossy()),
        shell_words::join(args.iter().map(|s| s.to_string_lossy()))
    );
    log::debug!("Running: {cmdstr}");
    let r = Command::new(arg0)
        .args(args)
        .current_dir(cwd)
        .status()
        .await
        .with_context(|| format!("Error running `{cmdstr}`"))?;
    CommandStatusError::for_status(r).with_context(|| format!("Command `{cmdstr}` failed"))
}

#[derive(Debug)]
pub(crate) struct CommandStatusError(ExitStatus);

impl CommandStatusError {
    pub(crate) fn for_status(r: ExitStatus) -> Result<(), Self> {
        if r.success() {
            Ok(())
        } else {
            Err(CommandStatusError(r))
        }
    }
}

impl fmt::Display for CommandStatusError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Command terminated unsuccessfully: {}", self.0)
    }
}

impl std::error::Error for CommandStatusError {}
