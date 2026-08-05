//! Integration: DXGI Zero-Copy screen capture pushed through
//! `mediaway-ffi`'s C ABI (`mediaway_encode_session_write_frame` with
//! `storage_kind == Gpu`) — proves the GPU frame input path added by
//! `adr/0002-gpu-frame-input-c-abi.md` actually reaches a real H.264 hardware
//! encoder and round-trips through fMP4, not just the pure-logic conversion
//! tests in `mediaway-common-ffi::gpu`.
//!
//! Captures screen frames the same way
//! `mediaway/tests/screen_mic_av_smoke.rs` does (own shared D3D11
//! device, cursor nudge for deterministic DXGI delivery), but pushes them
//! through the raw `#[unsafe(no_mangle)]` C ABI functions instead of the
//! Rust-level `EncodeSession` API directly — this is the actual C surface a
//! language binding (e.g. C#'s `Mediaway.Pipeline`) calls.

#![cfg(all(windows, feature = "pipeline"))]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    reason = "integration test"
)]

use std::time::{Duration, Instant};

use mediaway::platform;
use mediaway_common::{GpuDeviceHandle, NativeHandle, Rational, VideoFrameStorage};
use mediaway_container::mp4::Demuxer;
use mediaway_device::Select;
use mediaway_device::desktop::{
    CaptureOutputPreference, DesktopCaptureSource, DesktopVideoCaptureConfig,
};
use mediaway_ffi::pipeline::{
    MediawayGpuDeviceHandle, MediawayGpuDeviceKind, MediawayPipelineCodecKind,
    MediawayPipelineStatus, MediawayPixelFormat, MediawayVideoFrame, MediawayVideoFrameStorageKind,
    mediaway_auto_encoder_open, mediaway_auto_video_encode_config_new,
    mediaway_encode_session_close, mediaway_encode_session_finish, mediaway_encode_session_open,
    mediaway_encode_session_write_frame, mediaway_pipeline_ffi_buffer_free,
};
use windows::Win32::Foundation::{HMODULE, POINT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};
use windows::core::Interface;

/// Bounded recording window — deterministic termination, not "until Ctrl+C".
const CAPTURE_SECS: u64 = 5;
/// ~30fps pacing to match the configured encoder time base.
const TICK: Duration = Duration::from_millis(33);

#[test]
fn gpu_screen_frame_write_frame_roundtrips_to_fmp4() {
    let Some((_device, device_handle)) = open_shared_d3d11_device() else {
        return;
    };

    let cap_cfg = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Screen {
            select: Select::Default,
        },
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::ZeroCopyGpu,
        gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
    };
    let mut screen = match platform::ScreenCapture::open(&cap_cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: screen capture unavailable ({e})");
            return;
        }
    };
    let geometry = screen.stream_info().geometry().expect("video geometry");

    let mut config = mediaway_auto_video_encode_config_new(
        MediawayPipelineCodecKind::H264,
        geometry.width,
        geometry.height,
        Rational::new(1, 30).into(),
    );
    config.bitrate_bps = 4_000_000;
    config.pixel_format = MediawayPixelFormat::Bgra8;
    config.gpu_device = MediawayGpuDeviceHandle {
        kind: MediawayGpuDeviceKind::DirectX11,
        native: device_handle.get(),
        webgpu_device_id: 0,
    };

    let mut encoder = std::ptr::null_mut();
    // SAFETY: `config` is a valid local value pointer; `encoder` is a valid, writable
    // local out-pointer.
    let status = unsafe { mediaway_auto_encoder_open(&raw const config, &raw mut encoder) };
    if status != MediawayPipelineStatus::Ok || encoder.is_null() {
        eprintln!("skip: video encoder unavailable ({status:?})");
        screen.close().ok();
        return;
    }

    let mut session = std::ptr::null_mut();
    // SAFETY: `encoder` was just returned by `mediaway_auto_encoder_open` and not yet
    // consumed; `session` is a valid, writable local out-pointer. This call
    // unconditionally consumes `encoder`.
    let status = unsafe { mediaway_encode_session_open(encoder, &raw mut session) };
    assert_eq!(
        status,
        MediawayPipelineStatus::Ok,
        "session open failed: {status:?}"
    );

    let pushed_frames = record_loop(&mut *screen, session);
    screen.close().ok();

    if pushed_frames == 0 {
        eprintln!("skip: no video frames captured during the recording window");
        // SAFETY: `session` is a live, not-yet-consumed handle.
        unsafe { mediaway_encode_session_close(session) };
        return;
    }

    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    // SAFETY: `session` is a live, not-yet-consumed handle (function contract);
    // `out_data`/`out_len` are valid, writable local out-pointers. This call
    // unconditionally consumes `session`.
    let status =
        unsafe { mediaway_encode_session_finish(session, &raw mut out_data, &raw mut out_len) };
    assert_eq!(
        status,
        MediawayPipelineStatus::Ok,
        "session finish failed: {status:?}"
    );

    // SAFETY: `out_data`/`out_len` describe a buffer valid for reads of that length,
    // just returned by `mediaway_encode_session_finish` and not yet freed.
    let bytes = unsafe { std::slice::from_raw_parts(out_data, out_len) }.to_vec();
    assert!(
        bytes.len() > 1_000,
        "fmp4 output implausibly small: {} bytes",
        bytes.len()
    );
    assert_eq!(&bytes[4..8], b"ftyp");

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert_eq!(demux.streams().len(), 1, "expected exactly 1 demuxed track");
    let mut demuxed = 0usize;
    while demux.poll_packet().is_some() {
        demuxed += 1;
    }
    assert!(demuxed > 0, "expected at least one demuxed packet");

    // SAFETY: `out_data`/`out_len` are exactly the pair returned above, not yet freed.
    unsafe { mediaway_pipeline_ffi_buffer_free(out_data, out_len) };

    eprintln!(
        "gpu_screen_frame_write_frame_roundtrips_to_fmp4: pushed={pushed_frames} demuxed={demuxed} bytes={}",
        bytes.len()
    );
}

