//! Integration: `mediaway_gpu_device_create`'s factory-built device drives real
//! Zero-Copy Screen capture, pushed straight into a real H.264 encode session
//! through the capture-to-encode bridge
//! (`adr/pipeline/0005-capture-encode-bridge-c-abi.md`) — no
//! `mediaway_desktop_frame_t` ever touched by this test.
//!
//! This is the capstone proof for `mediaway-device` ADR-0007 (GPU adapter
//! enumeration + configurable device factory): before it, every Screen capture
//! caller (Rust or FFI) had to hand-roll its own `D3D11CreateDevice` call
//! (`gpu_write_frame_smoke.rs`'s `open_shared_d3d11_device`); an FFI-only caller
//! (Node/Python/C#/C++) had no way to do that at all. Skips (does not fail) when
//! no GPU/Desktop Duplication path is available.

#![cfg(all(windows, feature = "pipeline", feature = "desktop"))]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    clippy::too_many_lines,
    reason = "integration test"
)]

use std::time::{Duration, Instant};

use mediaway_container::mp4::Demuxer;
use mediaway_ffi::device::{
    GpuDeviceSessionHandle, MediawayDeviceStatus, MediawayGpuAdapterSelect,
    MediawayGpuAdapterSelectKind, MediawayGpuDeviceHandle, MediawayGpuDeviceKind,
    MediawayGpuDeviceOptions, MediawayRational as MediawayDeviceRational,
    mediaway_desktop_capture_close, mediaway_desktop_capture_config_screen,
    mediaway_desktop_capture_geometry, mediaway_desktop_capture_open, mediaway_gpu_device_close,
    mediaway_gpu_device_create, mediaway_gpu_device_handle,
};
use mediaway_ffi::pipeline::{
    MediawayPipelineCodecKind, MediawayPipelineStatus, MediawayPixelFormat, MediawayRational,
    mediaway_auto_encoder_open, mediaway_auto_video_encode_config_new,
    mediaway_encode_session_finish, mediaway_encode_session_open,
    mediaway_encode_session_write_frame_from_desktop_capture, mediaway_pipeline_ffi_buffer_free,
};

const TARGET_FRAMES: u32 = 5;
const POLL_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

