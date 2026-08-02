//! Media probe CLI entrypoint.
//!
//! Maps a subset of common `ffprobe`-style arguments onto Mediaway demux/metadata
//! paths. Not affiliated with the `FFmpeg` project. Not part of the library API.

#![forbid(unsafe_code)]
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI writes reports to stdout, errors to stderr"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "internal modules stay pub(crate) on purpose: this is a bin crate with no external API, \
              and plain `pub` would pull every field/variant under the missing_docs lint for no benefit"
)]

mod args;
mod error;
mod probe;
mod report;

use args::{OutputFormat, parse_args};
use error::ProbeError;
use std::{env, fs};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if let Err(err) = run(&args) {
        eprintln!("mediaway-avprobe: {err}");
        std::process::exit(err.exit_code());
    }
}

fn run(argv: &[String]) -> Result<(), ProbeError> {
    let parsed = parse_args(argv)?;

    let bytes = fs::read(&parsed.input).map_err(|source| ProbeError::Read {
        path: parsed.input.display().to_string(),
        source,
    })?;

    let probe_report = probe::build_report(&parsed.input, &bytes)?;

    let rendered = match parsed.output_format {
        OutputFormat::Default => {
            report::render_text(&probe_report, parsed.show_format, parsed.show_streams)
        }
        OutputFormat::Json => {
            report::render_json(&probe_report, parsed.show_format, parsed.show_streams)
        }
    };
    print!("{rendered}");
    Ok(())
}
