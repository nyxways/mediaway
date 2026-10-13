//! Tests for [`super::WindowsVideoEncoder::open_constant_quality`].
#![allow(
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may expect and report hardware-gated skips"
)]

use super::WindowsVideoEncoder;
use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{Bytes, CodecKind, PixelFormat, Rational, VideoFrame, VideoFrameStorage};

/// The bytes the CPU encoder makes of 30 frames of a moving gradient with noise on it, at
/// `quality` (constant-quality mode) or at the bitrate target (`None`); `None` if the
/// encoder cannot be opened here.
fn encoded_size(quality: Option<u8>) -> Option<usize> {
    let (w, h) = (320u32, 240u32);
    let cfg = VideoEncoderConfig {
        codec: CodecKind::H264,
        width: w,
        height: h,
        time_base: Rational::new(1, 30),
        bitrate_bps: 1_000_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 30,
        rate_control: None,
        intra_refresh_period: None,
    };
    let opened = quality.map_or_else(
        || WindowsVideoEncoder::open(&cfg),
        |q| WindowsVideoEncoder::open_constant_quality(&cfg, q),
    );
    let mut enc = opened.ok()?;
    let mut seed = 12_345u32;
    let mut total = 0usize;
    for n in 0..30u32 {
        let mut data = Vec::with_capacity((w * h * 3 / 2) as usize);
        for y in 0..h {
            for x in 0..w {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 28).to_le_bytes()[0];
                // Only the low byte is wanted: the gradient wraps.
                let gradient = (x + n * 3).to_le_bytes()[0].wrapping_add(y.to_le_bytes()[0] / 2);
                data.push(gradient.wrapping_add(noise));
            }
        }
        data.resize((w * h * 3 / 2) as usize, 128);
        let frame = VideoFrame {
            pts: i64::from(n),
            duration: 1,
            width: w,
            height: h,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Cpu {
                data: Bytes::from(data),
            },
        };
        enc.push_frame(&frame).expect("push");
        while let Some(p) = enc.poll_packet().expect("poll") {
            total += p.payload.len();
        }
    }
    enc.flush().expect("flush");
    while let Some(p) = enc.poll_packet().expect("poll") {
        total += p.payload.len();
    }
    Some(total)
}

#[test]
fn constant_quality_follows_the_quality_not_the_bitrate_target() {
    let (Some(low), Some(high)) = (encoded_size(Some(20)), encoded_size(Some(95))) else {
        eprintln!("skip: WindowsVideoEncoder::open failed - MF unavailable?");
        return;
    };
    // The target is 1 Mbps (about 125 kB for the second); the quality decides, not it.
    assert!(
        high > low * 2,
        "quality 95 made {high} bytes, quality 20 made {low}"
    );
}

#[test]
fn constant_quality_is_not_offered_for_zero_copy_input() {
    let cfg = VideoEncoderConfig {
        input: VideoInputPreference::ZeroCopyGpu,
        ..VideoEncoderConfig::h264(64, 64, Rational::new(1, 30))
    };
    assert!(matches!(
        WindowsVideoEncoder::open_constant_quality(&cfg, 90),
        Err(EncodeError::Unsupported)
    ));
}
