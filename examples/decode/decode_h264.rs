//! Video decode in isolation — the point of this example is
//! `platform::AutoDecoder`, not muxing or encoding.
//!
//! Decoders need real bitstream bytes to decode, so `make_sample_bitstream`
//! below encodes a handful of frames purely to produce that input — it is
//! setup, not part of the decode demo. For a full encode→mux→demux→decode
//! pipeline, see `pipeline/trim_and_splice.rs`.
//!
//! Run:
//! ```text
//! cargo run --example decode_h264
//! ```

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::expect_used,
    reason = "example demonstrates the happy path with console output"
)]

use mediaway_common::{
    Bytes, CodecKind, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
};
use mediaway_decoder::{VideoDecoderConfig, VideoOutputPreference};
use mediaway_encoder::VideoEncoder;
use mediaway_encoder::auto::AutoVideoEncodeConfig;
use mediaway_pipeline::platform;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
const FPS: u32 = 30;
const SAMPLE_FRAME_COUNT: u32 = 30;

fn main() {
    let Some((packets, extra_data)) = make_sample_bitstream() else {
        eprintln!(
            "decode_h264: encoder unavailable — cannot produce sample input on this platform"
        );
        return;
    };

    let dec_cfg = VideoDecoderConfig {
        extra_data,
        output: VideoOutputPreference::CpuFramesOk,
        ..VideoDecoderConfig::h264(WIDTH, HEIGHT, Rational::new(1, FPS))
    };
    let mut decoder = match platform::AutoDecoder::open(&dec_cfg) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("decode_h264: open failed ({e}) — platform not supported yet");
            return;
        }
    };

    let mut frame_count = 0u32;
    for packet in &packets {
        decoder.push_packet(packet).expect("push packet");
        while let Some(_frame) = decoder.poll_frame().expect("poll frame") {
            frame_count += 1;
        }
    }
    decoder.flush().expect("flush");
    while let Some(_frame) = decoder.poll_frame().expect("poll frame") {
        frame_count += 1;
    }

    println!(
        "decode_h264: {} packets in → {frame_count} decoded frames out",
        packets.len()
    );
}

/// Setup only: encode a few synthetic frames so `main` has real H.264 packets
/// to feed the decoder. `None` when no encoder backend is available here.
fn make_sample_bitstream() -> Option<(Vec<mediaway_common::Packet>, Bytes)> {
    let config = AutoVideoEncodeConfig {
        bitrate_bps: 1_000_000,
        ..AutoVideoEncodeConfig::new(CodecKind::H264, WIDTH, HEIGHT, Rational::new(1, FPS))
    };
    let mut encoder = platform::AutoEncoder::open(&config).ok()?;

    let nv12_len = (WIDTH * HEIGHT + WIDTH * HEIGHT / 2) as usize;
    let source = Bytes::from(vec![128u8; nv12_len]);
    let mut packets = Vec::new();

    for pts in 0..i64::from(SAMPLE_FRAME_COUNT) {
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
        while let Some(p) = encoder.poll_packet().expect("poll packet") {
            packets.push(p);
        }
    }
    encoder.flush().expect("flush");
    while let Some(p) = encoder.poll_packet().expect("poll packet") {
        packets.push(p);
    }

    let StreamInfo::Video { extra_data, .. } = encoder.stream_info().clone() else {
        unreachable!("video encoder always reports StreamInfo::Video")
    };
    Some((packets, extra_data))
}
