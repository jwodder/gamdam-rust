#![allow(dead_code)]
use anyhow::Context;
use clap::Parser;
use futures::stream::TryStreamExt;
use gamdam_rust::Downloadable;
use std::path::{Path, PathBuf};
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
#[derive(Debug, Parser)]
#[clap(version)]
struct Arguments {
    /// Additional options to pass to `git-annex addurl`
    #[clap(long, value_parser = shell_words::split, value_name = "OPTIONS")]
    addurl_opts: Vec<String>,

    /// git-annex repository to operate in  [default: current directory]
    #[clap(short = 'C', long = "chdir", value_name = "DIR", default_value_os_t = PathBuf::from("."), hide_default_value = true)]
    repo: PathBuf,

    /// Write failed download items to the given file
    #[clap(short = 'F', long = "failures", value_name = "FILE")]
    failures: Option<PathBuf>,

    /// Number of jobs for `git-annex addurl` to use  [default: one per CPU]
    #[clap(short = 'J', value_name = "INT")]
    jobs: Option<usize>,

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
    #[clap(long, overrides_with = "_no_save")]
    save: bool,

    /// Don't commit the downloaded files when done
    #[clap(long = "no-save")]
    _no_save: bool,

    /// File containing JSON lines with "url", "path", "metadata" (optional),
    /// and "extra_urls" (optional) fields  [default: read from stdin]
    #[clap(default_value_os_t = PathBuf::from("-"), hide_default_value = true)]
    infile: PathBuf,
}

fn main() {
    let args = Arguments::parse();
    println!("{args:?}");
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
