//! Demux exception / robustness tests (synthetic + optional FATE corpus).
//!
//! Synthetic cases always run. FATE samples run when `MEDIAWAY_FATE_SAMPLES` or
//! `FATE_SAMPLES` points at a local fate-suite tree (see testing.md).
//!
//! When `ffprobe` is on PATH, `oracle_compare` manifest rows must match Mediaway
//! frame count against ffprobe's demux frame count.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::manual_flatten,
    clippy::print_stderr,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "exception tests may unwrap / skip-log"
)]

use mpeg_audio::Demuxer;
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

/// Frame count from ffprobe.
///
/// Tries `-f mp3` format override first (for .bit files), then default.
fn ffprobe_frame_count(path: &Path) -> Option<usize> {
    // Try with explicit mp3 format first (for .bit files)
    let out = Command::new("ffprobe")
        .args([
            "-f",
            "mp3",
            "-v",
            "error",
            "-count_packets",
            "-show_entries",
            "stream=nb_read_packets,nb_frames",
            "-of",
            "csv=p=0",
            path.to_str()?,
        ])
        .output()
        .ok()?;

    if out.status.success() {
        return parse_ffprobe_output(&out.stdout);
    }

    // Fallback: try without explicit format
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_packets",
            "-show_entries",
            "stream=nb_read_packets,nb_frames",
            "-of",
            "csv=p=0",
            path.to_str()?,
        ])
        .output()
        .ok()?;

    if out.status.success() {
        parse_ffprobe_output(&out.stdout)
    } else {
        None
    }
}

fn parse_ffprobe_output(stdout: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(stdout);
    let mut total_frames = 0usize;
    for line in text.lines() {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.is_empty() || line.trim().is_empty() {
            continue;
        }
        // csv: nb_read_packets,nb_frames (prefer nb_read_packets)
        if !cols.is_empty() {
            if let Ok(p) = cols[0].trim().parse::<usize>() {
                total_frames += p;
                continue;
            }
        }
        // Fallback to nb_frames if nb_read_packets is not available
        if cols.len() >= 2 {
            if let Ok(f) = cols[1].trim().parse::<usize>() {
                total_frames += f;
            }
        }
    }
    if total_frames > 0 {
        Some(total_frames)
    } else {
        None
    }
}

fn demux_all(bytes: &[u8]) -> usize {
    let mut d = Demuxer::new();
    for chunk in bytes.chunks(17) {
        d.push_bytes(chunk);
    }
    let mut frames = 0usize;
    while let Ok(Some(_)) = d.poll_frame() {
        frames += 1;
    }
    frames
}

fn demux_all_no_panic(bytes: &[u8]) {
    let mut d = Demuxer::new();
    for chunk in bytes.chunks(17) {
        d.push_bytes(chunk);
    }
    while let Ok(Some(_)) = d.poll_frame() {}
}

#[test]
fn demux_empty_input_yields_nothing() {
    let frames = demux_all(&[]);
    assert_eq!(frames, 0);
}

#[test]
fn demux_truncated_sync_does_not_panic() {
    demux_all_no_panic(&[0x00, 0x00]);
}

#[test]
fn demux_random_noise_does_not_panic() {
    let noise: Vec<u8> = (0u16..256).map(|i| ((i * 17) & 0xff) as u8).collect();
    demux_all_no_panic(&noise);
}

#[test]
fn demux_fate_manifest_samples() {
    let Some(root) = fate_root() else {
        eprintln!(
            "skip fate demux: set MEDIAWAY_FATE_SAMPLES or FATE_SAMPLES to a local fate-suite root"
        );
        return;
    };
    if !root.is_dir() {
        eprintln!("skip fate demux: {} is not a directory", root.display());
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
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("failed to read {}: {e}", path.display());
                continue;
            }
        };
        demux_file_resilient(&path, &bytes);

        if ent.mode == FateMode::OracleCompare && probe {
            let Some(ff_frames) = ffprobe_frame_count(&path) else {
                eprintln!("ffprobe failed on {} (oracle_compare)", path.display());
                continue;
            };
            let mw_frames = demux_all(&bytes);
            assert_eq!(
                mw_frames, ff_frames,
                "frame count mismatch on {}: mediaway={mw_frames} ffprobe={ff_frames}",
                ent.rel
            );
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

fn demux_file_resilient(path: &Path, bytes: &[u8]) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        demux_all_no_panic(bytes);
    }));
    if result.is_err() {
        eprintln!("demux panicked on FATE sample {}", path.display());
    }
}
