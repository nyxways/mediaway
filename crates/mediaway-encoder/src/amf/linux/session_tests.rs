//! Tests for [`super::AmfSession`] — see `docs/conventions/testing.md` Tier 1.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;
use mediaway_common::{CodecKind, Rational};

fn tiny_h264_cfg(width: u32, height: u32) -> VideoEncoderConfig {
    VideoEncoderConfig {
        codec: CodecKind::H264,
        width,
        height,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    }
}

// --- Pure logic: no AMF library/driver needed ----------------------------------------------

#[test]
fn validate_accepts_h264_cpu_upload_config() {
    assert!(validate(&tiny_h264_cfg(64, 64)).is_ok());
}

#[test]
fn validate_accepts_hevc_and_av1_cpu_upload_config() {
    let mut cfg = tiny_h264_cfg(64, 64);
    cfg.codec = CodecKind::Hevc;
    assert!(validate(&cfg).is_ok());
    cfg.codec = CodecKind::Av1;
    assert!(validate(&cfg).is_ok());
}

#[test]
fn validate_rejects_vp9_codec() {
    // `shiguredo_amf`'s own `CodecConfig` has no VP9 variant — ADR-0003 § Context.
    let mut cfg = tiny_h264_cfg(64, 64);
    cfg.codec = CodecKind::Vp9;
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_dimensions() {
    let cfg = tiny_h264_cfg(0, 64);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn validate_rejects_non_nv12_pixel_format() {
    let mut cfg = tiny_h264_cfg(64, 64);
    cfg.pixel_format = PixelFormat::Bgra8;
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_timebase_denominator() {
    let mut cfg = tiny_h264_cfg(64, 64);
    cfg.time_base = Rational::new(1, 0);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn validate_rejects_zero_gop_size() {
    let mut cfg = tiny_h264_cfg(64, 64);
    cfg.gop_size = 0;
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn stream_info_from_config_carries_geometry_and_timebase() {
    let cfg = tiny_h264_cfg(64, 32);
    let info = stream_info_from(&cfg);
    assert!(matches!(
        info,
        StreamInfo::Video {
            codec: CodecKind::H264,
            ..
        }
    ));
    if let StreamInfo::Video {
        time_base,
        geometry,
        ..
    } = info
    {
        assert_eq!(time_base, cfg.time_base);
        assert_eq!(geometry.width, 64);
        assert_eq!(geometry.height, 32);
    }
}

/// Regression test for the ADR-0003-fixed `stream_info_from` bug: the returned `codec` must
/// track `config.codec` for HEVC/AV1, not stay hardcoded to `CodecKind::H264`.
#[test]
fn stream_info_from_carries_hevc_and_av1_codec() {
    let mut cfg = tiny_h264_cfg(64, 32);
    cfg.codec = CodecKind::Hevc;
    let info = stream_info_from(&cfg);
    assert!(matches!(
        info,
        StreamInfo::Video {
            codec: CodecKind::Hevc,
            ..
        }
    ));

    cfg.codec = CodecKind::Av1;
    let info = stream_info_from(&cfg);
    assert!(matches!(
        info,
        StreamInfo::Video {
            codec: CodecKind::Av1,
            ..
        }
    ));
}

#[test]
fn codec_config_for_dispatches_h264_hevc_av1() {
    assert!(matches!(
        codec_config_for(CodecKind::H264),
        Ok(CodecConfig::H264(_))
    ));
    assert!(matches!(
        codec_config_for(CodecKind::Hevc),
        Ok(CodecConfig::Hevc(_))
    ));
    assert!(matches!(
        codec_config_for(CodecKind::Av1),
        Ok(CodecConfig::Av1(_))
    ));
    assert!(matches!(
        codec_config_for(CodecKind::Vp9),
        Err(EncodeError::Unsupported)
    ));
}

// --- Hardware-gated: real shiguredo_amf / AMD AMF session ----------------------------------

/// Opens a real AMD AMF H.264 CPU-upload session and, if a driver is available, encodes one
/// black NV12 frame end to end (`Encoder::new` → `alloc_surface` → `upload_cpu_nv12` →
/// `encode` → callback-populated queue → `finish` → `poll_packet`).
///
/// **Expected to skip in every session that authored this crate** — no AMD GPU/driver is
/// available anywhere in this workspace's sessions (Windows host; the WSL2 environment used
/// for compile verification has no AMD hardware/driver either). See
/// [ADR-0002](../../adr/amf/0002-amf-linux-shiguredo-amf-h264-cpu-upload.md) § Zero
/// real-hardware verification. This test is provided so a future run on real AMD hardware +
/// driver can verify the path this crate was written against.
#[test]
fn amf_open_and_encode_or_skip_without_hw() {
    open_encode_or_skip(&tiny_h264_cfg(64, 64), "h264");
}

/// HEVC counterpart of [`amf_open_and_encode_or_skip_without_hw`] — same "expected to skip,
/// no AMD hardware in this workspace" honesty posture, exercising the ADR-0003 HEVC dispatch.
#[test]
fn amf_open_and_encode_hevc_or_skip_without_hw() {
    let mut cfg = tiny_h264_cfg(64, 64);
    cfg.codec = CodecKind::Hevc;
    open_encode_or_skip(&cfg, "hevc");
}

/// AV1 counterpart of [`amf_open_and_encode_or_skip_without_hw`] — same "expected to skip, no
/// AMD hardware in this workspace" honesty posture, exercising the ADR-0003 AV1 dispatch.
#[test]
fn amf_open_and_encode_av1_or_skip_without_hw() {
    let mut cfg = tiny_h264_cfg(64, 64);
    cfg.codec = CodecKind::Av1;
    open_encode_or_skip(&cfg, "av1");
}

/// Shared body for the hardware-gated smoke tests above: open a real AMD AMF CPU-upload
/// session for `cfg` and, if a driver is available, encode one black NV12 frame end to end
/// (`Encoder::new` → `alloc_surface` → `upload_cpu_nv12` → `encode` → callback-populated queue
/// → `finish` → `poll_packet`). `label` only tags the diagnostic `eprintln!`.
fn open_encode_or_skip(cfg: &VideoEncoderConfig, label: &str) {
    let mut session = match AmfSession::open(cfg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skip: AmfSession::open failed ({e:?}) — no AMD AMF driver? ({label})");
            return;
        }
    };

    let nv12_len = 64 * 64 + (64 * 64) / 2;
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width: 64,
        height: 64,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from(vec![0u8; nv12_len]),
        },
    };

    if let Err(e) = session.push_frame(&frame) {
        eprintln!("skip: push_frame failed ({e:?}) — no usable AMF encode session? ({label})");
        return;
    }
    let _ = session.flush();

    let mut packets = 0usize;
    while let Ok(Some(p)) = session.poll_packet() {
        assert!(!p.payload.is_empty());
        packets += 1;
    }
    eprintln!("amf {label} cpu-upload packets={packets}");
}
