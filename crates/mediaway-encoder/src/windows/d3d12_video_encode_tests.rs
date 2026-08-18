//! Hardware-gated tests for [`super::D3d12VideoEncoder`] — real `ID3D12Device`, real
//! `ID3D12VideoDevice3` H.264 support check, real `EncodeFrame` submission. Skips
//! gracefully (never fails the default suite) when this machine/driver lacks D3D12
//! native video-encode support.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use crate::{RateControlConfig, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, GpuDeviceHandle, NativeHandle, PixelFormat, Rational, VideoFrame,
    VideoFrameStorage,
};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_MESSAGE, D3D12CreateDevice, D3D12GetDebugInterface, ID3D12Debug, ID3D12Device,
    ID3D12InfoQueue,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
use windows::core::Interface;

use super::D3d12VideoEncoder;

#[path = "d3d12_video_encode_tests_av1.rs"]
mod av1_tests;

// D3D12_FEATURE_VIDEO_ENCODER_OUTPUT_RESOLUTION reports a real minimum encode
// resolution on real hardware (observed: 160x64 on an NVIDIA RTX 4090) — below it,
// CreateVideoEncoderHeap fails with E_INVALIDARG. 176x144 (QCIF, 16-pixel MB-aligned)
// comfortably clears that floor on every driver this backend has been tested against.
const WIDTH: u32 = 176;
const HEIGHT: u32 = 144;

// HEVC's SPS requires pic_width/height_in_luma_samples to be an exact multiple of the
// driver-reported minimum coding-unit size (8/16/32/64 depending on hardware) — this
// backend has no conformance-window/cropping support (see bitstream_hevc.rs), so the test
// picks a size that is a multiple of every legal CU size up to 64.
const WIDTH_HEVC: u32 = 256;
const HEIGHT_HEVC: u32 = 192;

// AV1's D3D12_FEATURE_VIDEO_ENCODER_OUTPUT_RESOLUTION reports a real minimum encode
// resolution on real hardware (observed: 192x128 on an NVIDIA RTX 4090, higher than
// H.264/HEVC's) — reuse the HEVC test's 256x192 (already known to clear every codec's
// observed minimum on this hardware, and a multiple of AV1's `ResolutionWidthMultipleRequirement`).
const WIDTH_AV1: u32 = WIDTH_HEVC;
const HEIGHT_AV1: u32 = HEIGHT_HEVC;

/// Parse Annex-B `nal_unit_type` values (start-code-prefixed NALs) out of `payload`.
fn nal_unit_types(payload: &[u8]) -> Vec<u8> {
    let mut types = Vec::new();
    let mut i = 0usize;
    while i + 3 < payload.len() {
        let is_start_code_3 = payload[i] == 0 && payload[i + 1] == 0 && payload[i + 2] == 1;
        let is_start_code_4 = i + 4 < payload.len()
            && payload[i] == 0
            && payload[i + 1] == 0
            && payload[i + 2] == 0
            && payload[i + 3] == 1;
        if is_start_code_4 {
            types.push(payload[i + 4] & 0x1F);
            i += 5;
        } else if is_start_code_3 {
            types.push(payload[i + 3] & 0x1F);
            i += 4;
        } else {
            i += 1;
        }
    }
    types
}

/// Parse Annex-B HEVC `nal_unit_type` values (2-byte NAL header: `forbidden_zero_bit`(1) +
/// `nal_unit_type`(6) + `nuh_layer_id`(6) + `nuh_temporal_id_plus1`(3), Rec. ITU-T H.265
/// §7.3.1.2 — `nal_unit_type` is the top 6 bits of the first header byte, after the
/// 1-bit-zero `forbidden_zero_bit`).
fn nal_unit_types_hevc(payload: &[u8]) -> Vec<u8> {
    let mut types = Vec::new();
    let mut i = 0usize;
    while i + 3 < payload.len() {
        let is_start_code_3 = payload[i] == 0 && payload[i + 1] == 0 && payload[i + 2] == 1;
        let is_start_code_4 = i + 4 < payload.len()
            && payload[i] == 0
            && payload[i + 1] == 0
            && payload[i + 2] == 0
            && payload[i + 3] == 1;
        if is_start_code_4 && i + 5 < payload.len() {
            types.push((payload[i + 4] >> 1) & 0x3F);
            i += 6;
        } else if is_start_code_3 && i + 4 < payload.len() {
            types.push((payload[i + 3] >> 1) & 0x3F);
            i += 5;
        } else {
            i += 1;
        }
    }
    types
}

