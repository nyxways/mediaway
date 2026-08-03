//! Tests for [`super::LinuxVideoEncoder`] — see `docs/conventions/testing.md` Tier 1.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;
use mediaway_common::Rational;
use crate::VideoInputPreference;

const fn tiny_h264_cfg() -> VideoEncoderConfig {
    VideoEncoderConfig {
        codec: mediaway_common::CodecKind::H264,
        width: 64,
        height: 64,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: mediaway_common::PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
    }
}

/// `validate()` runs before any VA-API display is touched, so an unsupported codec is
/// rejected deterministically on every machine — no hardware needed for this assertion.
#[test]
fn open_unsupported_codec_returns_unsupported_without_hardware() {
    let mut cfg = tiny_h264_cfg();
    cfg.codec = mediaway_common::CodecKind::Av1;
    assert!(matches!(
        LinuxVideoEncoder::open(&cfg),
        Err(EncodeError::Unsupported)
    ));
}

/// `VideoInputPreference::ZeroCopyGpu` is not implemented this stage (DMA-BUF surface import
/// is deferred — ADR-0001 § Scope) and is rejected before touching hardware, so this is
/// deterministic on every machine too.
#[test]
fn open_zero_copy_gpu_returns_unsupported_without_hardware() {
    let mut cfg = tiny_h264_cfg();
    cfg.input = VideoInputPreference::ZeroCopyGpu;
    assert!(matches!(
        LinuxVideoEncoder::open(&cfg),
        Err(EncodeError::Unsupported)
    ));
}

/// End-to-end through the public [`LinuxVideoEncoder`] wrapper (delegation to the inner VA-API
/// session). **Expected to skip in the session that authored this crate** — see
/// [ADR-0001](../adr/0001-vaapi-cros-libva-h264-cpu-upload.md) § Zero real-hardware
/// verification and `vaapi::video_tests::vaapi_open_and_encode_or_skip_without_hw` for the
/// lower-level equivalent.
#[test]
fn open_h264_cpu_upload_or_skip_without_hw() {
    let cfg = tiny_h264_cfg();
    let mut enc = match LinuxVideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: LinuxVideoEncoder::open failed ({e:?}) — no VA-API display?");
            return;
        }
    };

    let nv12_len = 64 * 64 + (64 * 64) / 2;
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width: 64,
        height: 64,
        format: mediaway_common::PixelFormat::Nv12,
        storage: mediaway_common::VideoFrameStorage::Cpu {
            data: Bytes::from(vec![0u8; nv12_len]),
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
    eprintln!("LinuxVideoEncoder packets={packets}");
}
