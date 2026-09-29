//! Real-hardware proof for `adr/device/0005-window-capture-c-abi.md`: a real win32 window, a
//! real D3D11 device from `mediaway_gpu_device_create`, and a real WGC session opened through
//! the raw `#[unsafe(no_mangle)]` C ABI — the surface a language binding calls.
//!
//! `#[ignore]`d because it calls `ShowWindow`, which puts a real window on the desktop the suite
//! runs on (`docs/conventions/testing.md` § Tests that manipulate the desktop). Run explicitly:
//!
//! ```text
//! cargo nextest run -p mediaway-ffi --run-ignored all -E 'test(window_capture_smoke)'
//! ```

#![cfg(windows)]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    reason = "integration test"
)]

use mediaway_ffi::device::{
    DesktopCaptureHandle, GpuDeviceSessionHandle, MediawayCaptureBorder, MediawayCaptureCursor,
    MediawayDesktopCaptureConfig, MediawayDesktopFrame, MediawayDeviceStatus,
    MediawayFrameDimensions, MediawayGpuAdapterSelect, MediawayGpuAdapterSelectKind,
    MediawayGpuDeviceHandle, MediawayGpuDeviceKind, MediawayGpuDeviceOptions, MediawayRational,
    MediawayVideoFrameStorageKind, mediaway_desktop_capture_border_hidden,
    mediaway_desktop_capture_close, mediaway_desktop_capture_config_window,
    mediaway_desktop_capture_geometry, mediaway_desktop_capture_open,
    mediaway_desktop_capture_poll_frame_blocking, mediaway_desktop_capture_release_frame,
    mediaway_desktop_frame_free, mediaway_gpu_device_close, mediaway_gpu_device_create,
    mediaway_gpu_device_handle,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SW_SHOWNORMAL,
    ShowWindow, UnregisterClassW, WINDOW_EX_STYLE, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};
use windows::core::PCWSTR;

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: forwards to the default window procedure, as any minimal win32 window does.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// A real, visible top-level window: WGC cannot capture a nonexistent or minimized one.
struct TestWindow {
    hwnd: HWND,
    class_name: Vec<u16>,
}

impl TestWindow {
    fn create(width: i32, height: i32) -> Option<Self> {
        let class_name: Vec<u16> = "MediawayFfiWindowCaptureSmoke\0".encode_utf16().collect();
        let instance = unsafe { GetModuleHandleW(None) }.ok()?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        // SAFETY: `class_name` and `class` are live for the duration of this call.
        if unsafe { RegisterClassW(&raw const class) } == 0 {
            return None;
        }
        let title: Vec<u16> = "mediaway ffi window capture\0".encode_utf16().collect();
        // SAFETY: a registered class name, no parent or menu — a plain top-level window.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width,
                height,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .ok()?;
        if hwnd.is_invalid() {
            return None;
        }
        // SAFETY: `hwnd` is the live window created above.
        let _ = unsafe { ShowWindow(hwnd, SW_SHOWNORMAL) };
        Some(Self { hwnd, class_name })
    }

    fn bits(&self) -> u64 {
        self.hwnd.0 as usize as u64
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: both were created by this struct's `create`.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = UnregisterClassW(PCWSTR(self.class_name.as_ptr()), None);
        }
    }
}

