//! Video encode in isolation — no muxing, no capture, no OS file output.
//!
//! Pushes synthetic NV12 frames straight into the best available H.264
//! encoder on this platform (`mediaway::platform::AutoEncoder`) and
//! reports the compressed packets it produces. For turning that packet
//! stream into a playable file, see `pipeline/encode_to_mp4.rs`.
//!
//! Run:
//! ```text
//! cargo run --example encode_h264
//! ```

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::expect_used,
    reason = "example demonstrates the happy path with console output"
)]

use mediaway::platform;
use mediaway_common::{Bytes, CodecKind, PixelFormat, Rational, VideoFrame, VideoFrameStorage};
use mediaway_encoder::VideoEncoder;
use mediaway_encoder::auto::AutoVideoEncodeConfig;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
const FPS: u32 = 30;
const FRAME_COUNT: u32 = 60;

fn main() {
    let config = AutoVideoEncodeConfig {
        bitrate_bps: 1_000_000,
        ..AutoVideoEncodeConfig::new(CodecKind::H264, WIDTH, HEIGHT, Rational::new(1, FPS))
    };

    let mut encoder = match platform::AutoEncoder::open(&config) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("encode_h264: open failed ({e}) — platform not supported yet");
            return;
        }
    };

    // Synthetic NV12 source — a real app would push captured/decoded frames here.
    let nv12_len = (WIDTH * HEIGHT + WIDTH * HEIGHT / 2) as usize;
    let source = Bytes::from(vec![128u8; nv12_len]); // grey Y=128, UV=128

    let mut packet_count = 0u32;
    let mut byte_count = 0usize;

    for pts in 0..i64::from(FRAME_COUNT) {
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
        encoder.push_frame(&frame).expect("push frame");
        while let Some(packet) = encoder.poll_packet().expect("poll packet") {
            packet_count += 1;
            byte_count += packet.payload.len();
        }
    }

    encoder.flush().expect("flush");
    while let Some(packet) = encoder.poll_packet().expect("poll packet") {
        packet_count += 1;
        byte_count += packet.payload.len();
    }

    println!(
        "encode_h264: {FRAME_COUNT} frames in → {packet_count} packets out ({byte_count} bytes)"
    );
}
