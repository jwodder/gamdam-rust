use anyhow::Context;
use clap::builder::ArgAction;
use clap::Parser;
use futures::stream::TryStreamExt;
use gamdam::cmd::{CommandError, LoggedCommand};
use gamdam::{ensure_annex_repo, Downloadable, Gamdam, Jobs};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use tokio::fs::File;
use tokio::io::{stdin, AsyncRead};
use tokio_util::codec::{FramedRead, LinesCodec};

/// Git-Annex Mass Downloader and Metadata-er
///
/// `gamdam` reads a series of JSON entries from a file (or from standard input
/// if no file is specified) following the input format described in the README
/// at <https://github.com/jwodder/gamdam-rust>.  It feeds the URLs and output
/// paths to `git-annex addurl`, and once each file has finished downloading,
/// it attaches any listed metadata and extra URLs using `git-annex metadata`
/// and `git-annex registerurl`, respectively.
#[derive(Debug, Parser, PartialEq)]
#[clap(version)]
struct Arguments {
    /// Additional options to pass to `git-annex addurl`
    #[clap(long, value_parser = shell_words::split, value_name = "OPTIONS")]
    addurl_opts: Vec<String>,

    /// git-annex repository to operate in  [default: current directory]
    #[clap(short = 'C', long = "chdir", value_name = "DIR", default_value_os_t = PathBuf::from("."), hide_default_value = true)]
    repo: PathBuf,

    // TODO
    // /// Write failed download items to the given file
    // #[clap(short = 'F', long = "failures", value_name = "FILE")]
    // failures: Option<PathBuf>,
    /// Number of jobs for `git-annex addurl` to use  [default: one per CPU]
    #[clap(short = 'J', value_name = "INT")]
    jobs: Option<NonZeroUsize>,

    /// Set logging level
    #[clap(
        short,
        long,
        default_value = "INFO",
        value_name = "OFF|ERROR|WARN|INFO|DEBUG|TRACE"
    )]
    log_level: log::LevelFilter,

    /// The commit message to use when saving
    ///
    /// Any occurrences of "{downloaded}" in the message will be replaced by
    /// the number of successfully downloaded files.
    #[clap(
        short,
        long,
        default_value = "Downloaded {downloaded} URLs",
        value_name = "TEXT"
    )]
    message: String,

    /// Don't commit if any files failed to download
    #[clap(long)]
    no_save_on_fail: bool,

    /// Commit the downloaded files when done  [default]
    #[clap(long = "save")]
    _no_save: bool,

    /// Don't commit the downloaded files when done
    #[clap(long = "no-save", overrides_with = "_no_save", action = ArgAction::SetFalse)]
    save: bool,

    /// File containing JSON lines with "url", "path", "metadata" (optional),
    /// and "extra_urls" (optional) fields  [default: read from stdin]
    #[clap(default_value_os_t = PathBuf::from("-"), hide_default_value = true)]
    infile: PathBuf,
}

impl Default for Arguments {
    fn default() -> Arguments {
        Arguments {
            addurl_opts: Vec::new(),
            repo: PathBuf::from("."),
            jobs: None,
            log_level: log::LevelFilter::Info,
            message: "Downloaded {downloaded} URLs".into(),
            no_save_on_fail: false,
            save: true,
            _no_save: false,
            infile: PathBuf::from("-"),
        }
    }
}

#[tokio::main]
async fn main() -> Result<ExitCode, anyhow::Error> {
    let args = Arguments::parse();
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{:<5}] {}",
                chrono::Local::now().format("%H:%M:%S"),
                record.level(),
                message
            ))
        })
        .level(args.log_level)
        .chain(std::io::stderr())
        .apply()
        .unwrap();
    let items = read_input_file(args.infile).await?;
    ensure_annex_repo(&args.repo).await?;
    let gamdam = Gamdam {
        repo: args.repo.clone(),
        addurl_options: args.addurl_opts,
        addurl_jobs: args.jobs.map_or(Jobs::CPUs, Jobs::Qty),
    };
    let report = gamdam.download(items).await?;
    if report.downloaded > 0 && args.save && !(args.no_save_on_fail && report.failed > 0) {
        match LoggedCommand::new("git", ["diff", "--cached", "--quiet"], &args.repo)
            .status()
            .await
        {
            Err(CommandError::Exit { .. }) => {
                LoggedCommand::new(
                    "git",
                    [
                        "commit",
                        "-m",
                        &args
                            .message
                            .replace("{downloaded}", &report.downloaded.to_string()),
                    ],
                    args.repo,
                )
                .status()
                .await?
            }
            Ok(()) => {
                // This can happen if we only downloaded files that were
                // already present in the repo.
                log::info!("Nothing to commit");
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(if report.failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn read_input_file<P: AsRef<Path>>(path: P) -> Result<Vec<Downloadable>, anyhow::Error> {
    let path = path.as_ref();
    let fp: Box<dyn AsyncRead + std::marker::Unpin> = if path == Path::new("-") {
        Box::new(stdin())
    } else {
        Box::new(
            File::open(&path)
                .await
                .with_context(|| format!("Error opening {} for reading", path.display()))?,
        )
    };
    let lines = FramedRead::new(fp, LinesCodec::new_with_max_length(65535));
    tokio::pin!(lines);
    let mut items = Vec::new();
    let mut lineno = 1;
    while let Some(ln) = lines.try_next().await.context("Error reading input")? {
        match serde_json::from_str(&ln) {
            Ok(d) => items.push(d),
            Err(e) => log::warn!("Input line {} is invalid; discarding: {}", lineno, e),
        }
        lineno += 1;
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_no_args() {
        let args = Arguments::try_parse_from(["arg0"]).unwrap();
        assert_eq!(args, Arguments::default());
    }

    #[test]
    fn test_cli_save() {
        let args = Arguments::try_parse_from(["arg0", "--save"]).unwrap();
        assert_eq!(
            args,
            Arguments {
                _no_save: true,
                ..Arguments::default()
            }
        );
    }

    #[test]
    fn test_cli_no_save() {
        let args = Arguments::try_parse_from(["arg0", "--no-save"]).unwrap();
        assert_eq!(
            args,
            Arguments {
                save: false,
                ..Arguments::default()
            }
        );
    }
}