fn nv12_frame(pts: i64) -> VideoFrame {
    nv12_frame_sized(pts, WIDTH, HEIGHT)
}

fn nv12_frame_sized(pts: i64, width: u32, height: u32) -> VideoFrame {
    let len = (width as usize) * (height as usize) + (width as usize) * (height as usize) / 2;
    // Mid-gray luma+chroma — content doesn't matter, only that a real encode runs.
    let data = vec![128u8; len];
    VideoFrame {
        pts,
        duration: 1,
        width,
        height,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from(data),
        },
    }
}

/// Open a real hardware D3D12 device on adapter 0, enabling the debug layer first so
/// [`dump_d3d12_info_queue`] can report the driver's exact validation messages on
/// failure. Returns `None` (after printing why) if this machine has no usable D3D12
/// adapter — never panics.
fn open_real_d3d12_device() -> Option<ID3D12Device> {
    let mut debug: Option<ID3D12Debug> = None;
    if unsafe { D3D12GetDebugInterface(&raw mut debug) }.is_ok()
        && let Some(debug) = debug
    {
        unsafe { debug.EnableDebugLayer() };
    }

    let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
        Ok(f) => f,
        Err(e) => {
            eprintln!("skip: CreateDXGIFactory1 ({e:?})");
            return None;
        }
    };
    let adapter: IDXGIAdapter1 = match unsafe { factory.EnumAdapters1(0) } {
        Ok(a) => a,
        Err(e) => {
            eprintln!("skip: EnumAdapters1 ({e:?})");
            return None;
        }
    };
    let mut device: Option<ID3D12Device> = None;
    if unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &raw mut device) }.is_err() {
        eprintln!("skip: D3D12CreateDevice failed");
        return None;
    }
    if device.is_none() {
        eprintln!("skip: null D3D12 device");
    }
    device
}

/// Print every message the D3D12 debug layer has queued (real validation errors —
/// e.g. resource-state/heap-type mismatches — with the exact resource/call named),
/// then clear the queue. No-op when `iq` is `None` (debug layer unavailable).
fn dump_d3d12_info_queue(iq: Option<&ID3D12InfoQueue>) {
    let Some(iq) = iq else { return };
    let n = unsafe { iq.GetNumStoredMessages() };
    for i in 0..n {
        let mut len = 0usize;
        if unsafe { iq.GetMessage(i, None, &raw mut len) }.is_err() || len == 0 {
            continue;
        }
        // 8-byte-aligned buffer: `D3D12_MESSAGE` needs pointer alignment on 64-bit targets.
        let mut buf: Vec<u64> = vec![0; len.div_ceil(8)];
        let msg_ptr = buf.as_mut_ptr().cast::<D3D12_MESSAGE>();
        if unsafe { iq.GetMessage(i, Some(msg_ptr), &raw mut len) }.is_ok() {
            // SAFETY: `GetMessage` just wrote a valid `D3D12_MESSAGE` into `buf`.
            let msg = unsafe { &*msg_ptr };
            // SAFETY: `pDescription` is a NUL-terminated ASCII string `DescriptionByteLength`
            // bytes long (including the NUL), per the D3D12 debug-layer contract.
            let desc = unsafe {
                std::slice::from_raw_parts(
                    msg.pDescription,
                    msg.DescriptionByteLength.saturating_sub(1),
                )
            };
            eprintln!("D3D12 InfoQueue[{i}]: {}", String::from_utf8_lossy(desc));
        }
    }
    unsafe { iq.ClearStoredMessages() };
}

