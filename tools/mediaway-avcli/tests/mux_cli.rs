//! End-to-end: run the built `mediaway-avcli` binary and demux its output
//! back through `mediaway-container` to confirm the flag-driven mux pipeline
//! actually produces a valid, readable MP4.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration test may unwrap"
)]

use mediaway_container::mp4::Demuxer;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mediaway-avcli")
}

fn tmp_path(name: &str) -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    dir.push(format!("mediaway-avcli-{name}-{}.mp4", std::process::id()));
    dir
}

fn demux_all(bytes: &[u8]) -> Demuxer {
    let mut demuxer = Demuxer::new();
    demuxer.push_bytes(bytes);
    demuxer
}

#[test]
fn synthetic_mode_writes_a_readable_mp4_with_requested_packet_count() {
    let out = tmp_path("synthetic");
    let status = Command::new(bin())
        .args(["--synthetic", "12", out.to_str().expect("utf8 path")])
        .status()
        .expect("spawn avcli");
    assert!(status.success());

    let bytes = std::fs::read(&out).expect("read output mp4");
    let mut demuxer = demux_all(&bytes);
    assert_eq!(demuxer.streams().len(), 1);
    let mut count = 0;
    while demuxer.poll_packet().is_some() {
        count += 1;
    }
    assert_eq!(count, 12);

    let _ = std::fs::remove_file(&out);
}

#[test]
fn geometry_flag_is_applied_to_the_video_track() {
    let out = tmp_path("geometry");
    let status = Command::new(bin())
        .args([
            "--synthetic",
            "1",
            "-s",
            "640x480",
            out.to_str().expect("utf8 path"),
        ])
        .status()
        .expect("spawn avcli");
    assert!(status.success());

    let bytes = std::fs::read(&out).expect("read output mp4");
    let demuxer = demux_all(&bytes);
    let geometry = demuxer.streams()[0].geometry().expect("video geometry");
    assert_eq!(geometry.width, 640);
    assert_eq!(geometry.height, 480);

    let _ = std::fs::remove_file(&out);
}

#[test]
fn stdin_input_muxes_a_single_keyframe_packet() {
    let out = tmp_path("stdin");
    let mut child = Command::new(bin())
        .args(["-i", "-", out.to_str().expect("utf8 path")])
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn avcli");
    child
        .stdin
        .take()
        .expect("stdin handle")
        .write_all(&[0x00, 0x00, 0x00, 0x01, 0x65, 0xAA, 0xBB])
        .expect("write stdin");
    let status = child.wait().expect("wait avcli");
    assert!(status.success());

    let bytes = std::fs::read(&out).expect("read output mp4");
    let mut demuxer = demux_all(&bytes);
    let packet = demuxer.poll_packet().expect("one packet");
    assert!(packet.is_keyframe);
    assert!(demuxer.poll_packet().is_none());

    let _ = std::fs::remove_file(&out);
}

#[test]
fn missing_output_is_a_usage_error_exit_two() {
    let status = Command::new(bin())
        .args(["--synthetic", "1"])
        .status()
        .expect("spawn avcli");
    assert_eq!(status.code(), Some(2));
}

#[test]
fn combining_input_and_synthetic_is_a_usage_error() {
    let out = tmp_path("conflict");
    let status = Command::new(bin())
        .args([
            "-i",
            "-",
            "--synthetic",
            "1",
            out.to_str().expect("utf8 path"),
        ])
        .status()
        .expect("spawn avcli");
    assert_eq!(status.code(), Some(2));
}

#[test]
fn unknown_flag_is_a_usage_error_exit_two() {
    let status = Command::new(bin())
        .args(["-bogus-flag", "out.mp4"])
        .status()
        .expect("spawn avcli");
    assert_eq!(status.code(), Some(2));
}
