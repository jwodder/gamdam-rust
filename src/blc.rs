//! tokio-util's `LinesCodec` is incompatible with tokio-serde + JSON in the
//! following ways:
//!
//! - tokio-serde requires provided Streams to produce BytesMut, but
//!   LinesCodec's Decoder impl produces Strings.
//!
//! - tokio-serde requires provided Sinks to take Bytes, but LinesCodec's
//!   Encoder impl takes AsRef<str>.
//!
//! - The error type used by the serde codec (here, json_serde::Error) has to
//!   be convertible to the error type of the encoder & decoder (here,
//!   LinesCodecError), yet it is not.
//!
//! Hence, I've copied the source of [`lines_codec.rs`][1] and adjusted it as
//! necessary to resolve the above problems.  -- jwodder
//!
//! [1]: https://github.com/tokio-rs/tokio/blob/a03e0420249d1740668f608a5a16f1fa614be2c7/tokio-util/src/codec/lines_codec.rs

// Copyright (c) 2022 Tokio Contributors
//
// Permission is hereby granted, free of charge, to any
// person obtaining a copy of this software and associated
// documentation files (the "Software"), to deal in the
// Software without restriction, including without
// limitation the rights to use, copy, modify, merge,
// publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software
// is furnished to do so, subject to the following
// conditions:
//
// The above copyright notice and this permission notice
// shall be included in all copies or substantial portions
// of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
// ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
// TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
// PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
// SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
// IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use tokio_util::codec::{Decoder, Encoder};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::{cmp, fmt, io, usize};

/// A simple [`Decoder`] and [`Encoder`] implementation that splits up data into lines.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BinaryLinesCodec {
    // Stored index of the next index to examine for a `\n` character.
    // This is used to optimize searching.
    // For example, if `decode` was called with `abc`, it would hold `3`,
    // because that is the next index to examine.
    // The next time `decode` is called with `abcde\n`, the method will
    // only look at `de\n` before returning.
    next_index: usize,

    /// The maximum length for a given line. If `usize::MAX`, lines will be
    /// read until a `\n` character is reached.
    max_length: usize,

    /// Are we currently discarding the remainder of a line which was over
    /// the length limit?
    is_discarding: bool,
}

impl BinaryLinesCodec {
    /// Returns a `BinaryLinesCodec` for splitting up data into lines.
    ///
    /// # Note
    ///
    /// The returned `BinaryLinesCodec` will not have an upper bound on the length
    /// of a buffered line. See the documentation for [`new_with_max_length`]
    /// for information on why this could be a potential security risk.
    pub fn new() -> BinaryLinesCodec {
        BinaryLinesCodec {
            next_index: 0,
            max_length: usize::MAX,
            is_discarding: false,
        }
    }

    /// Returns a `BinaryLinesCodec` with a maximum line length limit.
    ///
    /// If this is set, calls to `BinaryLinesCodec::decode` will return a
    /// [`BinaryLinesCodecError`] when a line exceeds the length limit. Subsequent calls
    /// will discard up to `limit` bytes from that line until a newline
    /// character is reached, returning `None` until the line over the limit
    /// has been fully discarded. After that point, calls to `decode` will
    /// function as normal.
    ///
    /// # Note
    ///
    /// Setting a length limit is highly recommended for any `BinaryLinesCodec` which
    /// will be exposed to untrusted input. Otherwise, the size of the buffer
    /// that holds the line currently being read is unbounded. An attacker could
    /// exploit this unbounded buffer by sending an unbounded amount of input
    /// without any `\n` characters, causing unbounded memory consumption.
    pub fn new_with_max_length(max_length: usize) -> Self {
        BinaryLinesCodec {
            max_length,
            ..BinaryLinesCodec::new()
        }
    }
}

fn without_carriage_return(s: &[u8]) -> &[u8] {
    if let Some(&b'\r') = s.last() {
        &s[..s.len() - 1]
    } else {
        s
    }
}