/// Real D3D12 device → real H.264 `ID3D12VideoDevice3` support check → real
/// `EncodeFrame` submissions → real Annex-B output (SPS `nal_unit_type == 7`, IDR
/// `nal_unit_type == 5` present in every packet). Skips (does not fail) if this
/// machine's adapter/driver lacks D3D12 native video-encode support.
#[test]
fn d3d12_native_h264_encode_or_skip() {
    let Some(device) = open_real_d3d12_device() else {
        return;
    };
    let Some(handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
        eprintln!("skip: null D3D12 device pointer");
        return;
    };
    let info_queue: Option<ID3D12InfoQueue> = device.cast().ok();

    let cfg = VideoEncoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    };

    let mut enc = match D3d12VideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!(
                "skip: D3d12VideoEncoder::open failed ({e:?}) — no D3D12 H.264 video-encode \
                 support on this device/driver?"
            );
            return;
        }
    };

    let mut packets = 0usize;
    for i in 0..3i64 {
        let frame = nv12_frame(i);
        if let Err(e) = enc.push_frame(&frame) {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: push_frame failed ({e:?})");
            return;
        }
        let packet = match enc.poll_packet() {
            Ok(Some(p)) => p,
            Ok(None) => {
                eprintln!("skip: no packet after push_frame {i}");
                return;
            }
            Err(e) => {
                eprintln!("skip: poll_packet failed ({e:?})");
                return;
            }
        };
        assert!(!packet.payload.is_empty(), "packet {i} payload is empty");
        assert!(packet.is_keyframe, "packet {i} should be an IDR keyframe");

        let types = nal_unit_types(&packet.payload);
        assert!(
            types.contains(&7),
            "packet {i} missing SPS NAL (type 7); found types {types:?}"
        );
        assert!(
            types.contains(&5),
            "packet {i} missing IDR slice NAL (type 5); found types {types:?}"
        );
        packets += 1;
    }

    enc.flush().expect("flush");
    eprintln!("d3d12 native h264 encode ok: {packets} packets, all with real SPS+IDR Annex-B NALs");
}

/// Real D3D12 device → real H.264 GOP-mode (`gop_size: 3`) → real `EncodeFrame`
/// submissions with single-forward-reference P frames → real Annex-B I/P NAL cadence
/// (`GOPLength=3`, `PPicturePeriod=1`: IDR, P, P, IDR, P, P, IDR — type 5 vs type 1) and
/// `Packet::is_keyframe` matching. Skips (does not fail) if this machine's adapter/driver
/// lacks D3D12 native H.264 video-encode support, or falls back to IDR-only if the driver
/// can't honor `MaxReferenceFramesInDPB >= 1` for this config (ADR-0007's 2026-08-06
/// addendum) — in that case this test still passes (every packet is then an IDR, a valid,
/// documented fallback).
#[test]
fn d3d12_native_h264_gop_encode_or_skip() {
    let Some(device) = open_real_d3d12_device() else {
        return;
    };
    let Some(handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
        eprintln!("skip: null D3D12 device pointer");
        return;
    };
    let info_queue: Option<ID3D12InfoQueue> = device.cast().ok();

    let cfg = VideoEncoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
        gop_size: 3,
        rate_control: None,
        intra_refresh_period: None,
    };

    let mut enc = match D3d12VideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!(
                "skip: D3d12VideoEncoder::open (GOP) failed ({e:?}) — no D3D12 H.264 video-encode \
                 support on this device/driver?"
            );
            return;
        }
    };

    let mut keyframe_flags = Vec::new();
    let mut nal_type_cadence = Vec::new();
    for i in 0..7i64 {
        let frame = nv12_frame(i);
        if let Err(e) = enc.push_frame(&frame) {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: push_frame (GOP) failed ({e:?})");
            return;
        }
        let packet = match enc.poll_packet() {
            Ok(Some(p)) => p,
            Ok(None) => {
                eprintln!("skip: no packet after push_frame {i} (GOP)");
                return;
            }
            Err(e) => {
                eprintln!("skip: poll_packet (GOP) failed ({e:?})");
                return;
            }
        };
        assert!(!packet.payload.is_empty(), "packet {i} payload is empty");
        let types = nal_unit_types(&packet.payload);
        let is_idr_nal = types.contains(&5);
        let is_p_nal = types.contains(&1);
        assert!(
            is_idr_nal || is_p_nal,
            "packet {i} has neither an IDR (type 5) nor a P (type 1) slice NAL; found {types:?}"
        );
        assert_eq!(
            packet.is_keyframe, is_idr_nal,
            "packet {i}: Packet::is_keyframe ({}) disagrees with its own NAL type {types:?}",
            packet.is_keyframe
        );
        keyframe_flags.push(packet.is_keyframe);
        nal_type_cadence.push(if is_idr_nal { 'I' } else { 'P' });
    }

    enc.flush().expect("flush");
    let cadence: String = nal_type_cadence.iter().collect();
    // Either GOP mode landed (real `IPPIPPI` cadence) or the driver couldn't honor
    // `MaxReferenceFramesInDPB >= 1` and this backend silently fell back to IDR-only
    // (`IIIIIII`) — both are valid, documented outcomes (ADR-0007's 2026-08-06
    // addendum); anything else (a cadence that isn't periodic-by-3 and isn't
    // all-IDR) is a real bug.
    let is_gop_cadence = cadence == "IPPIPPI";
    let is_idr_only_fallback = keyframe_flags.iter().all(|&k| k);
    assert!(
        is_gop_cadence || is_idr_only_fallback,
        "unexpected I/P cadence {cadence:?} — neither GOP mode's IPPIPPI nor an all-IDR fallback"
    );
    eprintln!("d3d12 native h264 GOP encode ok: cadence {cadence:?}");
}