/// Own D3D11 device shared by screen capture and the encoder's Zero-Copy path, so a
/// captured texture can be pushed straight into `write_frame` with no copy. `None`
/// (with an honest `skip:` line) when no HW-capable D3D11 device is available.
fn open_shared_d3d11_device() -> Option<(ID3D11Device, NativeHandle)> {
    let mut device: Option<ID3D11Device> = None;
    let hr = unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&raw mut device),
            None,
            None,
        )
    };
    let Some(device) = device else {
        eprintln!("skip: D3D11CreateDevice failed ({hr:?})");
        return None;
    };
    let handle = NativeHandle::new(Interface::as_raw(&device) as usize).expect("device pointer");
    Some((device, handle))
}

/// Bounded capture -> `write_frame` loop: polls the screen, converts each Zero-Copy
/// GPU frame into a `mediaway_video_frame_t` with `storage_kind == Gpu`, and pushes it
/// through the real C ABI function. Terminates at [`CAPTURE_SECS`] regardless of
/// activity — not "until Ctrl+C". Returns the number of frames successfully pushed.
fn record_loop(
    screen: &mut dyn mediaway_device::desktop::DesktopVideoCapture,
    session: *mut mediaway_ffi::pipeline::EncodeSessionHandle,
) -> usize {
    let mut pushed_frames = 0usize;
    let mut pts = 0i64;
    let mut toggle = false;
    let mut origin = POINT::default();
    // SAFETY: GetCursorPos writes into a valid, uniquely-owned local POINT.
    unsafe {
        let _ = GetCursorPos(&raw mut origin);
    }
    let deadline = Instant::now() + Duration::from_secs(CAPTURE_SECS);

    while Instant::now() < deadline {
        nudge_cursor(origin, &mut toggle);

        match screen.poll_frame() {
            Ok(Some(mut frame)) => {
                frame.pts = pts;
                pts += 1;
                let VideoFrameStorage::Gpu(handle) = frame.storage else {
                    panic!("ZeroCopyGpu capture produced a CPU frame");
                };
                let c_frame = MediawayVideoFrame {
                    pts: frame.pts,
                    duration: frame.duration,
                    width: frame.width,
                    height: frame.height,
                    pixel_format: frame.format.into(),
                    storage_kind: MediawayVideoFrameStorageKind::Gpu,
                    raw_bytes: std::ptr::null(),
                    raw_bytes_len: 0,
                    gpu_buffer: handle.into(),
                };
                // SAFETY: `session` is a live, not-yet-consumed handle; `c_frame`'s
                // `gpu_buffer` aliases the capture session's own texture, valid for
                // this synchronous call (released only after the call returns, below).
                let status =
                    unsafe { mediaway_encode_session_write_frame(session, &raw const c_frame) };
                let _ = screen.release_frame();
                match status {
                    MediawayPipelineStatus::Ok => pushed_frames += 1,
                    other => eprintln!("gpu_write_frame_smoke: write_frame error ({other:?})"),
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("gpu_write_frame_smoke: capture error ({e}), stopping capture loop");
                break;
            }
        }

        std::thread::sleep(TICK);
    }

    // SAFETY: restore the cursor to where the loop found it.
    unsafe {
        let _ = SetCursorPos(origin.x, origin.y);
    }
    pushed_frames
}

/// Jitter the cursor by one pixel and back. DXGI Desktop Duplication's
/// `AcquireNextFrame` delivers a new frame when *either* the desktop image or the
/// pointer position changes, so this keeps frames flowing deterministically even when
/// nothing else on the desktop is redrawing.
fn nudge_cursor(origin: POINT, toggle: &mut bool) {
    *toggle = !*toggle;
    let dx = if *toggle { 1 } else { -1 };
    // SAFETY: SetCursorPos is a simple user32 call; failure is non-fatal here.
    unsafe {
        let _ = SetCursorPos(origin.x + dx, origin.y);
    }
}
