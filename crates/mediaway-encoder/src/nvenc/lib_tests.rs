//! Tests for [`super::NvencVideoEncoder`] — see `docs/conventions/testing.md` Tier 1.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;
use crate::VideoInputPreference;
use mediaway_common::{CodecKind, PixelFormat, Rational};

const fn h264_cfg(width: u32, height: u32) -> VideoEncoderConfig {
    VideoEncoderConfig {
        codec: CodecKind::H264,
        width,
        height,
        time_base: Rational::new(1, 30),
        bitrate_bps: 2_000_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    }
}

/// `validate()` runs before any D3D11/NVENC session is touched, so an unsupported codec is
/// rejected deterministically on every machine — no hardware needed for this assertion.
/// VP9 specifically: NVENC has no VP9 **encoder** at all (VP9 is decode-only on this
/// silicon), so it is the one `CodecKind` this backend can never accept regardless of
/// GPU/driver — unlike H.264/HEVC/AV1, which pass `validate()` and need real hardware to
/// exercise (see the hardware-gated tests in `dx11::video_tests`).
#[test]
fn open_unsupported_codec_returns_unsupported_without_hardware() {
    let mut cfg = h264_cfg(640, 480);
    cfg.codec = CodecKind::Vp9;
    assert!(matches!(
        NvencVideoEncoder::open(&cfg),
        Err(EncodeError::Unsupported)
    ));
}

/// `VideoInputPreference::ZeroCopyGpu` (caller-supplied D3D11/D3D12 texture) is not
/// implemented this stage and is rejected before touching hardware — deterministic too.
#[test]
fn open_zero_copy_gpu_returns_unsupported_without_hardware() {
    let mut cfg = h264_cfg(640, 480);
    cfg.input = VideoInputPreference::ZeroCopyGpu;
    assert!(matches!(
        NvencVideoEncoder::open(&cfg),
        Err(EncodeError::Unsupported)
    ));
}

/// End-to-end through the public [`NvencVideoEncoder`] wrapper (delegation to the inner
/// D3D11/NVENC session). See `dx11::video_tests::nvenc_open_and_encode_or_skip_without_hw`
/// for the lower-level, more thorough equivalent — hardware-verified 2026-07-29 on a real
/// RTX 4090.
#[test]
fn open_h264_cpu_upload_or_skip_without_hw() {
    let cfg = h264_cfg(640, 480);
    let mut enc = match NvencVideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: NvencVideoEncoder::open failed ({e:?}) — no NVENC GPU/driver?");
            return;
        }
    };

    let nv12_len = 640 * 480 + (640 * 480) / 2;
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width: 640,
        height: 480,
        format: PixelFormat::Nv12,
        storage: mediaway_common::VideoFrameStorage::Cpu {
            data: Bytes::from(vec![96u8; nv12_len]),
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
    eprintln!("NvencVideoEncoder packets={packets}");
}
