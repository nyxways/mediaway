//! Tier 7 — optional ffprobe oracle on Mediaway-muxed fMP4.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "oracle tests may unwrap / skip-log"
)]

mod common;

use common::mux_tiny_h264_fmp4;
use std::io::Write;
use std::process::Command;

fn ffprobe_ok() -> bool {
    Command::new("ffprobe")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn ffprobe_reads_mediaway_fmp4_without_error() {
    if !ffprobe_ok() {
        eprintln!("skip oracle: ffprobe not on PATH");
        return;
    }
    let bytes = mux_tiny_h264_fmp4();
    let dir = std::env::temp_dir();
    let path = dir.join("mediaway_conformance_oracle.mp4");
    {
        let mut f = std::fs::File::create(&path).expect("tmp");
        f.write_all(&bytes).expect("write");
    }
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=format_name",
            "-of",
            "default=nw=1",
            path.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("ffprobe");
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "ffprobe failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