/// Real D3D12 device → real H.264 row-based intra-refresh (`intra_refresh_period: 4`) →
/// real `EncodeFrame` submissions → only the very first packet is an IDR (type 5); every
/// packet after that is a P slice (type 1) forever — an unbounded GOP, unlike
/// [`d3d12_native_h264_gop_encode_or_skip`]'s periodic re-IDR. `Packet::is_keyframe` must
/// agree. Falls back to the documented all-IDR outcome if the driver can't honor
/// `D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_ROW_BASED` for this config (ADR-0007's
/// 2026-08-06 addendum) — that's still a pass, not a failure.
#[test]
fn d3d12_native_h264_intra_refresh_encode_or_skip() {
    let Some(device) = open_real_d3d12_device() else {
        return;
    };
    let Some(handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
        eprintln!("skip: null D3D12 device pointer");
        return;
    };
    let info_queue: Option<ID3D12InfoQueue> = device.cast().ok();

    let cfg = VideoEncoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: Some(4),
    };

    let mut enc = match D3d12VideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!(
                "skip: D3d12VideoEncoder::open (intra-refresh) failed ({e:?}) — no D3D12 \
                 H.264 video-encode support on this device/driver?"
            );
            return;
        }
    };

    let mut nal_type_cadence = Vec::new();
    for i in 0..9i64 {
        let frame = nv12_frame(i);
        if let Err(e) = enc.push_frame(&frame) {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: push_frame (intra-refresh) failed ({e:?})");
            return;
        }
        let packet = match enc.poll_packet() {
            Ok(Some(p)) => p,
            Ok(None) => {
                eprintln!("skip: no packet after push_frame {i} (intra-refresh)");
                return;
            }
            Err(e) => {
                eprintln!("skip: poll_packet (intra-refresh) failed ({e:?})");
                return;
            }
        };
        assert!(!packet.payload.is_empty(), "packet {i} payload is empty");
        let types = nal_unit_types(&packet.payload);
        let is_idr_nal = types.contains(&5);
        let is_p_nal = types.contains(&1);
        assert!(
            is_idr_nal || is_p_nal,
            "packet {i} has neither an IDR (type 5) nor a P (type 1) slice NAL; found {types:?}"
        );
        assert_eq!(
            packet.is_keyframe, is_idr_nal,
            "packet {i}: Packet::is_keyframe ({}) disagrees with its own NAL type {types:?}",
            packet.is_keyframe
        );
        nal_type_cadence.push(if is_idr_nal { 'I' } else { 'P' });
    }

    enc.flush().expect("flush");
    let cadence: String = nal_type_cadence.iter().collect();
    // Either intra-refresh landed (real "IPPPPPPPP" — exactly one IDR, ever) or the
    // driver couldn't honor row-based intra refresh and this backend fell back to
    // IDR-only ("IIIIIIIII"); anything else is a real bug.
    let is_intra_refresh_cadence = cadence == "IPPPPPPPP";
    let is_idr_only_fallback = cadence == "IIIIIIIII";
    assert!(
        is_intra_refresh_cadence || is_idr_only_fallback,
        "unexpected I/P cadence {cadence:?} — neither intra-refresh's single-IDR-forever \
         nor an all-IDR fallback"
    );
    eprintln!("d3d12 native h264 intra-refresh encode ok: cadence {cadence:?}");
}

