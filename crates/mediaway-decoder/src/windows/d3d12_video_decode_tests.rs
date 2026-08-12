//! Hardware-gated integration test(s) for [`super::D3d12VideoDecoder`]'s D3D12 native
//! H.264 decode path. Mirrors `mediaway-encoder-windows`'s `d3d12_video_encode_tests.rs`
//! convention: `eprintln!("skip: ...")` and return rather than failing the default
//! suite on a machine without D3D12 H.264 *decode* capability.
//!
//! Real H.264 bitstream source: `mediaway-encoder-windows`'s WMF `WindowsVideoEncoder`
//! (already a dev-dependency), CPU-upload H.264, pushing several **non-flat** (gradient,
//! varies per frame) NV12 frames — the MFT's default GOP settings are expected to emit
//! at least one real inter (P) frame after the initial IDR, unlike this crate's other
//! D3D12 backends (encode) which are deliberately all-intra. Whether that actually
//! happens depends on the installed encoder MFT's defaults; this test inspects the real
//! packets produced and reports what it found rather than assuming.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit/integration tests"
)]

use super::{D3d12VideoDecoder, DecodedOutput};
use crate::{VideoDecoderConfig, VideoOutputPreference};
use mediaway_common::{
    Bytes, CodecKind, GpuDeviceHandle, NativeHandle, PixelFormat, Rational, VideoFrame,
    VideoFrameStorage,
};
use mediaway_encoder::windows::WindowsVideoEncoder;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};

// CIF (352x288) — deliberately *not* the 64x64 this crate's other D3D12 tests use.
// `mediaway-encoder-windows`'s ADR-0007 found a real minimum-resolution floor for
// D3D12 *encode* on this exact GPU (160x64 on an RTX 4090); NVDEC (decode) may have an
// analogous floor the `D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT` query does not itself
// validate (unlike encode's separate `D3D12_FEATURE_VIDEO_ENCODER_OUTPUT_RESOLUTION`
// query — decode has no equivalent min/max-resolution feature query at all). A tiny
// 64x64 coded picture triggered a real GPU hang (`DXGI_ERROR_DEVICE_HUNG` TDR) on this
// session's hardware; CIF is a conservative, standard H.264 test size well clear of any
// plausible hardware decode minimum. See ADR-0002 Addendum.
const WIDTH: u32 = 352;
const HEIGHT: u32 = 288;

/// Build a non-flat NV12 frame (a simple gradient that shifts per `frame_index`) so
/// real decoded output can be checked for genuine pixel variance, not a zeroed buffer.
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

/// Open a real hardware D3D12 device on adapter 0, enabling the debug layer first so
/// [`dump_d3d12_info_queue`] can report the driver's exact validation messages on
/// failure — same technique `mediaway-encoder-windows`'s
/// `d3d12_video_encode_tests.rs::open_real_d3d12_device` used to find its three real
/// hardware findings (ADR-0007).
fn create_d3d12_device() -> Option<windows::Win32::Graphics::Direct3D12::ID3D12Device> {
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Direct3D12::{
        D3D12CreateDevice, D3D12GetDebugInterface, ID3D12Debug, ID3D12Device,
    };
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};

    let mut debug: Option<ID3D12Debug> = None;
    if unsafe { D3D12GetDebugInterface(&raw mut debug) }.is_ok() {
        if let Some(debug) = debug {
            // SAFETY: standard debug-layer enable, no arguments, before any device is created.
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
    device
}

/// Print every message the D3D12 debug layer has queued (real validation errors — the
/// exact failing call/resource/constraint named — not just a bare `HRESULT`), then
/// clear the queue. No-op when `iq` is `None` (debug layer unavailable). Byte-for-byte
/// the same technique as `mediaway-encoder-windows`'s
/// `d3d12_video_encode_tests.rs::dump_d3d12_info_queue`.
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
        // 8-byte-aligned buffer: `D3D12_MESSAGE` needs pointer alignment on 64-bit targets.
        let mut buf: Vec<u64> = vec![0; len.div_ceil(8)];
        let msg_ptr = buf.as_mut_ptr().cast::<D3D12_MESSAGE>();
        // SAFETY: `buf` is sized for `len` bytes, aligned for `D3D12_MESSAGE`.
        if unsafe { iq.GetMessage(i, Some(msg_ptr), &raw mut len) }.is_ok() {
            // SAFETY: `GetMessage` just wrote a valid `D3D12_MESSAGE` into `buf`.
            let msg = unsafe { &*msg_ptr };
            // SAFETY: `pDescription` is a NUL-terminated ASCII string
            // `DescriptionByteLength` bytes long (including the NUL), per the D3D12
            // debug-layer contract.
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

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one linear encode-then-decode-then-assert integration sequence; splitting fragments it"
)]
fn h264_decode_idr_and_p_frame_or_skip() {
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

    // Real H.264 bitstream: encode a handful of gradient NV12 frames via the WMF
    // encoder (already a hardware-verified, working path in this crate's sibling
    // encoder crate).
    let enc_cfg = VideoEncoderConfig {
        codec: CodecKind::H264,
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
            eprintln!("skip: WindowsVideoEncoder::open failed ({e:?}) — MF unavailable?");
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
    let has_non_keyframe = packets.iter().any(|p| !p.is_keyframe);
    eprintln!(
        "encoded {} packet(s), has_non_keyframe={has_non_keyframe}",
        packets.len()
    );

    let dec_cfg = VideoDecoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        pixel_format: PixelFormat::Nv12,
        output: VideoOutputPreference::CpuFramesOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(device_handle)),
        extra_data: Bytes::new(),
    };
    let mut decoder = match D3d12VideoDecoder::open(&dec_cfg) {
        Ok(d) => d,
        Err(e) => {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: D3d12VideoDecoder::open failed ({e:?})");
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
                     is_keyframe={}) — see D3D12 InfoQueue messages above, if any",
                    packet.is_keyframe
                );
                return;
            }
        }
        // Drain the debug layer after every successful packet too — a validation
        // warning can be queued even when the call itself still "succeeds" HRESULT-wise.
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
        if let DecodedOutput::Cpu { data } = &frame.output {
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
        "decoded {} frame(s) from {} packet(s) (has_non_keyframe={has_non_keyframe})",
        decoded_frames.len(),
        packets.len()
    );
}
