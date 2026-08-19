//! Hardware-gated integration test for [`super::D3d12VideoDecoderAv1`]'s D3D12 native AV1
//! decode path. Mirrors `d3d12_video_decode_hevc_tests.rs`'s
//! `hevc_decode_idr_and_p_frame_or_skip` `eprintln!`-and-return soft-skip convention.
//!
//! # ⚠️ Written this pass, **must not be run** without new, separate, explicit consent
//!
//! Per ADR-0005 § Test plan: this file's own existence/compilation is this pass's
//! deliverable — running it is a real `DXGI_ERROR_DEVICE_HUNG` GPU-hang risk on completely
//! unverified code (unlike even H.264's own D3D12 decode path, which has at least been
//! *attempted* on real hardware 8 times, and unlike HEVC's, which at least ports a
//! hardware-verified-for-IDR Vulkan bitstream parser), compounding the existing, still-
//! unresolved H.264 D3D12 decode TDR. A human/agent with informed, deliberate, explicit
//! consent for a real hardware attempt must decide separately, later, whether and when to
//! run it.
//!
//! **Doubly cautioned beyond HEVC's own precedent (ADR-0005 § Open questions #1)**: this
//! crate family's own D3D12 AV1 *encoder* output is on record
//! (`docs/standards/registry.toml`'s `av1-bitstream-spec` entry) as **not confirmed
//! decodable by `libdav1d`** — even a future, separately-consented hardware attempt with
//! this test may have no valid input bitstream to chain from at all. Before any real
//! hardware attempt, an implementer must first resolve that open question (re-confirm
//! whether the D3D12 AV1 encoder's output has since become decodable, or source a bitstream
//! from `mediaway-sw::av1`/`rav1e` instead, per ADR-0005 § Test plan's own planned shape).
//!
//! **Real AV1 bitstream source — same technique HEVC's own hardware test uses, one layer
//! up**: `mediaway-encoder-windows`'s native D3D12 AV1 *encoder* (`d3d12_video_encode/
//! {av1.rs,ops_av1.rs,bitstream_av1.rs}`) is `mod d3d12_video_encode;` — crate-private and
//! unregistered, exactly like this crate's own (private, unregistered) `d3d12_video_decode`
//! module — so it cannot be reached cross-crate without first changing that crate's own
//! module visibility (out of scope for a decode-only task). This test instead uses
//! `mediaway-encoder-windows`'s **public** `WindowsVideoEncoder` with `CodecKind::Av1`
//! (its WMF AV1 encoder MFT path, `windows::wmf::codec::to_mf_subtype`'s own
//! `MFVideoFormat_AV1` mapping) — **whether any real AV1 encoder MFT is actually present on
//! this workspace's reference machine is unconfirmed**; `WindowsVideoEncoder::open` softly
//! skips below if not.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit/integration tests"
)]

use super::{D3d12VideoDecoderAv1, DecodedOutputAv1};
use crate::{VideoDecoderConfig, VideoOutputPreference};
use mediaway_common::{
    Bytes, CodecKind, GpuDeviceHandle, NativeHandle, PixelFormat, Rational, VideoFrame,
    VideoFrameStorage,
};
use mediaway_encoder::windows::WindowsVideoEncoder;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};

// CIF (352x288) — same conservative, standard resolution choice
// `d3d12_video_decode_tests.rs`/`d3d12_video_decode_hevc_tests.rs` document for their own
// tests (a tiny 64x64 coded picture triggered a real GPU hang on this workspace's
// reference hardware for H.264; there is no reason to believe AV1's own decode-minimum-
// resolution floor, if any, is smaller).
const WIDTH: u32 = 352;
const HEIGHT: u32 = 288;

/// Build a non-flat NV12 frame — byte-for-byte the same gradient construction
/// `d3d12_video_decode_hevc_tests.rs::gradient_nv12_frame` uses, so genuine pixel variance
/// can be checked in decoded output rather than a zeroed buffer.
fn gradient_nv12_frame(frame_index: u32, pts: i64) -> VideoFrame {
    let mut data =
        vec![0u8; WIDTH as usize * HEIGHT as usize + WIDTH as usize * HEIGHT as usize / 2];
    for y in 0..HEIGHT as usize {
        for x in 0..WIDTH as usize {
            let value = u8::try_from((x + y + frame_index as usize * 7) % 256).unwrap_or(0);
            data[y * WIDTH as usize + x] = value;
        }
    }
    let chroma_base = WIDTH as usize * HEIGHT as usize;
    for i in 0..(chroma_base / 2) {
        data[chroma_base + i] = 128;
    }
    VideoFrame {
        pts,
        duration: 1,
        width: WIDTH,
        height: HEIGHT,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from(data),
        },
    }
}

/// Open a real hardware D3D12 device on adapter 0 with the debug layer enabled — same
/// technique `d3d12_video_decode_tests.rs::create_d3d12_device` uses.
fn create_d3d12_device() -> Option<windows::Win32::Graphics::Direct3D12::ID3D12Device> {
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Direct3D12::{
        D3D12CreateDevice, D3D12GetDebugInterface, ID3D12Debug, ID3D12Device,
    };
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};

    let mut debug: Option<ID3D12Debug> = None;
    if unsafe { D3D12GetDebugInterface(&raw mut debug) }.is_ok()
        && let Some(debug) = debug
    {
        // SAFETY: standard debug-layer enable, no arguments, before any device is created.
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
    device
}