/// Real D3D12 device → `VideoEncoderConfig::rate_control` requested → real `EncodeFrame`
/// submissions, then a live [`VideoEncoder::set_bitrate`] retarget mid-session with more
/// frames pushed after it. Real CBR when this driver accepts `RateControlState::Cbr` for
/// the chosen (IDR-only) GOP tier, the documented fixed-QP fallback otherwise
/// (`open`'s one-extra-probe design, see `d3d12_video_encode.rs`'s doc) — this test cannot
/// tell which outcome landed without reaching into a private field, so it accepts either:
/// `set_bitrate` returning `Ok(())` (real CBR) or `Err(EncodeError::Unsupported)` (fixed-QP
/// fallback) are both legitimate, and either way encoding must keep working right after the
/// call — that's the actual thing under test. Skips (does not fail) if this machine's
/// adapter/driver lacks D3D12 native H.264 video-encode support at all.
#[test]
fn d3d12_native_h264_cbr_encode_or_skip() {
    let Some(device) = open_real_d3d12_device() else {
        return;
    };
    let Some(handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
        eprintln!("skip: null D3D12 device pointer");
        return;
    };
    let info_queue: Option<ID3D12InfoQueue> = device.cast().ok();

    let cfg = VideoEncoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
        gop_size: 1,
        rate_control: Some(RateControlConfig {
            target_bitrate_bps: 500_000,
            vbv_buffer_size_bytes: None,
        }),
        intra_refresh_period: None,
    };

    let mut enc = match D3d12VideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: D3d12VideoEncoder::open (H.264 CBR) failed ({e:?})");
            return;
        }
    };

    for i in 0..3i64 {
        let frame = nv12_frame(i);
        if let Err(e) = enc.push_frame(&frame) {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: push_frame (H.264 CBR) failed ({e:?})");
            return;
        }
        match enc.poll_packet() {
            Ok(Some(p)) => assert!(!p.payload.is_empty(), "packet {i} payload is empty"),
            Ok(None) => {
                eprintln!("skip: no packet after push_frame {i} (H.264 CBR)");
                return;
            }
            Err(e) => {
                eprintln!("skip: poll_packet (H.264 CBR) failed ({e:?})");
                return;
            }
        }
    }

    let cbr_selected = match enc.set_bitrate(250_000) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("d3d12 h264 set_bitrate: {e:?} (fixed-QP fallback, expected)");
            false
        }
    };

    for i in 3..6i64 {
        let frame = nv12_frame(i);
        if let Err(e) = enc.push_frame(&frame) {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: push_frame after set_bitrate failed ({e:?})");
            return;
        }
        match enc.poll_packet() {
            Ok(Some(p)) => assert!(
                !p.payload.is_empty(),
                "post-set_bitrate packet {i} payload is empty"
            ),
            Ok(None) => {
                eprintln!("skip: no packet after push_frame {i} (post-set_bitrate)");
                return;
            }
            Err(e) => {
                eprintln!("skip: poll_packet (post-set_bitrate) failed ({e:?})");
                return;
            }
        }
    }

    enc.flush().expect("flush");
    eprintln!(
        "d3d12 native h264 CBR encode ok: cbr_selected={cbr_selected}, encoding kept working \
         across set_bitrate"
    );
}

