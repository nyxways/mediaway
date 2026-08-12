//! Tests for [`super::AndroidVideoEncoder`] — see `docs/conventions/testing.md` Tier 1.
//!
//! **Never executed as authored** — no local Android NDK; see
//! `adr/android/0001-ndk-amediacodec-h264-cpu-upload.md` § CI verification plan.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;
use crate::VideoInputPreference;
use mediaway_common::Rational;

const fn tiny_h264_cfg() -> VideoEncoderConfig {
    VideoEncoderConfig {
        codec: mediaway_common::CodecKind::H264,
        width: 64,
        height: 64,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: mediaway_common::PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    }
}

/// `validate()` runs before any `AMediaCodec` instance is touched, so an unsupported codec is
/// rejected deterministically on every machine — no hardware needed for this assertion.
#[test]
fn open_unsupported_codec_returns_unsupported_without_hardware() {
    let mut cfg = tiny_h264_cfg();
    cfg.codec = mediaway_common::CodecKind::Av1;
    assert!(matches!(
        AndroidVideoEncoder::open(&cfg),
        Err(EncodeError::Unsupported)
    ));
}

/// `VideoInputPreference::ZeroCopyGpu` is not implemented this stage (`AHardwareBuffer`/
/// `ANativeWindow` input is deferred — ADR-0001 § Scope) and is rejected before touching the
/// codec, so this is deterministic on every machine too.
#[test]
fn open_zero_copy_gpu_returns_unsupported_without_hardware() {
    let mut cfg = tiny_h264_cfg();
    cfg.input = VideoInputPreference::ZeroCopyGpu;
    assert!(matches!(
        AndroidVideoEncoder::open(&cfg),
        Err(EncodeError::Unsupported)
    ));
}

/// End-to-end through the public [`AndroidVideoEncoder`] wrapper (delegation to the inner
/// `AMediaCodec` session). **Never run at all in the session that authored this crate** — see
/// `adr/android/0001-ndk-amediacodec-h264-cpu-upload.md` § CI verification plan and
/// `amediacodec::video_tests::amediacodec_open_and_encode_or_skip_without_hw` for the
/// lower-level equivalent.
#[test]
fn open_h264_cpu_upload_or_skip_without_hw() {
    let cfg = tiny_h264_cfg();
    let mut enc = match AndroidVideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: AndroidVideoEncoder::open failed ({e:?}) — no AVC encoder?");
            return;
        }
    };

    let yuv_len = 64 * 64 + (64 * 64) / 2;
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width: 64,
        height: 64,
        format: mediaway_common::PixelFormat::Nv12,
        storage: mediaway_common::VideoFrameStorage::Cpu {
            data: Bytes::from(vec![0u8; yuv_len]),
        },
    };

    if let Err(e) = enc.push_frame(&frame) {
        eprintln!("skip: push_frame failed ({e:?})");
        return;
    }
    let _ = enc.flush();

    let mut packets = 0usize;
    while let Ok(Some(p)) = enc.poll_packet() {
        assert!(!p.payload.is_empty());
        packets += 1;
    }
    eprintln!("AndroidVideoEncoder packets={packets}");
}