#[test]
fn factory_gpu_device_drives_screen_capture_into_encode_session() {
    let options = MediawayGpuDeviceOptions {
        adapter: MediawayGpuAdapterSelect {
            kind: MediawayGpuAdapterSelectKind::Default,
            index: 0,
        },
        video_support: true,
        debug_layer: false,
    };
    let mut device: *mut GpuDeviceSessionHandle = std::ptr::null_mut();
    // SAFETY: `options` is a valid local value pointer; `device` is a valid,
    // writable local out-pointer.
    let status = unsafe { mediaway_gpu_device_create(&raw const options, &raw mut device) };
    if status != MediawayDeviceStatus::Ok {
        eprintln!("skip: mediaway_gpu_device_create failed ({status:?})");
        return;
    }

    let mut gpu_handle = MediawayGpuDeviceHandle {
        kind: MediawayGpuDeviceKind::None,
        native: 0,
        webgpu_device_id: 0,
    };
    // SAFETY: `device` was just created above and not yet closed; `gpu_handle` is
    // a valid, writable local out-pointer.
    let status = unsafe { mediaway_gpu_device_handle(device, &raw mut gpu_handle) };
    assert_eq!(
        status,
        MediawayDeviceStatus::Ok,
        "handle query must succeed"
    );

    let cap_config = mediaway_desktop_capture_config_screen(
        0,
        MediawayDeviceRational { num: 1, den: 30 },
        gpu_handle,
    );
    let mut capture = std::ptr::null_mut();
    // SAFETY: `cap_config` is a valid local value pointer; `capture` is a valid,
    // writable local out-pointer.
    let status = unsafe { mediaway_desktop_capture_open(&raw const cap_config, &raw mut capture) };
    if status != MediawayDeviceStatus::Ok {
        eprintln!("skip: screen capture open failed ({status:?}) — DDA unavailable?");
        // SAFETY: `device` is a live, not-yet-closed pointer.
        unsafe { mediaway_gpu_device_close(device) };
        return;
    }

    let mut width = 0u32;
    let mut height = 0u32;
    let status =
        unsafe { mediaway_desktop_capture_geometry(capture, &raw mut width, &raw mut height) };
    assert_eq!(status, MediawayDeviceStatus::Ok, "geometry must succeed");
    assert!(width > 0 && height > 0, "expected real screen geometry");

    let mut enc_config = mediaway_auto_video_encode_config_new(
        MediawayPipelineCodecKind::H264,
        width,
        height,
        MediawayRational { num: 1, den: 30 },
    );
    enc_config.gpu_device = gpu_handle;
    // DXGI Desktop Duplication delivers BGRA8 GPU textures — the auto config's
    // NV12 default is a CPU-encode assumption, mismatched with the captured
    // frame's actual format (same fix `gpu_write_frame_smoke.rs` needs).
    enc_config.pixel_format = MediawayPixelFormat::Bgra8;

    let mut encoder = std::ptr::null_mut();
    let status = unsafe { mediaway_auto_encoder_open(&raw const enc_config, &raw mut encoder) };
    if status != MediawayPipelineStatus::Ok || encoder.is_null() {
        eprintln!("skip: video encoder unavailable ({status:?})");
        unsafe { mediaway_desktop_capture_close(capture) };
        unsafe { mediaway_gpu_device_close(device) };
        return;
    }
    let mut session = std::ptr::null_mut();
    let status = unsafe { mediaway_encode_session_open(encoder, &raw mut session) };
    assert_eq!(
        status,
        MediawayPipelineStatus::Ok,
        "session open must succeed"
    );

    let mut written = 0u32;
    let deadline = Instant::now() + POLL_TIMEOUT;
    while written < TARGET_FRAMES && Instant::now() < deadline {
        let mut wrote = false;
        let status = unsafe {
            mediaway_encode_session_write_frame_from_desktop_capture(
                session,
                capture,
                &raw mut wrote,
            )
        };
        assert_eq!(
            status,
            MediawayPipelineStatus::Ok,
            "bridge write must succeed"
        );
        if wrote {
            written += 1;
        } else {
            std::thread::sleep(POLL_INTERVAL);
        }
    }
    unsafe { mediaway_desktop_capture_close(capture) };
    unsafe { mediaway_gpu_device_close(device) };

    if written == 0 {
        eprintln!("skip: screen capture opened but delivered no frames within {POLL_TIMEOUT:?}");
        return;
    }
    assert!(
        written == TARGET_FRAMES,
        "expected {TARGET_FRAMES} real screen frames, got {written}"
    );

    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status =
        unsafe { mediaway_encode_session_finish(session, &raw mut out_data, &raw mut out_len) };
    assert_eq!(status, MediawayPipelineStatus::Ok, "finish must succeed");
    assert!(out_len > 8, "expected non-empty fMP4 bytes");
    // SAFETY: `out_data`/`out_len` were just written by `finish` above.
    let fmp4 = unsafe { std::slice::from_raw_parts(out_data, out_len) };
    assert_eq!(&fmp4[4..8], b"ftyp", "fMP4 signature expected");

    let mut demux = Demuxer::new();
    demux.push_bytes(fmp4);
    assert_eq!(demux.streams().len(), 1, "expected exactly 1 demuxed track");
    let mut demuxed = 0usize;
    while demux.poll_packet().is_some() {
        demuxed += 1;
    }
    assert!(demuxed > 0, "expected at least one demuxed packet");

    unsafe { mediaway_pipeline_ffi_buffer_free(out_data, out_len) };
}