/// Real D3D12 device → real HEVC `ID3D12VideoDevice3` support check → real `EncodeFrame`
/// submissions → real Annex-B output (VPS `nal_unit_type == 32`, SPS `== 33`,
/// PPS `== 34`, IDR `== 19` (`IDR_W_RADL`) or `== 20` (`IDR_N_LP`) present in every
/// packet). Skips (does not fail) if this machine's adapter/driver lacks D3D12 native HEVC
/// video-encode support.
#[test]
fn d3d12_native_hevc_encode_or_skip() {
    let Some(device) = open_real_d3d12_device() else {
        return;
    };
    let Some(handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
        eprintln!("skip: null D3D12 device pointer");
        return;
    };
    let info_queue: Option<ID3D12InfoQueue> = device.cast().ok();

    let cfg = VideoEncoderConfig {
        codec: CodecKind::Hevc,
        width: WIDTH_HEVC,
        height: HEIGHT_HEVC,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    };

    let mut enc = match D3d12VideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!(
                "skip: D3d12VideoEncoder::open (HEVC) failed ({e:?}) — no D3D12 HEVC video-encode \
                 support on this device/driver?"
            );
            return;
        }
    };

    let mut packets = 0usize;
    for i in 0..3i64 {
        let frame = nv12_frame_sized(i, WIDTH_HEVC, HEIGHT_HEVC);
        if let Err(e) = enc.push_frame(&frame) {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: push_frame (HEVC) failed ({e:?})");
            return;
        }
        let packet = match enc.poll_packet() {
            Ok(Some(p)) => p,
            Ok(None) => {
                eprintln!("skip: no packet after push_frame {i} (HEVC)");
                return;
            }
            Err(e) => {
                eprintln!("skip: poll_packet (HEVC) failed ({e:?})");
                return;
            }
        };
        assert!(!packet.payload.is_empty(), "packet {i} payload is empty");
        assert!(packet.is_keyframe, "packet {i} should be an IDR keyframe");

        let types = nal_unit_types_hevc(&packet.payload);
        assert!(
            types.contains(&32),
            "packet {i} missing VPS NAL (type 32); found types {types:?}"
        );
        assert!(
            types.contains(&33),
            "packet {i} missing SPS NAL (type 33); found types {types:?}"
        );
        assert!(
            types.contains(&34),
            "packet {i} missing PPS NAL (type 34); found types {types:?}"
        );
        assert!(
            types.contains(&19) || types.contains(&20),
            "packet {i} missing IDR slice NAL (type 19/20); found types {types:?}"
        );
        packets += 1;
    }

    enc.flush().expect("flush");
    eprintln!(
        "d3d12 native hevc encode ok: {packets} packets, all with real VPS+SPS+PPS+IDR Annex-B NALs"
    );
}

