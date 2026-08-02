//! Demux exception / robustness tests (synthetic + optional FATE corpus).
//!
//! Synthetic cases always run. FATE samples run when `MEDIAWAY_FATE_SAMPLES` or
//! `FATE_SAMPLES` points at a local fate-suite tree (see testing.md).
//!
//! When `ffprobe` is on PATH, `oracle_compare` manifest rows must match Mediaway
//! `channels` and `sample_rate`.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::manual_flatten,
    clippy::print_stderr,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use riff_wave::parse;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FateMode {
    OracleCompare,
    MustNotPanic,
}

struct FateEntry {
    rel: &'static str,
    mode: FateMode,
}

/// Paths + modes from `fate_manifest.txt`.
fn fate_manifest() -> Vec<FateEntry> {
    include_str!("fate_manifest.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let rel = parts.next()?;
            let mode = match parts.next().unwrap_or("must_not_panic") {
                "oracle_compare" => FateMode::OracleCompare,
                _ => FateMode::MustNotPanic,
            };
            Some(FateEntry { rel, mode })
        })
        .collect()
}

fn fate_root() -> Option<PathBuf> {
    std::env::var_os("MEDIAWAY_FATE_SAMPLES")
        .or_else(|| std::env::var_os("FATE_SAMPLES"))
        .map(PathBuf::from)
}

fn ffprobe_ok() -> bool {
    Command::new("ffprobe")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `(channels, sample_rate)` from ffprobe (first audio stream).
///
/// Runs: `ffprobe -v error -show_entries stream=channels,sample_rate -of csv=p=0 <path>`
/// (Note: ffprobe outputs `sample_rate,channels` regardless of entry order)
fn ffprobe_audio_info(path: &Path) -> Option<(u16, u32)> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=channels,sample_rate",
            "-of",
            "csv=p=0",
            path.to_str()?,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() >= 2 {
            if let (Ok(sr), Ok(ch)) = (cols[0].trim().parse::<u32>(), cols[1].trim().parse::<u16>())
            {
                return Some((ch, sr));
            }
        }
    }
    None
}

#[test]
fn parse_empty_input_returns_error() {
    let result = parse(&[]);
    assert!(result.is_err(), "parse([]) should return Err");
}

#[test]
fn parse_truncated_riff_header_returns_error() {
    // 12 bytes but not a RIFF/WAVE header
    let truncated = b"xxxxxxxxxxxx";
    let result = parse(truncated);
    assert!(
        result.is_err(),
        "parse on truncated buffer should return Err"
    );
}

#[test]
fn parse_random_noise_does_not_panic() {
    let noise: Vec<u8> = (0u16..256).map(|i| ((i * 17) & 0xff) as u8).collect();
    let result = parse(&noise);
    // Ok or Err is fine; just must not panic.
    let _ = result;
}

#[test]
fn parse_fate_manifest_samples() {
    let Some(root) = fate_root() else {
        eprintln!(
            "skip fate parse: set MEDIAWAY_FATE_SAMPLES or FATE_SAMPLES to a local fate-suite root"
        );
        return;
    };
    if !root.is_dir() {
        eprintln!("skip fate parse: {} is not a directory", root.display());
        return;
    }

    let entries = fate_manifest();
    let probe = ffprobe_ok();
    if !probe {
        eprintln!("ffprobe not on PATH — oracle_compare rows check presence + no panic only");
    }

    let mut seen = 0usize;
    for ent in &entries {
        let path = root.join(ent.rel);
        if !path.is_file() {
            eprintln!("fate missing: {}", path.display());
            continue;
        }
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

        // Always check for panic.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parse(&bytes)));
        assert!(
            result.is_ok(),
            "parse panicked on FATE sample {}",
            path.display()
        );

        // If oracle_compare and ffprobe available, compare format info.
        if ent.mode == FateMode::OracleCompare && probe {
            let Some((ff_channels, ff_sample_rate)) = ffprobe_audio_info(&path) else {
                panic!("ffprobe failed on {} (oracle_compare)", path.display());
            };

            if let Ok(Ok((fmt, _payload))) = &result {
                assert_eq!(
                    fmt.channels, ff_channels,
                    "channels mismatch on {}: mediaway={} ffprobe={}",
                    ent.rel, fmt.channels, ff_channels
                );
                assert_eq!(
                    fmt.sample_rate, ff_sample_rate,
                    "sample_rate mismatch on {}: mediaway={} ffprobe={}",
                    ent.rel, fmt.sample_rate, ff_sample_rate
                );
            } else if let Ok(Err(e)) = &result {
                panic!(
                    "parse() failed on {}: {} (expected oracle_compare success)",
                    ent.rel, e
                );
            }
        }

        seen += 1;
    }

    assert!(
        seen > 0,
        "MEDIAWAY_FATE_SAMPLES/FATE_SAMPLES is set to {} but none of the {} manifest paths were found (run: bun tools/scripts/fetch-fate-samples.ts)",
        root.display(),
        entries.len()
    );
    assert_eq!(
        seen,
        entries.len(),
        "expected all {} manifest samples under {}; found {seen} (missing files printed above)",
        entries.len(),
        root.display()
    );
}
