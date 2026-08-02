//! AV CLI compatibility entrypoint.
//!
//! Maps a subset of common `ffmpeg`-style arguments onto Mediaway encode/mux
//! pipelines. Not affiliated with the `FFmpeg` project. Not part of the library API.

#![forbid(unsafe_code)]
#![allow(clippy::print_stderr, reason = "CLI reports errors to stderr")]
#![allow(
    clippy::redundant_pub_crate,
    reason = "internal modules stay pub(crate) on purpose: this is a bin crate with no external API, \
              and plain `pub` would pull every field/variant under the missing_docs lint for no benefit"
)]

mod args;
mod error;
mod pipeline;

use args::{CliMode, InputSource, OutputTarget, parse_args};
use error::CliError;
use std::io::{self, Read, Write};
use std::{env, fs};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if let Err(err) = run(&args) {
        eprintln!("mediaway-avcli: {err}");
        std::process::exit(err.exit_code());
    }
}

fn run(argv: &[String]) -> Result<(), CliError> {
    let parsed = parse_args(argv)?;

    let bytes = match parsed.mode {
        CliMode::Synthetic { count } => pipeline::mux_synthetic(count, parsed.geometry)?,
        CliMode::FromInput { input } => {
            let access_unit = read_input(&input)?;
            pipeline::mux_single_access_unit(access_unit, parsed.geometry)?
        }
    };

    write_output(&parsed.output, &bytes)
}

fn read_input(input: &InputSource) -> Result<Vec<u8>, CliError> {
    match input {
        InputSource::Stdin => {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf)?;
            Ok(buf)
        }
        InputSource::File(path) => Ok(fs::read(path)?),
    }
}

fn write_output(output: &OutputTarget, bytes: &[u8]) -> Result<(), CliError> {
    match output {
        OutputTarget::Stdout => {
            io::stdout().write_all(bytes)?;
            Ok(())
        }
        OutputTarget::File(path) => Ok(fs::write(path, bytes)?),
    }
}
