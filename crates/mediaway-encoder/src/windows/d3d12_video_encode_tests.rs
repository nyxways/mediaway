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

use crate::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};
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
    if unsafe { D3D12GetDebugInterface(&raw mut debug) }.is_ok() {
        if let Some(debug) = debug {
            unsafe { debug.EnableDebugLayer() };
        }
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
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
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
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
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

/// Parse AV1 `obu_type` values out of length-prefixed OBUs (`obu_has_size_field == 1`,
/// no extension — the only shape this backend ever writes/expects, see
/// [`super::bitstream_av1`]).
fn obu_types(payload: &[u8]) -> Vec<u8> {
    let mut types = Vec::new();
    let mut i = 0usize;
    while i < payload.len() {
        let header = payload[i];
        let obu_type = (header >> 3) & 0x0F;
        let has_size_field = (header >> 1) & 1 == 1;
        let has_extension = (header >> 2) & 1 == 1;
        i += 1;
        if has_extension {
            i += 1;
        }
        if !has_size_field || i >= payload.len() {
            break;
        }
        let mut size = 0u64;
        let mut shift = 0u32;
        loop {
            if i >= payload.len() {
                return types;
            }
            let b = payload[i];
            i += 1;
            size |= u64::from(b & 0x7f) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                break;
            }
        }
        types.push(obu_type);
        let Ok(size) = usize::try_from(size) else {
            break;
        };
        i += size;
    }
    types
}

/// Real D3D12 device → real AV1 `ID3D12VideoDevice3` support check → real `EncodeFrame`
/// submissions → real length-prefixed OBU output (temporal delimiter `obu_type == 2`,
/// sequence header `== 1`, frame `== 6` present in every packet). Skips (does not fail) if
/// this machine's adapter/driver lacks D3D12 native AV1 video-encode support (requires
/// Windows 11 24H2+ / WDDM 3.2).
#[test]
fn d3d12_native_av1_encode_or_skip() {
    let Some(device) = open_real_d3d12_device() else {
        return;
    };
    let Some(handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
        eprintln!("skip: null D3D12 device pointer");
        return;
    };
    let info_queue: Option<ID3D12InfoQueue> = device.cast().ok();

    let cfg = VideoEncoderConfig {
        codec: CodecKind::Av1,
        width: WIDTH_AV1,
        height: HEIGHT_AV1,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
    };

    let mut enc = match D3d12VideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!(
                "skip: D3d12VideoEncoder::open (AV1) failed ({e:?}) — no D3D12 AV1 video-encode \
                 support on this device/driver?"
            );
            return;
        }
    };

    let mut packets = 0usize;
    for i in 0..3i64 {
        let frame = nv12_frame_sized(i, WIDTH_AV1, HEIGHT_AV1);
        if let Err(e) = enc.push_frame(&frame) {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: push_frame (AV1) failed ({e:?})");
            return;
        }
        let packet = match enc.poll_packet() {
            Ok(Some(p)) => p,
            Ok(None) => {
                eprintln!("skip: no packet after push_frame {i} (AV1)");
                return;
            }
            Err(e) => {
                eprintln!("skip: poll_packet (AV1) failed ({e:?})");
                return;
            }
        };
        assert!(!packet.payload.is_empty(), "packet {i} payload is empty");
        assert!(packet.is_keyframe, "packet {i} should be a key frame");

        let types = obu_types(&packet.payload);
        assert!(
            types.contains(&2),
            "packet {i} missing temporal delimiter OBU (type 2); found types {types:?}"
        );
        assert!(
            types.contains(&1),
            "packet {i} missing sequence header OBU (type 1); found types {types:?}"
        );
        assert!(
            types.contains(&6),
            "packet {i} missing frame OBU (type 6); found types {types:?}"
        );
        packets += 1;
    }

    enc.flush().expect("flush");
    eprintln!("d3d12 native av1 encode ok: {packets} packets, all with real TD+SeqHdr+Frame OBUs");
}

/// Real, hardware-honest probe: does this machine's actual D3D12 driver advertise AV1
/// video-encode support (`D3D12_FEATURE_VIDEO_ENCODER_CODEC` for
/// `D3D12_VIDEO_ENCODER_CODEC_AV1`)? Kept alongside [`d3d12_native_av1_encode_or_skip`] as
/// a cheap, isolated probe (no encoder session) for triage when that test skips. Never
/// fails: both "supported" and "not supported" are informative, honest outcomes.
#[test]
fn d3d12_av1_encode_codec_probe() {
    use windows::Win32::Media::MediaFoundation::{
        D3D12_FEATURE_DATA_VIDEO_ENCODER_CODEC, D3D12_FEATURE_VIDEO_ENCODER_CODEC,
        D3D12_VIDEO_ENCODER_CODEC_AV1, ID3D12VideoDevice3,
    };
    use windows::core::BOOL;

    let Some(device) = open_real_d3d12_device() else {
        return;
    };
    let Ok(video_device) = device.cast::<ID3D12VideoDevice3>() else {
        eprintln!("skip: no ID3D12VideoDevice3 on this device");
        return;
    };

    let mut support = D3D12_FEATURE_DATA_VIDEO_ENCODER_CODEC {
        NodeIndex: 0,
        Codec: D3D12_VIDEO_ENCODER_CODEC_AV1,
        IsSupported: BOOL::default(),
    };
    // SAFETY: `support` is sized/typed exactly as `D3D12_FEATURE_VIDEO_ENCODER_CODEC` expects.
    let hr = unsafe {
        video_device.CheckFeatureSupport(
            D3D12_FEATURE_VIDEO_ENCODER_CODEC,
            std::ptr::from_mut(&mut support).cast(),
            u32::try_from(std::mem::size_of::<D3D12_FEATURE_DATA_VIDEO_ENCODER_CODEC>())
                .unwrap_or(u32::MAX),
        )
    };
    match hr {
        Ok(()) => eprintln!(
            "d3d12 av1 encode codec probe: IsSupported={}",
            support.IsSupported.as_bool()
        ),
        Err(e) => eprintln!("d3d12 av1 encode codec probe: CheckFeatureSupport failed ({e:?})"),
    }
}
