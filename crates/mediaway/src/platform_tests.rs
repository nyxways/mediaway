#![cfg(test)]
#![allow(clippy::expect_used, reason = "unit tests")]

// Each test imports what it uses: the tests are `#[cfg]`-gated per target, and a shared
// import list would be unused on some of them (and `-D warnings` on CI's Linux job).

/// Reaches the window-capture backend without opening a window: WGC checks its device
/// before it touches the `HWND`, so an unset device comes back `InvalidInput`. Had the
/// facade dispatched to *screen* capture instead, a window source would be `Unsupported` —
/// which is what makes this a dispatch test rather than a smoke test.
#[cfg(windows)]
#[test]
fn window_capture_dispatches_to_wgc() {
    use super::WindowCapture;
    use mediaway_common::{NativeHandle, Rational};
    use mediaway_device::CaptureError;
    use mediaway_device::desktop::DesktopVideoCaptureConfig;

    let window = NativeHandle::new(1).expect("non-zero placeholder");
    let config = DesktopVideoCaptureConfig::window(window, Rational::new(1, 30));
    assert!(matches!(
        WindowCapture::open(&config),
        Err(CaptureError::InvalidInput)
    ));
}

/// Opens a real per-process loopback session on this process — an audio client, nothing
/// on screen. The binding's type is the point: `DesktopAudio::open` hands back the concrete
/// backend, not a box.
#[cfg(windows)]
#[test]
fn desktop_audio_opens_process_loopback_as_the_concrete_backend() {
    use super::DesktopAudio;
    use mediaway_common::Rational;
    use mediaway_device::desktop::{DesktopAudioCaptureConfig, ProcessTreeScope};
    use mediaway_device::windows_desktop::WindowsDesktopAudioCapture;

    let config = DesktopAudioCaptureConfig::process_loopback(
        std::process::id(),
        ProcessTreeScope::IncludeChildren,
        Rational::new(1, 48_000),
    );
    let capture: WindowsDesktopAudioCapture =
        DesktopAudio::open(&config).expect("process loopback opens on Windows 10 2004+");
    drop(capture);
}

#[cfg(not(windows))]
#[test]
fn desktop_audio_has_no_backend_off_windows() {
    use super::DesktopAudio;
    use mediaway_common::Rational;
    use mediaway_device::CaptureError;
    use mediaway_device::desktop::DesktopAudioCaptureConfig;

    let config = DesktopAudioCaptureConfig::loopback(Rational::new(1, 48_000));
    assert!(matches!(
        DesktopAudio::open(&config),
        Err(CaptureError::NoBackend)
    ));
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
#[test]
fn window_capture_has_no_backend_elsewhere() {
    use super::WindowCapture;
    use mediaway_common::{NativeHandle, Rational};
    use mediaway_device::CaptureError;
    use mediaway_device::desktop::DesktopVideoCaptureConfig;

    let window = NativeHandle::new(1).expect("non-zero placeholder");
    let config = DesktopVideoCaptureConfig::window(window, Rational::new(1, 30));
    assert!(matches!(
        WindowCapture::open(&config),
        Err(CaptureError::NoBackend)
    ));
}
