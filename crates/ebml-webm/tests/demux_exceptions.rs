//! Demux exception / robustness tests (synthetic + optional FATE corpus).
//!
//! Synthetic cases always run. FATE samples run when `MEDIAWAY_FATE_SAMPLES` or
//! `FATE_SAMPLES` points at a local fate-suite tree (see testing.md).
//!
//! When `ffprobe` is on PATH, `oracle_compare` manifest rows must match Mediaway
//! stream count and demux frame count (`nb_read_packets`, else `nb_frames`).

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

use ebml_webm::Demuxer;
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

/// `(streams, demux_frames)` from ffprobe.
///
/// Prefers `nb_read_packets` (edit-list–expanded demux count) when present;
/// falls back to `nb_frames`.
fn ffprobe_counts(path: &Path) -> Option<(usize, usize)> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_packets",
            "-show_entries",
            "stream=nb_frames,nb_read_packets",
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
    let mut streams = 0usize;
    let mut frames = 0usize;
    let mut packets = 0usize;
    let mut any_packets = false;
    let mut any_frames = false;
    for line in text.lines() {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.is_empty() || line.trim().is_empty() {
            continue;
        }
        streams += 1;
        // csv: nb_frames,nb_read_packets (either may be empty/N/A)
        if let Some(f) = cols.first().and_then(|s| s.trim().parse::<usize>().ok()) {
            frames += f;
            any_frames = true;
        }
        if cols.len() >= 2
            && let Ok(p) = cols[1].trim().parse::<usize>()
        {
            packets += p;
            any_packets = true;
        }
    }
    if streams == 0 {
        return None;
    }
    let count = if any_packets {
        packets
    } else if any_frames {
        frames
    } else {
        return None;
    };
    Some((streams, count))
}

fn demux_chunked(bytes: &[u8]) -> (usize, usize) {
    let mut d = Demuxer::new();
    for chunk in bytes.chunks(17) {
        d.push_bytes(chunk);
    }
    let streams = d.streams().len();
    let mut frames = 0usize;
    while d.poll_frame().is_some() {
        frames += 1;
    }
    (streams, frames)
}

fn demux_chunked_no_panic(bytes: &[u8]) {
    demux_chunked(bytes);
}

#[test]
fn demux_empty_input_yields_nothing() {
    let (streams, frames) = demux_chunked(&[]);
    assert_eq!(streams, 0);
    assert_eq!(frames, 0);
}

#[test]
fn demux_truncated_ebml_header_does_not_panic() {
    // EBML element ID (0x1A45DFA3) + partial size
    demux_chunked_no_panic(&[0x1A, 0x45, 0xDF, 0xA3, 0x81]);
}

#[test]
fn demux_random_noise_does_not_panic() {
    let noise: Vec<u8> = (0u16..256).map(|i| ((i * 17) & 0xff) as u8).collect();
    demux_chunked_no_panic(&noise);
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
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        demux_file_resilient(&path, &bytes);

        if ent.mode == FateMode::OracleCompare && probe {
            let Some((ff_streams, ff_count)) = ffprobe_counts(&path) else {
                panic!("ffprobe failed on {} (oracle_compare)", path.display());
            };
            let (mw_streams, mw_frames) = demux_chunked(&bytes);
            assert_eq!(
                mw_streams, ff_streams,
                "stream count mismatch on {}: mediaway={mw_streams} ffprobe={ff_streams}",
                ent.rel
            );
            assert_eq!(
                mw_frames, ff_count,
                "frame count mismatch on {}: mediaway={mw_frames} ffprobe={ff_count} (nb_read_packets preferred)",
                ent.rel
            );
        }
        seen += 1;
    }

    assert!(
        seen > 0,
        "MEDIAWAY_FATE_SAMPLES/FATE_SAMPLES is set to {} but none of the {} manifest paths were found (run: bun tools/scripts/fetch-fate-samples.ts --ai-agent)",
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
        demux_chunked(bytes);
    }));
    assert!(
        result.is_ok(),
        "demux panicked on FATE sample {}",
        path.display()
    );
}
