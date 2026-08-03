//! Encode → CPU decode round trip: WMF H.264 CPU-upload encode into the new
//! [`VideoOutputPreference::CpuFramesOk`] software decode path, with no GPU device anywhere
//! in the chain. Skips (does not fail) when Media Foundation itself is unavailable.

#![cfg(all(windows, feature = "video"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "integration test"
)]

use mediaway_common::{Bytes, CodecKind, PixelFormat, Rational, VideoFrame, VideoFrameStorage};
use mediaway_decoder::{VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_decoder_windows::WindowsVideoDecoder;
use mediaway_encoder::windows::WindowsVideoEncoder;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

#[test]
fn encode_cpu_then_decode_cpu_round_trip() {
    let nv12_len = (WIDTH * HEIGHT + WIDTH * HEIGHT / 2) as usize;
    let enc_cfg = VideoEncoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
    };
    let mut encoder = match WindowsVideoEncoder::open(&enc_cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: WindowsVideoEncoder::open failed ({e:?}) — MF unavailable?");
            return;
        }
    };

    // Mid-gray NV12: luma 128, chroma 128/128 — content doesn't matter, only that it decodes.
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width: WIDTH,
        height: HEIGHT,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from(vec![128u8; nv12_len]),
        },
    };
    encoder.push_frame(&frame).expect("encoder push_frame");
    encoder.flush().expect("encoder flush");

    let mut packets = Vec::new();
    while let Some(p) = encoder.poll_packet().expect("encoder poll_packet") {
        packets.push(p);
    }
    assert!(!packets.is_empty(), "expected at least one encoded packet");
    let extra_data = encoder.stream_info().extra_data().clone(); // clone: owned snapshot for decoder config below

    let dec_cfg = VideoDecoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        pixel_format: PixelFormat::Nv12,
        output: VideoOutputPreference::CpuFramesOk,
        gpu_device: None,
        extra_data,
    };
    let mut decoder = match WindowsVideoDecoder::open(&dec_cfg) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "skip: WindowsVideoDecoder::open (CPU) failed ({e:?}) — no software H.264 MFT?"
            );
            return;
        }
    };

    for packet in &packets {
        decoder.push_packet(packet).expect("decoder push_packet");
    }
    decoder.flush().expect("decoder flush");

    let mut frames_out = Vec::new();
    while let Some(frame) = decoder.poll_frame().expect("decoder poll_frame") {
        frames_out.push(frame);
    }
    assert!(
        !frames_out.is_empty(),
        "expected at least one decoded CPU frame"
    );
    for frame in &frames_out {
        assert_eq!(frame.width, WIDTH);
        assert_eq!(frame.height, HEIGHT);
        assert!(
            matches!(&frame.storage, VideoFrameStorage::Cpu { data } if data.len() >= nv12_len),
            "expected VideoFrameStorage::Cpu with >= {nv12_len} bytes"
        );
    }
}
