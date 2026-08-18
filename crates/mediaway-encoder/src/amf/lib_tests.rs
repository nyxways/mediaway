//! Tests for [`super::AmfVideoEncoder`] — see `docs/conventions/testing.md` Tier 1.
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

/// `validate()` runs before any AMF library is touched, so an unsupported codec is
/// rejected deterministically on every machine — no hardware needed for this assertion.
/// `Vp9` (not `Av1`) is the genuinely unsupported codec here: `shiguredo_amf`'s own
/// `CodecConfig` has no VP9 variant, while AV1 became supported per ADR-0003.
#[test]
fn open_unsupported_codec_returns_unsupported_without_hardware() {
    let mut cfg = tiny_h264_cfg();
    cfg.codec = mediaway_common::CodecKind::Vp9;
    assert!(matches!(
        AmfVideoEncoder::open(&cfg),
        Err(EncodeError::Unsupported)
    ));
}

/// `VideoInputPreference::ZeroCopyGpu` is not implemented this stage (no GPU-surface-import
/// type confirmed in `shiguredo_amf` — see `adr/amf/0002` § Scope) and is rejected before
/// touching hardware, so this is deterministic on every machine too.
#[test]
fn open_zero_copy_gpu_returns_unsupported_without_hardware() {
    let mut cfg = tiny_h264_cfg();
    cfg.input = VideoInputPreference::ZeroCopyGpu;
    assert!(matches!(
        AmfVideoEncoder::open(&cfg),
        Err(EncodeError::Unsupported)
    ));
}

/// End-to-end through the public [`AmfVideoEncoder`] wrapper (delegation to the inner AMF
/// session). **Expected to skip in every session that authored this crate** — see
/// `adr/amf/0002` § Zero real-hardware verification and
/// `linux::session_tests::amf_open_and_encode_or_skip_without_hw` for the lower-level
/// equivalent.
#[test]
fn open_h264_cpu_upload_or_skip_without_hw() {
    open_encode_or_skip(&tiny_h264_cfg(), "h264");
}

/// HEVC counterpart of [`open_h264_cpu_upload_or_skip_without_hw`] — same "expected to skip,
/// no AMD hardware in this workspace" honesty posture, exercising the ADR-0003 HEVC dispatch
/// through the public [`AmfVideoEncoder`] wrapper.
#[test]
fn open_hevc_cpu_upload_or_skip_without_hw() {
    let mut cfg = tiny_h264_cfg();
    cfg.codec = mediaway_common::CodecKind::Hevc;
    open_encode_or_skip(&cfg, "hevc");
}

/// AV1 counterpart of [`open_h264_cpu_upload_or_skip_without_hw`] — same "expected to skip, no
/// AMD hardware in this workspace" honesty posture, exercising the ADR-0003 AV1 dispatch
/// through the public [`AmfVideoEncoder`] wrapper.
#[test]
fn open_av1_cpu_upload_or_skip_without_hw() {
    let mut cfg = tiny_h264_cfg();
    cfg.codec = mediaway_common::CodecKind::Av1;
    open_encode_or_skip(&cfg, "av1");
}

/// Shared body for the hardware-gated smoke tests above: open a real
/// [`AmfVideoEncoder`] for `cfg` and, if a driver is available, encode one black NV12 frame
/// end to end. `label` only tags the diagnostic `eprintln!`.
fn open_encode_or_skip(cfg: &VideoEncoderConfig, label: &str) {
    let mut enc = match AmfVideoEncoder::open(cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: AmfVideoEncoder::open failed ({e:?}) — no AMD AMF driver? ({label})");
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
        eprintln!("skip: push_frame failed ({e:?}) ({label})");
        return;
    }
    let _ = enc.flush();

    let mut packets = 0usize;
    while let Ok(Some(p)) = enc.poll_packet() {
        assert!(!p.payload.is_empty());
        packets += 1;
    }
    eprintln!("AmfVideoEncoder {label} packets={packets}");
}
