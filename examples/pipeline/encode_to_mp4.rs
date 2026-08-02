//! High-level encode pipeline: frames → H.264 → fragmented MP4.
//!
//! No `#[cfg(…)]` here — platform selection (`mediaway_pipeline::platform`) and the
//! encoder→muxer wiring (`mediaway_pipeline::EncodeSession`) are both handled by
//! `mediaway-pipeline`; this file is just the per-frame loop.
//!
//! For the low-level path (manual poll loops, no convenience wrapper), see
//! `mux_roundtrip.rs`.
//!
//! Run:
//! ```text
//! cargo run --example encode_to_mp4
//! ```
//! Output: `out.mp4` in the current directory.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::expect_used,
    reason = "example demonstrates the happy path with console output"
)]

use mediaway_common::{Bytes, CodecKind, PixelFormat, Rational, VideoFrame, VideoFrameStorage};
use mediaway_encoder::auto::AutoVideoEncodeConfig;
use mediaway_pipeline::{EncodeSession, platform};
use std::fs::File;
use std::io::Write as _;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const FPS: u32 = 30;
const SECONDS: u32 = 3;

fn main() {
    let config = AutoVideoEncodeConfig {
        bitrate_bps: 2_000_000,
        ..AutoVideoEncodeConfig::new(CodecKind::H264, WIDTH, HEIGHT, Rational::new(1, FPS))
    };

    let encoder = match platform::AutoEncoder::open(&config) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("encode_to_mp4: open failed ({e}) — platform not supported yet");
            return;
        }
    };

    println!("encode_to_mp4: running on this platform");
    let mut session = EncodeSession::open(encoder).expect("open encode session");

    // ── Synthetic NV12 source (replace with real frames in your app) ─────────
    let nv12_len = (WIDTH * HEIGHT + WIDTH * HEIGHT / 2) as usize;
    let source = Bytes::from(vec![128u8; nv12_len]); // grey Y=128, UV=128

    for pts in 0..i64::from(FPS * SECONDS) {
        let frame = VideoFrame {
            pts,
            duration: 1,
            width: WIDTH,
            height: HEIGHT,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Cpu {
                // clone: Bytes ref-count bump — backing buffer is not copied
                data: source.clone(),
            },
        };
        session.write_frame(&frame).expect("write frame");
    }

    let mp4_bytes = session.finish().expect("finish encode session");

    File::create("out.mp4")
        .and_then(|mut f| f.write_all(&mp4_bytes))
        .expect("write out.mp4");

    println!(
        "encode_to_mp4: {} frames → out.mp4 ({} bytes)",
        FPS * SECONDS,
        mp4_bytes.len()
    );
}