impl Decoder for BinaryLinesCodec {
    type Item = BytesMut;
    type Error = BinaryLinesCodecError;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<BytesMut>, BinaryLinesCodecError> {
        loop {
            // Determine how far into the buffer we'll search for a newline. If
            // there's no max_length set, we'll read to the end of the buffer.
            let read_to = cmp::min(self.max_length.saturating_add(1), buf.len());

            let newline_offset = buf[self.next_index..read_to]
                .iter()
                .position(|b| *b == b'\n');

            match (self.is_discarding, newline_offset) {
                (true, Some(offset)) => {
                    // If we found a newline, discard up to that offset and
                    // then stop discarding. On the next iteration, we'll try
                    // to read a line normally.
                    buf.advance(offset + self.next_index + 1);
                    self.is_discarding = false;
                    self.next_index = 0;
                }
                (true, None) => {
                    // Otherwise, we didn't find a newline, so we'll discard
                    // everything we read. On the next iteration, we'll continue
                    // discarding up to max_len bytes unless we find a newline.
                    buf.advance(read_to);
                    self.next_index = 0;
                    if buf.is_empty() {
                        return Ok(None);
                    }
                }
                (false, Some(offset)) => {
                    // Found a line!
                    let newline_index = offset + self.next_index;
                    self.next_index = 0;
                    let line = buf.split_to(newline_index + 1);
                    let line = &line[..line.len() - 1];
                    let line = without_carriage_return(line);
                    return Ok(Some(BytesMut::from(line)));
                }
                (false, None) if buf.len() > self.max_length => {
                    // Reached the maximum length without finding a
                    // newline, return an error and start discarding on the
                    // next call.
                    self.is_discarding = true;
                    return Err(BinaryLinesCodecError::MaxLineLengthExceeded);
                }
                (false, None) => {
                    // We didn't find a line or reach the length limit, so the next
                    // call will resume searching at the current offset.
                    self.next_index = read_to;
                    return Ok(None);
                }
            }
        }
    }

    fn decode_eof(
        &mut self,
        buf: &mut BytesMut,
    ) -> Result<Option<BytesMut>, BinaryLinesCodecError> {
        Ok(match self.decode(buf)? {
            Some(frame) => Some(frame),
            None => {
                // No terminating newline - return remaining data, if any
                if buf.is_empty() || buf == &b"\r"[..] {
                    None
                } else {
                    let line = buf.split_to(buf.len());
                    let line = without_carriage_return(&line);
                    self.next_index = 0;
                    Some(BytesMut::from(line))
                }
            }
        })
    }
}

impl Encoder<Bytes> for BinaryLinesCodec {
    type Error = BinaryLinesCodecError;

    fn encode(&mut self, line: Bytes, buf: &mut BytesMut) -> Result<(), BinaryLinesCodecError> {
        buf.reserve(line.len() + 1);
        buf.put(line);
        buf.put_u8(b'\n');
        Ok(())
    }
}

impl Default for BinaryLinesCodec {
    fn default() -> Self {
        Self::new()
    }
}

/// An error occurred while encoding or decoding a line.
#[derive(Debug)]
pub enum BinaryLinesCodecError {
    /// The maximum line length was exceeded.
    MaxLineLengthExceeded,
    /// An IO error occurred.
    Io(io::Error),
}

impl fmt::Display for BinaryLinesCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinaryLinesCodecError::MaxLineLengthExceeded => write!(f, "max line length exceeded"),
            BinaryLinesCodecError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for BinaryLinesCodecError {
    fn from(e: io::Error) -> BinaryLinesCodecError {
        BinaryLinesCodecError::Io(e)
    }
}

impl std::error::Error for BinaryLinesCodecError {}

impl From<serde_json::Error> for BinaryLinesCodecError {
    fn from(e: serde_json::Error) -> BinaryLinesCodecError {
        io::Error::from(e).into()
    }
}
