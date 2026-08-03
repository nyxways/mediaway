#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::*;
use crate::desktop::{
    CaptureOutputPreference, DesktopAudioCapture, DesktopAudioCaptureConfig, DesktopCaptureSource,
    DesktopVideoCapture, DesktopVideoCaptureConfig, capture_desktop_video_once,
};
use crate::{CaptureError, Select};
use mediaway_common::{GpuDeviceHandle, NativeHandle, Rational};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::core::Interface;

#[test]
fn open_screen_zero_copy_poll_release_or_skip() {
    let _guard = crate::windows_desktop::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        return;
    };
    let device_handle =
        NativeHandle::new(Interface::as_raw(&device) as usize).expect("device pointer");
    let cfg = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Screen {
            select: Select::Default,
        },
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::ZeroCopyGpu,
        gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
    };
    let mut cap = match WindowsScreenCapture::open(&cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: WindowsScreenCapture::open failed ({e:?}) — DDA unavailable?");
            return;
        }
    };
    let geometry = cap
        .stream_info()
        .geometry()
        .expect("video stream has geometry");
    assert!(geometry.width > 0);
    assert!(geometry.height > 0);
    match cap.poll_frame() {
        Ok(Some(_frame)) => {
            cap.release_frame().expect("release");
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("skip: poll_frame failed ({e:?})");
            return;
        }
    }
    cap.close().expect("close");
}

/// `mediaway-device-desktop` ADR-0006's facade-level convenience, hardware-verified
/// against the real DXGI backend. `capture_desktop_video_once` closes the session
/// before returning — for Screen's GPU-backed storage that can free the underlying
/// texture out from under the caller for a solo/last consumer (the dangling-handle bug
/// fixed alongside `mediaway-device-ffi/adr/0003-gpu-handle-c-abi.md`), so it now
/// refuses with `CaptureError::Unsupported` instead of ever handing back a frame.
#[test]
fn capture_video_once_screen_is_unsupported_for_gpu_storage_or_skip() {
    let _guard = crate::windows_desktop::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        return;
    };
    let device_handle =
        NativeHandle::new(Interface::as_raw(&device) as usize).expect("device pointer");
    let cfg = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Screen {
            select: Select::Default,
        },
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::ZeroCopyGpu,
        gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
    };
    let mut dangling_frame_size = None;
    match capture_desktop_video_once(
        || WindowsScreenCapture::open(&cfg),
        std::time::Duration::from_millis(500),
    ) {
        Ok(frame) => dangling_frame_size = Some((frame.width, frame.height)),
        Err(CaptureError::Unsupported) => {}
        Err(e) => eprintln!("skip: capture_desktop_video_once failed ({e:?}) — DDA unavailable?"),
    }
    assert!(
        dangling_frame_size.is_none(),
        "capture_desktop_video_once must never return a GPU-backed frame after the \
         dangling-handle fix, but got one: {dangling_frame_size:?}"
    );
}

/// Process-loopback capture via [`WindowsDesktopAudioCapture`] — the same shared
/// WASAPI engine `mediaway-device-windows-audio`'s own microphone test exercises,
/// wrapped for this crate's `DesktopAudioCapture` surface.
#[test]
fn open_desktop_audio_process_loopback_or_skip() {
    let _guard = crate::windows_desktop::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let cfg = DesktopAudioCaptureConfig::process_loopback(
        std::process::id(),
        crate::desktop::ProcessTreeScope::IncludeChildren,
        Rational::new(1, 48_000),
    );
    let mut cap = match WindowsDesktopAudioCapture::open(&cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: process loopback open ({e:?})");
            return;
        }
    };
    assert_eq!(cap.stream_info().sample_rate(), Some(48_000));
    assert_eq!(cap.stream_info().channels(), Some(2));
    std::thread::sleep(std::time::Duration::from_millis(50));
    match cap.poll_frame() {
        Ok(Some(frame)) => eprintln!("process loopback frame bytes={}", frame.data.len()),
        Ok(None) => eprintln!("process loopback: no frame yet (ok)"),
        Err(e) => eprintln!("skip: process loopback poll ({e:?})"),
    }
    cap.close().expect("close");
}

#[test]
fn exclude_window_from_capture_rejects_null() {
    assert!(matches!(
        exclude_window_from_capture(0),
        Err(crate::CaptureError::InvalidInput)
    ));
}

#[test]
fn null_window_handle_is_unrepresentable() {
    // A null HWND can no longer be constructed as `DesktopCaptureSource::Window` at
    // all — `NativeHandle::new(0)` returns `None`, so the "reject null hwnd" check
    // that used to run inside `WindowsWindowCapture::open` is now a compile-time
    // impossibility instead of a runtime error path.
    assert!(NativeHandle::new(0).is_none());
}

#[test]
fn open_window_capture_foreground_or_skip() {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let _guard = crate::windows_desktop::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        return;
    };
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        eprintln!("skip: no foreground window");
        return;
    }
    let window_handle = NativeHandle::new(hwnd.0 as usize).expect("foreground window handle");
    let mut cfg = DesktopVideoCaptureConfig::window(window_handle, Rational::new(1, 30));
    let device_handle =
        NativeHandle::new(Interface::as_raw(&device) as usize).expect("device pointer");
    cfg.gpu_device = Some(GpuDeviceHandle::DirectX11(device_handle));
    let mut cap = match WindowsWindowCapture::open(&cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: WindowsWindowCapture::open ({e:?})");
            return;
        }
    };
    let geometry = cap
        .stream_info()
        .geometry()
        .expect("video stream has geometry");
    let (info_w, info_h) = (geometry.width, geometry.height);
    assert!(info_w > 0 && info_h > 0);
    std::thread::sleep(std::time::Duration::from_millis(50));
    match cap.poll_frame() {
        Ok(Some(_)) => {
            cap.release_frame().expect("release");
            eprintln!("wgc frame ok {info_w}x{info_h}");
        }
        Ok(None) => eprintln!("wgc: no frame yet (ok)"),
        Err(e) => eprintln!("skip: wgc poll ({e:?})"),
    }
    cap.close().expect("close");
}
