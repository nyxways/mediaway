//! Command-line argument parsing — a small, explicit ffmpeg-compatible subset.
//!
//! Supported: `-i <input>` (`-` = stdin), `-s WxH`, `-y` (accepted as a no-op —
//! Mediaway never prompts before overwriting an output file), `--synthetic <n>`
//! (Mediaway-native self-test mode, **not** an ffmpeg flag), and a positional
//! output path (`-` = stdout). Anything else is a usage error. See
//! `adr/0001-avcli-flag-subset.md` for the mapping onto the mux pipeline.

use crate::error::CliError;
use std::path::PathBuf;

/// Where encoded input bytes come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InputSource {
    /// Read the whole file into memory.
    File(PathBuf),
    /// Read the whole of stdin into memory.
    Stdin,
}

/// Where muxed container bytes go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OutputTarget {
    /// Write to a file path.
    File(PathBuf),
    /// Write to stdout.
    Stdout,
}

/// Video geometry override (`-s WxH`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Geometry {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
        }
    }
}

/// What to mux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CliMode {
    /// Mux one access unit read from `input` as a single keyframe packet
    /// (generalizes the prior `--stdin` scaffold to files).
    FromInput {
        /// Encoded H.264 Annex-B bytes source.
        input: InputSource,
    },
    /// Mediaway-native self-test: mux `count` synthetic H.264 packets.
    Synthetic {
        /// Packet count to synthesize.
        count: usize,
    },
}

/// Parsed CLI arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CliArgs {
    pub(crate) mode: CliMode,
    pub(crate) geometry: Geometry,
    pub(crate) output: OutputTarget,
}

/// Parse argv (excluding `argv[0]`) into [`CliArgs`].
pub(crate) fn parse_args(args: &[String]) -> Result<CliArgs, CliError> {
    let mut input: Option<InputSource> = None;
    let mut synthetic_count: Option<usize> = None;
    let mut geometry = Geometry::default();
    let mut output: Option<OutputTarget> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-i" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| CliError::Usage("-i requires a file path (or -)".to_owned()))?;
                input = Some(if value == "-" {
                    InputSource::Stdin
                } else {
                    InputSource::File(PathBuf::from(value))
                });
            }
            "-s" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| {
                    CliError::Usage("-s requires WxH (e.g. 1920x1080)".to_owned())
                })?;
                geometry = parse_geometry(value)?;
            }
            "-y" => {}
            "--synthetic" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| {
                    CliError::Usage("--synthetic requires a packet count".to_owned())
                })?;
                let count: usize = value.parse().map_err(|_err| {
                    CliError::Usage(format!("--synthetic count is not a number: {value}"))
                })?;
                synthetic_count = Some(count);
            }
            flag if flag.len() > 1 && flag.starts_with('-') => {
                return Err(CliError::Usage(format!("unsupported option: {flag}")));
            }
            positional => {
                if output.is_some() {
                    return Err(CliError::Usage(format!(
                        "unexpected extra argument: {positional}"
                    )));
                }
                output = Some(if positional == "-" {
                    OutputTarget::Stdout
                } else {
                    OutputTarget::File(PathBuf::from(positional))
                });
            }
        }
        i += 1;
    }

    let output = output.ok_or_else(|| {
        CliError::Usage("missing output (usage: mediaway-avcli -i <input> <output>)".to_owned())
    })?;

    let mode = match (input, synthetic_count) {
        (Some(_), Some(_)) => {
            return Err(CliError::Usage(
                "-i and --synthetic are mutually exclusive".to_owned(),
            ));
        }
        (Some(input), None) => CliMode::FromInput { input },
        (None, Some(count)) => CliMode::Synthetic { count },
        (None, None) => {
            return Err(CliError::Usage(
                "missing input: pass -i <path> (or -) or --synthetic <n>".to_owned(),
            ));
        }
    };

    Ok(CliArgs {
        mode,
        geometry,
        output,
    })
}

fn parse_geometry(value: &str) -> Result<Geometry, CliError> {
    let (w, h) = value
        .split_once('x')
        .ok_or_else(|| CliError::Usage(format!("invalid -s geometry (want WxH): {value}")))?;
    let width: u32 = w
        .parse()
        .map_err(|_err| CliError::Usage(format!("invalid -s width: {w}")))?;
    let height: u32 = h
        .parse()
        .map_err(|_err| CliError::Usage(format!("invalid -s height: {h}")))?;
    if width == 0 || height == 0 {
        return Err(CliError::Usage(format!(
            "invalid -s geometry (want WxH > 0): {value}"
        )));
    }
    Ok(Geometry { width, height })
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
