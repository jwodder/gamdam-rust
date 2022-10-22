#![allow(dead_code)]
use anyhow::Context;
use futures::stream::StreamExt;
use gamdam_rust::Downloadable;
use std::path::Path;
use tokio::fs::File;
use tokio::io::{stdin, AsyncRead};
use tokio_util::codec::{FramedRead, LinesCodec};

fn main() {
    println!("Hello, world!");
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
    while let Some(ln) = lines.next().await {
        match serde_json::from_str(&ln.context("Error reading input")?) {
            Ok(d) => items.push(d),
            Err(e) => log::error!("Input line {} is invalid; discarding: {}", lineno, e),
        }
        lineno += 1;
    }
    Ok(items)
}