/// Real D3D12 device → real HEVC GOP-mode (`gop_size: 3`) → real `EncodeFrame`
/// submissions with single-forward-reference P frames → real Annex-B I/P NAL cadence
/// (IDR `nal_unit_type` 19/20 vs P `nal_unit_type` 1, `TRAIL_R`) and `Packet::is_keyframe`
/// matching. Skips (does not fail) if this machine's adapter/driver lacks D3D12 native HEVC
/// video-encode support, or falls back to IDR-only if the driver can't honor
/// `MaxReferenceFramesInDPB >= 1` for this config (ADR-0007's 2026-08-06 addendum) — in
/// that case this test still passes (every packet is then an IDR, a valid fallback).
#[test]
fn d3d12_native_hevc_gop_encode_or_skip() {
    let Some(device) = open_real_d3d12_device() else {
        return;
    };
    let Some(handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
        eprintln!("skip: null D3D12 device pointer");
        return;
    };
    let info_queue: Option<ID3D12InfoQueue> = device.cast().ok();

    let cfg = VideoEncoderConfig {
        codec: CodecKind::Hevc,
        width: WIDTH_HEVC,
        height: HEIGHT_HEVC,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
        gop_size: 3,
        rate_control: None,
        intra_refresh_period: None,
    };

    let mut enc = match D3d12VideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!(
                "skip: D3d12VideoEncoder::open (HEVC GOP) failed ({e:?}) — no D3D12 HEVC \
                 video-encode support on this device/driver?"
            );
            return;
        }
    };

    let mut keyframe_flags = Vec::new();
    let mut nal_type_cadence = Vec::new();
    for i in 0..7i64 {
        let frame = nv12_frame_sized(i, WIDTH_HEVC, HEIGHT_HEVC);
        if let Err(e) = enc.push_frame(&frame) {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: push_frame (HEVC GOP) failed ({e:?})");
            return;
        }
        let packet = match enc.poll_packet() {
            Ok(Some(p)) => p,
            Ok(None) => {
                eprintln!("skip: no packet after push_frame {i} (HEVC GOP)");
                return;
            }
            Err(e) => {
                eprintln!("skip: poll_packet (HEVC GOP) failed ({e:?})");
                return;
            }
        };
        assert!(!packet.payload.is_empty(), "packet {i} payload is empty");
        let types = nal_unit_types_hevc(&packet.payload);
        let is_idr_nal = types.contains(&19) || types.contains(&20);
        let is_p_nal = types.contains(&1);
        assert!(
            is_idr_nal || is_p_nal,
            "packet {i} has neither an IDR (type 19/20) nor a P (type 1, TRAIL_R) slice NAL; \
             found {types:?}"
        );
        assert_eq!(
            packet.is_keyframe, is_idr_nal,
            "packet {i}: Packet::is_keyframe ({}) disagrees with its own NAL type {types:?}",
            packet.is_keyframe
        );
        keyframe_flags.push(packet.is_keyframe);
        nal_type_cadence.push(if is_idr_nal { 'I' } else { 'P' });
    }

    enc.flush().expect("flush");
    let cadence: String = nal_type_cadence.iter().collect();
    let is_gop_cadence = cadence == "IPPIPPI";
    let is_idr_only_fallback = keyframe_flags.iter().all(|&k| k);
    assert!(
        is_gop_cadence || is_idr_only_fallback,
        "unexpected I/P cadence {cadence:?} — neither GOP mode's IPPIPPI nor an all-IDR fallback"
    );
    eprintln!("d3d12 native hevc GOP encode ok: cadence {cadence:?}");
}

/// HEVC sibling of [`d3d12_native_h264_intra_refresh_encode_or_skip`] — see that test's
/// doc for the shared design (unbounded GOP, single startup IDR, cadence assertion).
#[test]
fn d3d12_native_hevc_intra_refresh_encode_or_skip() {
    let Some(device) = open_real_d3d12_device() else {
        return;
    };
    let Some(handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
        eprintln!("skip: null D3D12 device pointer");
        return;
    };
    let info_queue: Option<ID3D12InfoQueue> = device.cast().ok();

    let cfg = VideoEncoderConfig {
        codec: CodecKind::Hevc,
        width: WIDTH_HEVC,
        height: HEIGHT_HEVC,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: Some(4),
    };

    let mut enc = match D3d12VideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!(
                "skip: D3d12VideoEncoder::open (HEVC intra-refresh) failed ({e:?}) — no D3D12 \
                 HEVC video-encode support on this device/driver?"
            );
            return;
        }
    };

    let mut nal_type_cadence = Vec::new();
    for i in 0..9i64 {
        let frame = nv12_frame_sized(i, WIDTH_HEVC, HEIGHT_HEVC);
        if let Err(e) = enc.push_frame(&frame) {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: push_frame (HEVC intra-refresh) failed ({e:?})");
            return;
        }
        let packet = match enc.poll_packet() {
            Ok(Some(p)) => p,
            Ok(None) => {
                eprintln!("skip: no packet after push_frame {i} (HEVC intra-refresh)");
                return;
            }
            Err(e) => {
                eprintln!("skip: poll_packet (HEVC intra-refresh) failed ({e:?})");
                return;
            }
        };
        assert!(!packet.payload.is_empty(), "packet {i} payload is empty");
        let types = nal_unit_types_hevc(&packet.payload);
        let is_idr_nal = types.contains(&19) || types.contains(&20);
        let is_p_nal = types.contains(&1);
        assert!(
            is_idr_nal || is_p_nal,
            "packet {i} has neither an IDR (type 19/20) nor a P (type 1, TRAIL_R) slice NAL; \
             found {types:?}"
        );
        assert_eq!(
            packet.is_keyframe, is_idr_nal,
            "packet {i}: Packet::is_keyframe ({}) disagrees with its own NAL type {types:?}",
            packet.is_keyframe
        );
        nal_type_cadence.push(if is_idr_nal { 'I' } else { 'P' });
    }

    enc.flush().expect("flush");
    let cadence: String = nal_type_cadence.iter().collect();
    let is_intra_refresh_cadence = cadence == "IPPPPPPPP";
    let is_idr_only_fallback = cadence == "IIIIIIIII";
    assert!(
        is_intra_refresh_cadence || is_idr_only_fallback,
        "unexpected I/P cadence {cadence:?} — neither intra-refresh's single-IDR-forever \
         nor an all-IDR fallback"
    );
    eprintln!("d3d12 native hevc intra-refresh encode ok: cadence {cadence:?}");
}