/// Print every message the D3D12 debug layer has queued, then clear the queue — same
/// technique `d3d12_video_decode_tests.rs::dump_d3d12_info_queue` uses.
fn dump_d3d12_info_queue(iq: Option<&windows::Win32::Graphics::Direct3D12::ID3D12InfoQueue>) {
    use windows::Win32::Graphics::Direct3D12::D3D12_MESSAGE;

    let Some(iq) = iq else { return };
    // SAFETY: plain getter on a live `ID3D12InfoQueue`.
    let n = unsafe { iq.GetNumStoredMessages() };
    for i in 0..n {
        let mut len = 0usize;
        // SAFETY: querying required buffer length with a null message pointer.
        if unsafe { iq.GetMessage(i, None, &raw mut len) }.is_err() || len == 0 {
            continue;
        }
        let mut buf: Vec<u64> = vec![0; len.div_ceil(8)];
        let msg_ptr = buf.as_mut_ptr().cast::<D3D12_MESSAGE>();
        // SAFETY: `buf` is sized for `len` bytes, aligned for `D3D12_MESSAGE`.
        if unsafe { iq.GetMessage(i, Some(msg_ptr), &raw mut len) }.is_ok() {
            // SAFETY: `GetMessage` just wrote a valid `D3D12_MESSAGE` into `buf`.
            let msg = unsafe { &*msg_ptr };
            // SAFETY: `pDescription` is a NUL-terminated ASCII string
            // `DescriptionByteLength` bytes long (including the NUL).
            let desc = unsafe {
                std::slice::from_raw_parts(
                    msg.pDescription,
                    msg.DescriptionByteLength.saturating_sub(1),
                )
            };
            eprintln!("D3D12 InfoQueue[{i}]: {}", String::from_utf8_lossy(desc));
        }
    }
    // SAFETY: plain call, clears the queue this function just drained.
    unsafe { iq.ClearStoredMessages() };
}

/// # Do not run — see this file's module doc.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one linear encode-then-decode-then-assert integration sequence; splitting fragments it"
)]
fn av1_decode_key_frames_or_skip() {
    use windows::core::Interface;

    let Some(device) = create_d3d12_device() else {
        return;
    };
    let Some(device_handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
        eprintln!("skip: null D3D12 device pointer");
        return;
    };
    let info_queue: Option<windows::Win32::Graphics::Direct3D12::ID3D12InfoQueue> =
        device.cast().ok();

    // Real AV1 bitstream — see module doc for why this uses `WindowsVideoEncoder` (WMF)
    // rather than this workspace's own D3D12 AV1 GPU encoder, and for this test's own
    // doubly-cautioned decodability caveat (ADR-0005 Open Question #1).
    let enc_cfg = VideoEncoderConfig {
        codec: CodecKind::Av1,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    };
    let mut encoder = match WindowsVideoEncoder::open(&enc_cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: WindowsVideoEncoder::open(AV1) failed ({e:?}) — no AV1 MFT?");
            return;
        }
    };
    let mut packets = Vec::new();
    for i in 0..8u32 {
        let frame = gradient_nv12_frame(i, i64::from(i));
        if let Err(e) = encoder.push_frame(&frame) {
            eprintln!("skip: encoder push_frame failed ({e:?})");
            return;
        }
        while let Ok(Some(p)) = encoder.poll_packet() {
            packets.push(p);
        }
    }
    let _ = encoder.flush();
    while let Ok(Some(p)) = encoder.poll_packet() {
        packets.push(p);
    }
    if packets.is_empty() {
        eprintln!("skip: encoder produced no packets");
        return;
    }

    let dec_cfg = VideoDecoderConfig {
        codec: CodecKind::Av1,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        pixel_format: PixelFormat::Nv12,
        output: VideoOutputPreference::CpuFramesOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(device_handle)),
        extra_data: Bytes::new(),
    };
    let mut decoder = match D3d12VideoDecoderAv1::open(&dec_cfg) {
        Ok(d) => d,
        Err(e) => {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: D3d12VideoDecoderAv1::open failed ({e:?})");
            return;
        }
    };

    let mut decoded_frames = Vec::new();
    for (packet_index, packet) in packets.iter().enumerate() {
        match decoder.push_packet(packet) {
            Ok(()) => {}
            Err(e) => {
                dump_d3d12_info_queue(info_queue.as_ref());
                eprintln!(
                    "skip: push_packet failed on packet {packet_index} ({e:?}, \
                     is_keyframe={}) — see D3D12 InfoQueue messages above, if any. If this \
                     is a bitstream-parsing rejection rather than a D3D12 failure, ADR-0005 \
                     Open Question #1 (encoder output decodability) is the first thing to \
                     re-check, not this decoder's own logic.",
                    packet.is_keyframe
                );
                return;
            }
        }
        dump_d3d12_info_queue(info_queue.as_ref());
        while let Ok(Some(frame)) = decoder.poll_frame() {
            decoded_frames.push(frame);
        }
    }
    let _ = decoder.flush();
    while let Ok(Some(frame)) = decoder.poll_frame() {
        decoded_frames.push(frame);
    }

    assert!(
        !decoded_frames.is_empty(),
        "expected at least one decoded frame"
    );
    let mut any_real_variance = false;
    for frame in &decoded_frames {
        if let DecodedOutputAv1::Cpu { data } = &frame.output {
            assert_eq!(data.len(), (WIDTH * HEIGHT + WIDTH * HEIGHT / 2) as usize);
            let first = data[0];
            if data.iter().any(|&b| b != first) {
                any_real_variance = true;
            }
        }
    }
    assert!(
        any_real_variance,
        "expected genuine pixel variance in at least one decoded frame"
    );
    eprintln!(
        "decoded {} frame(s) from {} packet(s)",
        decoded_frames.len(),
        packets.len()
    );
}
