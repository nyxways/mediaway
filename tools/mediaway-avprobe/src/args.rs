//! Command-line argument parsing — an explicit, ffprobe-compatible subset.
//!
//! Supported: positional input path, `-show_format`, `-show_streams`,
//! `-of`/`-print_format default|json`. Anything else (unknown flags, missing
//! values) is a usage error rather than being silently ignored, per the
//! roadmap's "explicit unsupported set".

use crate::error::ProbeError;
use std::path::PathBuf;

/// Selected output rendering (`-of` / `-print_format`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    /// ffprobe `-of default`-style `key=value` sections.
    Default,
    /// ffprobe `-of json`-style JSON object.
    Json,
}

/// Parsed CLI arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbeArgs {
    pub(crate) input: PathBuf,
    pub(crate) show_format: bool,
    pub(crate) show_streams: bool,
    pub(crate) output_format: OutputFormat,
}

/// Parse argv (excluding `argv[0]`) into [`ProbeArgs`].
pub(crate) fn parse_args(args: &[String]) -> Result<ProbeArgs, ProbeError> {
    let mut input: Option<PathBuf> = None;
    let mut show_format = false;
    let mut show_streams = false;
    let mut output_format = OutputFormat::Default;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-show_format" => show_format = true,
            "-show_streams" => show_streams = true,
            "-of" | "-print_format" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| {
                    ProbeError::Usage(format!("{arg} requires a value (default|json)"))
                })?;
                output_format = match value.as_str() {
                    "default" => OutputFormat::Default,
                    "json" => OutputFormat::Json,
                    other => {
                        return Err(ProbeError::Usage(format!(
                            "unsupported output format '{other}' (supported: default, json)"
                        )));
                    }
                };
            }
            "-i" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| ProbeError::Usage("-i requires a file path".to_owned()))?;
                input = Some(PathBuf::from(value));
            }
            flag if flag.starts_with('-') => {
                return Err(ProbeError::Usage(format!("unsupported option: {flag}")));
            }
            positional => {
                if input.is_some() {
                    return Err(ProbeError::Usage(format!(
                        "unexpected extra argument: {positional}"
                    )));
                }
                input = Some(PathBuf::from(positional));
            }
        }
        i += 1;
    }

    let input = input.ok_or_else(|| {
        ProbeError::Usage(
            "missing input file (usage: mediaway-avprobe [options] <file>)".to_owned(),
        )
    })?;

    // Bare `mediaway-avprobe <file>` with no `-show_*` flag: default to showing
    // both sections, matching the prior scaffold's "always print something
    // useful" behavior instead of ffprobe's "print nothing" default.
    if !show_format && !show_streams {
        show_format = true;
        show_streams = true;
    }

    Ok(ProbeArgs {
        input,
        show_format,
        show_streams,
        output_format,
    })
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