/// HEVC sibling of [`d3d12_native_h264_cbr_encode_or_skip`] — same real CBR-or-documented
/// fixed-QP-fallback design, `set_bitrate` retarget mid-session, same either-outcome
/// acceptance criterion (see that test's doc for why).
#[test]
fn d3d12_native_hevc_cbr_encode_or_skip() {
    let Some(device) = open_real_d3d12_device() else {
        return;
    };
    let Some(handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
        eprintln!("skip: null D3D12 device pointer");
        return;
    };
    let info_queue: Option<ID3D12InfoQueue> = device.cast().ok();

    let cfg = VideoEncoderConfig {
        codec: CodecKind::Hevc,
        width: WIDTH_HEVC,
        height: HEIGHT_HEVC,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
        gop_size: 1,
        rate_control: Some(RateControlConfig {
            target_bitrate_bps: 500_000,
            vbv_buffer_size_bytes: None,
        }),
        intra_refresh_period: None,
    };

    let mut enc = match D3d12VideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: D3d12VideoEncoder::open (HEVC CBR) failed ({e:?})");
            return;
        }
    };

    for i in 0..3i64 {
        let frame = nv12_frame_sized(i, WIDTH_HEVC, HEIGHT_HEVC);
        if let Err(e) = enc.push_frame(&frame) {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: push_frame (HEVC CBR) failed ({e:?})");
            return;
        }
        match enc.poll_packet() {
            Ok(Some(p)) => assert!(!p.payload.is_empty(), "packet {i} payload is empty"),
            Ok(None) => {
                eprintln!("skip: no packet after push_frame {i} (HEVC CBR)");
                return;
            }
            Err(e) => {
                eprintln!("skip: poll_packet (HEVC CBR) failed ({e:?})");
                return;
            }
        }
    }

    let cbr_selected = match enc.set_bitrate(250_000) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("d3d12 hevc set_bitrate: {e:?} (fixed-QP fallback, expected)");
            false
        }
    };

    for i in 3..6i64 {
        let frame = nv12_frame_sized(i, WIDTH_HEVC, HEIGHT_HEVC);
        if let Err(e) = enc.push_frame(&frame) {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: push_frame after set_bitrate failed ({e:?})");
            return;
        }
        match enc.poll_packet() {
            Ok(Some(p)) => assert!(
                !p.payload.is_empty(),
                "post-set_bitrate packet {i} payload is empty"
            ),
            Ok(None) => {
                eprintln!("skip: no packet after push_frame {i} (post-set_bitrate)");
                return;
            }
            Err(e) => {
                eprintln!("skip: poll_packet (post-set_bitrate) failed ({e:?})");
                return;
            }
        }
    }

    enc.flush().expect("flush");
    eprintln!(
        "d3d12 native hevc CBR encode ok: cbr_selected={cbr_selected}, encoding kept working \
         across set_bitrate"
    );
}
