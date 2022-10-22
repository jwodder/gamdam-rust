#![allow(dead_code)]
use anyhow::Context;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::str::from_utf8;
use tokio::fs::create_dir_all;
use tokio::process::Command;

pub(crate) async fn ensure_annex_repo<P: AsRef<Path>>(repo: P) -> Result<(), anyhow::Error> {
    let repo = repo.as_ref();
    create_dir_all(&repo)
        .await
        .with_context(|| format!("Error creating directory {}", repo.display()))?;
    log::debug!("Running: git rev-parse --show-toplevel");
    let toplevel = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&repo)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        // Use spawn() + wait_with_output() instead of output() so as not to
        // capture stderr
        .spawn()
        .context("Error running `git rev-parse --show-toplevel`")?
        .wait_with_output()
        .await
        .context("Error getting output from `git rev-parse --show-toplevel`")?;
    let repo: PathBuf = if toplevel.status.success() {
        from_utf8(&toplevel.stdout)
            .with_context(|| {
                format!(
                    "Could not decode `git rev-parse --show-toplevel` output: {:?}",
                    toplevel.stdout
                )
            })?
            .trim()
            .into()
    } else {
        log::info!(
            "{} is not a Git repository; initializing ...",
            repo.display()
        );
        runcmd(["git", "init"], &repo).await?;
        repo.into()
    };
    log::debug!("Using {} as the repository root", repo.display());
    log::debug!("Running: git rev-parse --git-dir");
    let git_dir = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(&repo)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        // Use spawn() + wait_with_output() instead of output() so as not to
        // capture stderr
        .spawn()
        .context("Error running `git rev-parse --git-dir`")?
        .wait_with_output()
        .await
        .context("Error getting output from `git rev-parse --git-dir`")?;
    CommandStatusError::for_status(git_dir.status)
        .context("Command `git rev-parse --git-dir` failed")?;
    let mut path: PathBuf = from_utf8(&git_dir.stdout)
        .with_context(|| {
            format!(
                "Could not decode `git rev-parse --git-dir` output: {:?}",
                git_dir.stdout
            )
        })?
        .trim()
        .into();
    path.push("annex");
    if !path.exists() {
        log::info!(
            "Repository at {} is not a git-annex repository; initializing ...",
            repo.display()
        );
        runcmd(["git-annex", "init"], &repo).await?;
    }
    Ok(())
}

pub(crate) async fn runcmd<I, S, P>(argv: I, cwd: P) -> Result<(), anyhow::Error>
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
