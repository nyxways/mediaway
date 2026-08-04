//! Tier 7 — optional ffprobe oracle on Mediaway-muxed `WebM`.
//!
//! See `adr/0003-webm-mux.md` § Negative/Trade-offs: mux output was
//! previously verified only by round-tripping through this crate's own
//! `Demuxer`. This adds the same external check `iso-bmff` already has
//! (`tests/conformance_oracle.rs`) — ffprobe reading real Mediaway-muxed
//! bytes, independent of this crate's own demux logic.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "oracle tests may unwrap / skip-log"
)]

use ebml_webm::mux::{Live, Muxer, Open};
use ebml_webm::types::TrackInfo;
use std::io::Write;
use std::process::Command;

fn ffprobe_ok() -> bool {
    Command::new("ffprobe")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn mux_tiny_webm() -> Vec<u8> {
    let mut open = Muxer::<Open>::new();
    open.add_track(TrackInfo {
        track_number: 1,
        track_type: 1,
        codec_id: "V_VP9".to_string(),
        codec_private: None,
        width: 320,
        height: 240,
        sample_rate: 8000.0,
        channels: 1,
    })
    .expect("video track");
    open.add_track(TrackInfo {
        track_number: 2,
        track_type: 2,
        codec_id: "A_OPUS".to_string(),
        codec_private: None,
        width: 0,
        height: 0,
        sample_rate: 48_000.0,
        channels: 1,
    })
    .expect("audio track");
    let mut live: Muxer<Live> = open.begin();
    live.push_frame(1, 0, true, &[0x82, 0x49, 0x83])
        .expect("v0");
    live.push_frame(2, 0, true, &[0xFC, 0xFF, 0xFE])
        .expect("a0");
    live.push_frame(1, 33, false, &[0x41, 0x00]).expect("v1");
    live.flush();
    let mut out = Vec::new();
    assert!(
        live.poll_bytes(&mut out) > 0,
        "mux should produce WebM bytes"
    );
    out
}

#[test]
fn ffprobe_reads_mediaway_webm_without_error() {
    if !ffprobe_ok() {
        eprintln!("skip oracle: ffprobe not on PATH");
        return;
    }
    let bytes = mux_tiny_webm();
    let dir = std::env::temp_dir();
    let path = dir.join("mediaway_ebml_webm_oracle.webm");
    {
        let mut f = std::fs::File::create(&path).expect("tmp");
        f.write_all(&bytes).expect("write");
    }
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,codec_type",
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
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("codec_name=vp9"),
        "expected ffprobe to recognize the VP9 video track: {stdout}"
    );
    assert!(
        stdout.contains("codec_name=opus"),
        "expected ffprobe to recognize the Opus audio track: {stdout}"
    );
}
