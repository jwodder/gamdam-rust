use anyhow::Context;
use clap::builder::ArgAction;
use clap::Parser;
use futures::sink::SinkExt;
use futures::StreamExt;
use gamdam::cmd::{CommandError, LoggedCommand};
use gamdam::{ensure_annex_repo, DownloadResult, Downloadable, Gamdam, Jobs};
use patharg::{InputArg, OutputArg};
use serde_jsonlines::{AsyncBufReadJsonLines, AsyncWriteJsonLines};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::process::ExitCode;
use tokio::io::BufReader;

/// Git-Annex Mass Downloader and Metadata-er
///
/// `gamdam` reads a series of JSON entries from a file (or from standard input
/// if no file is specified) following the input format described in the README
/// at <https://github.com/jwodder/gamdam-rust>.  It feeds the URLs and output
/// paths to `git-annex addurl`, and once each file has finished downloading,
/// it attaches any listed metadata and extra URLs using `git-annex metadata`
/// and `git-annex registerurl`, respectively.
#[derive(Debug, Parser, PartialEq)]
#[command(version)]
struct Arguments {
    /// Additional options to pass to `git-annex addurl`
    ///
    /// Multiple options & arguments need to be quoted as a single string,
    /// which must also use proper shell quoting internally.
    #[arg(long, value_name = "OPTIONS", value_parser = shell_words::split, allow_hyphen_values = true)]
    // We need to refer to Vec with a fully-qualified name in order for clap to
    // not treat the option as multiuse.
    addurl_opts: Option<std::vec::Vec<String>>,

    /// git-annex repository to operate in  [default: current directory]
    ///
    /// If the given directory does not exist, it is created.  If it is not
    /// already inside a Git or git-annex repository, one is initialized.
    #[arg(short = 'C', long = "chdir", value_name = "DIR", default_value_os_t = PathBuf::from("."), hide_default_value = true)]
    repo: PathBuf,

    /// Write failed download items to the given file
    #[arg(short = 'F', long = "failures", value_name = "FILE")]
    failures: Option<OutputArg>,

    /// Number of jobs for `git-annex addurl` to use  [default: one per CPU]
    #[arg(short = 'J', value_name = "INT")]
    jobs: Option<NonZeroUsize>,

    /// Set logging level
    #[arg(
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
    #[arg(
        short,
        long,
        default_value = "Downloaded {downloaded} URLs",
        value_name = "TEXT"
    )]
    message: String,

    /// Don't commit if any files failed to download
    #[arg(long)]
    no_save_on_fail: bool,

    /// Commit the downloaded files when done  [default]
    #[arg(long = "save")]
    _no_save: bool,

    /// Don't commit the downloaded files when done
    #[arg(long = "no-save", overrides_with = "_no_save", action = ArgAction::SetFalse)]
    save: bool,

    /// File containing JSON lines with "url", "path", "metadata" (optional),
    /// and "extra_urls" (optional) fields  [default: read from stdin]
    #[arg(default_value_t, hide_default_value = true)]
    infile: InputArg,
}

impl Default for Arguments {
    fn default() -> Arguments {
        Arguments {
            addurl_opts: None,
            repo: PathBuf::from("."),
            failures: None,
            jobs: None,
            log_level: log::LevelFilter::Info,
            message: "Downloaded {downloaded} URLs".into(),
            no_save_on_fail: false,
            save: true,
            _no_save: false,
            infile: InputArg::Stdin,
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
        .expect("no other logger should have been previously initialized");
    let items = read_input_file(args.infile).await?;
    if items.is_empty() {
        log::info!("Nothing to download");
        return Ok(ExitCode::SUCCESS);
    }
    ensure_annex_repo(&args.repo).await?;
    let gamdam = Gamdam {
        repo: args.repo.clone(),
        addurl_options: args.addurl_opts.unwrap_or_default(),
        addurl_jobs: args.jobs.map_or(Jobs::CPUs, Jobs::Qty),
    };
    let report = gamdam.download(items).await?;
    if !report.successful.is_empty()
        && args.save
        && (!args.no_save_on_fail || report.failed.is_empty())
    {
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
                            .replace("{downloaded}", &report.successful.len().to_string()),
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
    if report.failed.is_empty() {
        Ok(ExitCode::SUCCESS)
    } else {
        if let Some(path) = args.failures {
            if let Err(e) = write_failures(path, report.failed).await {
                log::error!("Error writing failures report: {e}");
            }
        }
        Ok(ExitCode::FAILURE)
    }
}

async fn read_input_file(infile: InputArg) -> Result<Vec<Downloadable>, anyhow::Error> {
    let mut lines = BufReader::new(
        infile
            .async_open()
            .await
            .with_context(|| format!("Error opening {infile} for reading"))?,
    )
    .json_lines::<Downloadable>();
    let mut items = Vec::new();
    let mut lineno = 1;
    while let Some(r) = lines.next().await {
        match r {
            Ok(d) => items.push(d),
            Err(e)
                if e.get_ref()
                    .map(|inner| inner.is::<serde_json::Error>())
                    .unwrap_or(false) =>
            {
                log::warn!("Input line {} is invalid; discarding: {}", lineno, e);
            }
            Err(e) => return Err(e).context("Error reading input"),
        }
        lineno += 1;
    }
    Ok(items)
}

async fn write_failures<I>(outfile: OutputArg, failures: I) -> Result<(), anyhow::Error>
where
    I: IntoIterator<Item = DownloadResult>,
{
    let mut sink = outfile
        .async_create()
        .await
        .with_context(|| format!("Error opening {outfile} for writing"))?
        .into_json_lines_sink();
    for item in failures {
        sink.send(item.downloadable)
            .await
            .context("Error writing to file")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli() {
        Arguments::command().debug_assert()
    }

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

    #[test]
    fn test_cli_hyphen_infile() {
        let args = Arguments::try_parse_from(["arg0", "-"]).unwrap();
        assert_eq!(
            args,
            Arguments {
                infile: InputArg::Stdin,
                ..Arguments::default()
            }
        );
    }

    #[test]
    fn test_cli_addurl_opts() {
        let args = Arguments::try_parse_from([
            "arg0",
            "--addurl-opts",
            "--user-agent 'gamdam via git-annex'",
        ])
        .unwrap();
        assert_eq!(
            args,
            Arguments {
                addurl_opts: Some(vec!["--user-agent".into(), "gamdam via git-annex".into()]),
                ..Arguments::default()
            }
        );
    }

    #[test]
    fn test_cli_addurl_opts_infile() {
        let args = Arguments::try_parse_from([
            "arg0",
            "--addurl-opts",
            "--user-agent 'gamdam via git-annex'",
            "file.json",
        ])
        .unwrap();
        assert_eq!(
            args,
            Arguments {
                addurl_opts: Some(vec!["--user-agent".into(), "gamdam via git-annex".into()]),
                infile: InputArg::Path("file.json".into()),
                ..Arguments::default()
            }
        );
    }

    #[test]
    fn test_cli_jobs() {
        let args = Arguments::try_parse_from(["arg0", "-J", "42"]).unwrap();
        assert_eq!(
            args,
            Arguments {
                jobs: NonZeroUsize::new(42),
                ..Arguments::default()
            }
        );
    }

    #[test]
    fn test_cli_zero_jobs() {
        let args = Arguments::try_parse_from(["arg0", "-J", "0"]);
        assert!(args.is_err());
    }
}