fn create_device() -> (*mut GpuDeviceSessionHandle, MediawayGpuDeviceHandle) {
    let options = MediawayGpuDeviceOptions {
        adapter: MediawayGpuAdapterSelect {
            kind: MediawayGpuAdapterSelectKind::Default,
            index: 0,
        },
        video_support: true,
        debug_layer: false,
    };
    let mut device: *mut GpuDeviceSessionHandle = std::ptr::null_mut();
    let status = unsafe { mediaway_gpu_device_create(&raw const options, &raw mut device) };
    assert_eq!(status, MediawayDeviceStatus::Ok, "GPU device create");
    let mut handle = MediawayGpuDeviceHandle {
        kind: MediawayGpuDeviceKind::None,
        native: 0,
        webgpu_device_id: 0,
    };
    let status = unsafe { mediaway_gpu_device_handle(device, &raw mut handle) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
    (device, handle)
}

fn open(
    config: &MediawayDesktopCaptureConfig,
) -> (MediawayDeviceStatus, *mut DesktopCaptureHandle) {
    let mut out: *mut DesktopCaptureHandle = std::ptr::null_mut();
    let status = unsafe { mediaway_desktop_capture_open(config, &raw mut out) };
    (status, out)
}

fn geometry(capture: *mut DesktopCaptureHandle) -> (u32, u32) {
    let (mut w, mut h) = (0, 0);
    let status = unsafe { mediaway_desktop_capture_geometry(capture, &raw mut w, &raw mut h) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
    (w, h)
}

/// Polls one frame, asserts it is a GPU frame of `expected` size, and releases it.
fn assert_frame(capture: *mut DesktopCaptureHandle, expected: (u32, u32)) {
    let mut frame: MediawayDesktopFrame = unsafe { std::mem::zeroed() };
    let status =
        unsafe { mediaway_desktop_capture_poll_frame_blocking(capture, 3000, &raw mut frame) };
    assert_eq!(status, MediawayDeviceStatus::Ok, "no frame within 3 s");
    assert_eq!(frame.storage_kind, MediawayVideoFrameStorageKind::Gpu);
    assert_eq!((frame.width, frame.height), expected);
    unsafe {
        assert_eq!(
            mediaway_desktop_capture_release_frame(capture),
            MediawayDeviceStatus::Ok
        );
        mediaway_desktop_frame_free(&raw mut frame);
    }
}

#[test]
#[ignore = "opens a visible window on the desktop; run explicitly with --run-ignored all"]
fn window_capture_smoke_options_reach_a_real_wgc_session() {
    // 335x250 makes WGC's capture size odd on both axes (it excludes the invisible resize
    // borders: 14 px narrower, 7 px shorter at 100% scale on Windows 11), so even-cropping is
    // observable. The assertion below checks that instead of trusting the arithmetic.
    let window = TestWindow::create(335, 250).expect("a real test window");
    let (device, gpu) = create_device();
    let time_base = MediawayRational { num: 1, den: 30 };

    // Native size, from a plain session.
    let plain = mediaway_desktop_capture_config_window(window.bits(), time_base, gpu);
    let (status, native_capture) = open(&plain);
    assert_eq!(status, MediawayDeviceStatus::Ok, "plain window open");
    let native = geometry(native_capture);
    assert!(
        native.0 % 2 == 1 || native.1 % 2 == 1,
        "the test window captured at {native:?}, even on both axes, so this proves nothing about \
         cropping; change the window size in this test"
    );
    // A Shown border is never "hidden".
    let mut hidden = true;
    let status = unsafe { mediaway_desktop_capture_border_hidden(native_capture, &raw mut hidden) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
    assert!(!hidden, "border = SHOWN must report not hidden");
    assert_frame(native_capture, native);
    unsafe { mediaway_desktop_capture_close(native_capture) };

    // Cursor + hidden border + even crop, all through the config struct.
    let mut config = mediaway_desktop_capture_config_window(window.bits(), time_base, gpu);
    config.cursor = MediawayCaptureCursor::Included;
    config.border = MediawayCaptureBorder::Hidden;
    config.dimensions = MediawayFrameDimensions::EvenCropped;
    let (status, capture) = open(&config);
    assert_eq!(status, MediawayDeviceStatus::Ok, "even-cropped open");
    let cropped = geometry(capture);
    assert_eq!(cropped.0 % 2 + cropped.1 % 2, 0, "{cropped:?} must be even");
    assert_eq!(cropped, (native.0 & !1, native.1 & !1));
    // Needs Windows 11 build 22000+. Asserted, not skipped: a skip is how the border once went
    // unhidden with nobody noticing.
    let status = unsafe { mediaway_desktop_capture_border_hidden(capture, &raw mut hidden) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
    assert!(hidden, "the OS refused to hide the border");
    assert_frame(capture, cropped);
    unsafe { mediaway_desktop_capture_close(capture) };

    // A region at the origin (free: the pool is sized to it) and one away from it (one GPU copy
    // per frame). Both deliver frames of exactly the region's size.
    for (x, y) in [(0, 0), (10, 8)] {
        let mut config = mediaway_desktop_capture_config_window(window.bits(), time_base, gpu);
        config.region_enabled = true;
        (config.region_x, config.region_y) = (x, y);
        (config.region_width, config.region_height) = (64, 48);
        let (status, capture) = open(&config);
        assert_eq!(status, MediawayDeviceStatus::Ok, "region ({x}, {y}) open");
        assert_eq!(geometry(capture), (64, 48));
        assert_frame(capture, (64, 48));
        unsafe { mediaway_desktop_capture_close(capture) };
    }

    // A region larger than the window is its own status, not INVALID_INPUT.
    let mut config = mediaway_desktop_capture_config_window(window.bits(), time_base, gpu);
    config.region_enabled = true;
    (config.region_width, config.region_height) = (native.0 + 100, native.1 + 100);
    let (status, capture) = open(&config);
    assert_eq!(status, MediawayDeviceStatus::RegionOutOfBounds);
    assert!(capture.is_null());

    unsafe { mediaway_gpu_device_close(device) };
    eprintln!("window_capture_smoke: native {native:?}, cropped {cropped:?}, regions ok");
}
