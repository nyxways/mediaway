//! Opaque camera capture handle and its C ABI functions — Camera only.
//!
//! Handle shape and panic-safety strategy: `adr/0001-capture-c-abi.md` §2, §8. Split
//! out of the former unified `video.rs` (Camera+Screen+Window) —
//! `adr/0004-domain-feature-split.md`: Screen/Window moved to `desktop_video.rs`, under
//! a separate `desktop` Cargo feature, so a Camera-only build never links DXGI/WGC
//! backend code. Local platform dispatch (`adr/0001-capture-c-abi.md` §1) goes straight
//! to `mediaway-device-windows-camera`/`mediaway-device-linux`, **not** through the
//! `mediaway-device-windows` orchestrator crate — that crate unconditionally depends on
//! the Desktop/Audio backends too, which would defeat this feature's isolation.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use mediaway_common::VideoFrameStorage;
use mediaway_device::{CaptureError, Select};
use mediaway_device_camera::{CameraCapture, CameraCaptureConfig, CaptureOutputPreference};

use crate::buffer::{leak_boxed_slice, reclaim_boxed_slice};
use crate::status::MediawayDeviceStatus;
use crate::types::{MediawayCameraCaptureConfig, MediawayCameraFrame, MediawayRational};

/// Opaque camera capture handle (`mediaway_camera_capture_t*` in the C header).
///
/// `poll_frame` is called repeatedly (potentially hundreds of times per session) —
/// needs the same `poisoned` guard as `mediaway-container-ffi`'s
/// `MuxerHandle`/`DemuxerHandle`.
///
/// Thread-confined by convention: may be moved between threads, but must not be used
/// from two threads concurrently without external synchronization.
pub struct CameraCaptureHandle {
    poisoned: bool,
    inner: Box<dyn CameraCapture>,
}

/// Build a default camera capture config for device ordinal `device_index`.
#[unsafe(no_mangle)]
pub const extern "C" fn mediaway_camera_capture_config_default(
    device_index: u32,
    time_base: MediawayRational,
) -> MediawayCameraCaptureConfig {
    MediawayCameraCaptureConfig {
        device_index,
        time_base,
    }
}

/// Open a camera capture session for `config`.
///
/// Three outcomes: (1) `Ok` — builds the handle, writes it to `*out_capture`; (2) a
/// normal `Err` — no handle exists, `*out_capture` is set to `NULL`, the matching
/// status is returned; (3) a caught panic — same `NULL`/
/// [`MediawayDeviceStatus::InternalPanic`] shape as (2).
///
/// # Safety
///
/// `config` must be a valid, readable [`MediawayCameraCaptureConfig`] pointer.
/// `out_capture` must be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_camera_capture_open(
    config: *const MediawayCameraCaptureConfig,
    out_capture: *mut *mut CameraCaptureHandle,
) -> MediawayDeviceStatus {
    if config.is_null() || out_capture.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `config` is valid for reads (function contract).
    let config = unsafe { *config };
    // SAFETY: `out_capture` is checked non-null above; caller guarantees it is
    // writable (function contract).
    unsafe { out_capture.write(std::ptr::null_mut()) };

    let result = catch_unwind(AssertUnwindSafe(|| open_camera_capture_for(config)));

    match result {
        Ok(Ok(capture)) => {
            let handle = Box::new(CameraCaptureHandle {
                poisoned: false,
                inner: capture,
            });
            // SAFETY: `out_capture` is checked non-null above (function contract).
            unsafe { out_capture.write(Box::into_raw(handle)) };
            MediawayDeviceStatus::Ok
        }
        Ok(Err(err)) => err.into(),
        Err(_) => MediawayDeviceStatus::InternalPanic,
    }
}

/// Resolve `config` to an open session — shared by [`mediaway_camera_capture_open`] and
/// [`mediaway_camera_capture_capture_once`].
fn open_camera_capture_for(
    config: MediawayCameraCaptureConfig,
) -> Result<Box<dyn CameraCapture>, CaptureError> {
    let select = camera_select(config.device_index)?;
    let rust_config = CameraCaptureConfig {
        select,
        time_base: config.time_base.into(),
        output: CaptureOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    open_camera_capture(&rust_config)
}

/// Resolve a C ABI `device_index` ordinal to a [`Select`] (ADR-0005): `0` maps to
/// [`Select::Default`] with no enumeration round trip. A nonzero index requires a live
/// `enumerate_cameras()` round trip to translate "the Nth camera" into a stable
/// [`Select::Id`] — this ABI never exposed `Select::Id`/`Select::NameContains`
/// directly, so this is the one place that bridges the old raw-ordinal C surface onto
/// the new type.
///
/// # Errors
///
/// Returns [`CaptureError::InvalidInput`] when no camera exists at `device_index`'s
/// ordinal. Returns [`CaptureError::Unsupported`] on a platform whose backend has no
/// enumeration yet (`mediaway-device-linux`, ADR-0005 § Deferred). Returns
/// [`CaptureError::NoBackend`] when no platform backend is compiled in at all.
#[allow(
    clippy::missing_const_for_fn,
    reason = "the windows branch calls a non-const enumerate_cameras(); constness would vary by cfg target"
)]
fn camera_select(device_index: u32) -> Result<Select, CaptureError> {
    if device_index == 0 {
        return Ok(Select::Default);
    }

    #[cfg(windows)]
    {
        let devices = mediaway_device_windows_camera::enumerate_cameras()?;
        let entry = devices
            .into_iter()
            .find(|d| d.ordinal == device_index)
            .ok_or(CaptureError::InvalidInput)?;
        Ok(Select::Id(entry.id))
    }

    #[cfg(target_os = "linux")]
    {
        // `mediaway-device-linux` has no camera enumeration yet (ADR-0005's backend
        // implementation is Windows-only this pass) — a nonzero camera ordinal cannot
        // be resolved to a stable `Select::Id` here.
        let _ = device_index;
        Err(CaptureError::Unsupported)
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = device_index;
        Err(CaptureError::NoBackend)
    }
}

/// Local `#[cfg(windows)]`/`#[cfg(target_os = "linux")]` Camera dispatch — the only
/// `#[cfg(target_os = …)]` in this module, mirroring `mediaway_pipeline::platform`'s
/// shape without importing that crate (`adr/0001-capture-c-abi.md` §1). Goes directly
/// to `mediaway-device-windows-camera`, not the `mediaway-device-windows` orchestrator
/// (`adr/0004-domain-feature-split.md`).
fn open_camera_capture(
    config: &CameraCaptureConfig,
) -> Result<Box<dyn CameraCapture>, CaptureError> {
    #[cfg(windows)]
    {
        use mediaway_device_windows_camera::WindowsCameraCapture;
        let cap = WindowsCameraCapture::open(config)?;
        Ok(Box::new(cap))
    }

    #[cfg(target_os = "linux")]
    {
        use mediaway_device_linux::LinuxCameraCapture;
        let cap = LinuxCameraCapture::open(config)?;
        Ok(Box::new(cap))
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = config;
        Err(CaptureError::NoBackend)
    }
}

/// Query the negotiated frame width/height for an open capture session.
///
/// Takes a `const` handle: on a caught panic (not expected in practice — this only
/// reads already-negotiated geometry) this returns
/// [`MediawayDeviceStatus::InternalPanic`] without poisoning the handle, since there is
/// no mutable access here to record the flag.
///
/// # Safety
///
/// `capture` must be a live pointer returned by [`mediaway_camera_capture_open`].
/// `out_width`/`out_height` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_camera_capture_geometry(
    capture: *const CameraCaptureHandle,
    out_width: *mut u32,
    out_height: *mut u32,
) -> MediawayDeviceStatus {
    if capture.is_null() || out_width.is_null() || out_height.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `capture` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &*capture };
    if handle.poisoned {
        return MediawayDeviceStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| handle.inner.stream_info().geometry()));

    match result {
        Ok(Some(geometry)) => {
            // SAFETY: `out_width`/`out_height` are checked non-null above (function
            // contract).
            unsafe {
                out_width.write(geometry.width);
                out_height.write(geometry.height);
            }
            MediawayDeviceStatus::Ok
        }
        // Defensive only: the Camera backend this crate wraps always reports
        // `StreamInfo::Video`, so `geometry()` is never actually `None` here.
        Ok(None) => MediawayDeviceStatus::Unsupported,
        Err(_) => MediawayDeviceStatus::InternalPanic,
    }
}

/// Pull the next video frame if ready.
///
/// `*out_has_frame == false` is a valid "no frame yet" result, not an error;
/// `*out_frame` is only meaningful when `*out_has_frame == true`, and must then be
/// released with [`mediaway_camera_frame_free`].
///
/// # Safety
///
/// `capture` must be a live pointer returned by [`mediaway_camera_capture_open`].
/// `out_frame` must be a valid, writable pointer to a [`MediawayCameraFrame`].
/// `out_has_frame` must be a valid, writable `bool` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_camera_capture_poll_frame(
    capture: *mut CameraCaptureHandle,
    out_frame: *mut MediawayCameraFrame,
    out_has_frame: *mut bool,
) -> MediawayDeviceStatus {
    if capture.is_null() || out_frame.is_null() || out_has_frame.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `capture` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *capture };
    if handle.poisoned {
        return MediawayDeviceStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let maybe_frame = handle
            .inner
            .poll_frame()
            .map_err(MediawayDeviceStatus::from)?;
        let Some(frame) = maybe_frame else {
            return Ok(None);
        };
        convert_frame(frame).map(Some)
    }));

    match result {
        Ok(Ok(Some(frame))) => {
            // SAFETY: `out_frame`/`out_has_frame` are checked non-null above (function
            // contract).
            unsafe {
                out_frame.write(frame);
                out_has_frame.write(true);
            }
            MediawayDeviceStatus::Ok
        }
        Ok(Ok(None)) => {
            // SAFETY: `out_has_frame` is checked non-null above (function contract).
            unsafe { out_has_frame.write(false) };
            MediawayDeviceStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayDeviceStatus::InternalPanic
        }
    }
}

/// Block until the next video frame is ready or `timeout_ms` elapses.
///
/// Unlike [`mediaway_camera_capture_poll_frame`], an `Ok` status unconditionally means
/// `*out_frame` was written. Does **not** close the session — callers finish reading
/// the frame, then call [`mediaway_camera_capture_close`] themselves.
///
/// # Safety
///
/// `capture` must be a live pointer returned by [`mediaway_camera_capture_open`].
/// `out_frame` must be a valid, writable pointer to a [`MediawayCameraFrame`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_camera_capture_poll_frame_blocking(
    capture: *mut CameraCaptureHandle,
    timeout_ms: u32,
    out_frame: *mut MediawayCameraFrame,
) -> MediawayDeviceStatus {
    if capture.is_null() || out_frame.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `capture` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *capture };
    if handle.poisoned {
        return MediawayDeviceStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        handle
            .inner
            .capture_next_frame_blocking(Duration::from_millis(u64::from(timeout_ms)))
            .map_err(MediawayDeviceStatus::from)
            .and_then(convert_frame)
    }));

    match result {
        Ok(Ok(frame)) => {
            // SAFETY: `out_frame` is checked non-null above (function contract).
            unsafe { out_frame.write(frame) };
            MediawayDeviceStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayDeviceStatus::InternalPanic
        }
    }
}

/// Open a camera capture session, block for one frame (up to `timeout_ms`), then
/// release and close — a convenience for callers who don't want to manage a session
/// (e.g. a hotkey camera snapshot).
///
/// **Pays a full session-open cost on every call** — do not call this in a loop to
/// build a recorder.
///
/// # Safety
///
/// `config` must be a valid, readable [`MediawayCameraCaptureConfig`] pointer.
/// `out_frame` must be a valid, writable pointer to a [`MediawayCameraFrame`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_camera_capture_capture_once(
    config: *const MediawayCameraCaptureConfig,
    timeout_ms: u32,
    out_frame: *mut MediawayCameraFrame,
) -> MediawayDeviceStatus {
    if config.is_null() || out_frame.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `config` is valid for reads (function contract).
    let config = unsafe { *config };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut capture = open_camera_capture_for(config).map_err(MediawayDeviceStatus::from)?;
        let timeout = Duration::from_millis(u64::from(timeout_ms));
        let frame_result = capture
            .capture_next_frame_blocking(timeout)
            .map_err(MediawayDeviceStatus::from);
        let _ = capture.release_frame();
        let _ = capture.close();
        frame_result.and_then(convert_frame)
    }));

    match result {
        Ok(Ok(frame)) => {
            // SAFETY: `out_frame` is checked non-null above (function contract).
            unsafe { out_frame.write(frame) };
            MediawayDeviceStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => MediawayDeviceStatus::InternalPanic,
    }
}

/// Convert a polled [`mediaway_common::VideoFrame`] to its C representation — shared by
/// [`mediaway_camera_capture_poll_frame`], [`mediaway_camera_capture_poll_frame_blocking`],
/// and [`mediaway_camera_capture_capture_once`]. Every Camera backend produces
/// `VideoFrameStorage::Cpu` frames only — copies bytes once into a fresh owned
/// allocation (C has no refcounted-buffer concept to hand across without inventing one,
/// `adr/0001-capture-c-abi.md` §6).
///
/// # Errors
///
/// Returns [`MediawayDeviceStatus::Unsupported`] for any other storage kind —
/// defensive only, no Camera backend this crate wraps produces one today.
fn convert_frame(
    frame: mediaway_common::VideoFrame,
) -> Result<MediawayCameraFrame, MediawayDeviceStatus> {
    let VideoFrameStorage::Cpu { data } = frame.storage else {
        return Err(MediawayDeviceStatus::Unsupported);
    };
    let (data_ptr, data_len) = leak_boxed_slice(data.to_vec());
    Ok(MediawayCameraFrame {
        pts: frame.pts,
        duration: frame.duration,
        width: frame.width,
        height: frame.height,
        pixel_format: frame.format.into(),
        data: data_ptr,
        data_len,
    })
}

/// Release backend resources held by the last polled frame.
///
/// Documented no-op for the Camera backend today (CPU-owned frames hold no backend
/// resource to release — the copy already happened on the worker thread), but must
/// still be called before the next frame-acquiring poll: it matches
/// `CameraCapture::release_frame`'s trait contract 1:1.
///
/// # Safety
///
/// `capture` must be a live pointer returned by [`mediaway_camera_capture_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_camera_capture_release_frame(
    capture: *mut CameraCaptureHandle,
) -> MediawayDeviceStatus {
    if capture.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `capture` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *capture };
    if handle.poisoned {
        return MediawayDeviceStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        handle
            .inner
            .release_frame()
            .map_err(MediawayDeviceStatus::from)
    }));

    match result {
        Ok(Ok(())) => MediawayDeviceStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayDeviceStatus::InternalPanic
        }
    }
}

/// Close a camera capture session, freeing its handle.
///
/// **Blocks for up to one frame interval** — joins the backend's worker thread
/// (`adr/0001-capture-c-abi.md` §9); this is a real, non-instantaneous cost, not merely
/// a pointer free. Always safe to call, including on a poisoned handle, or with
/// `capture == NULL` (a no-op, reported as `Ok`).
///
/// # Safety
///
/// `capture` must be null or a pointer previously returned by
/// [`mediaway_camera_capture_open`] and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_camera_capture_close(
    capture: *mut CameraCaptureHandle,
) -> MediawayDeviceStatus {
    if capture.is_null() {
        return MediawayDeviceStatus::Ok;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `capture` is a valid, not-yet-freed handle pointer
        // (function contract).
        let mut handle = unsafe { Box::from_raw(capture) };
        handle.inner.close().map_err(MediawayDeviceStatus::from)
    }));

    match result {
        Ok(Ok(())) => MediawayDeviceStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => MediawayDeviceStatus::InternalPanic,
    }
}

/// Free a frame returned by [`mediaway_camera_capture_poll_frame`].
///
/// Nulls the frame's `data` pointer/length afterward, making a double-free a visible
/// no-op instead of undefined behavior.
///
/// # Safety
///
/// `frame` must be null or a valid, writable pointer to a [`MediawayCameraFrame`] whose
/// `data`/`data_len` were produced by [`mediaway_camera_capture_poll_frame`] and not
/// already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_camera_frame_free(frame: *mut MediawayCameraFrame) {
    if frame.is_null() {
        return;
    }
    // SAFETY: caller guarantees `frame` is a valid, writable pointer (function
    // contract).
    let frame = unsafe { &mut *frame };
    // SAFETY: `frame.data`/`frame.data_len` were produced by `leak_boxed_slice` via
    // `mediaway_camera_capture_poll_frame` (function contract).
    unsafe { reclaim_boxed_slice(frame.data, frame.data_len) };
    frame.data = std::ptr::null_mut();
    frame.data_len = 0;
}

#[cfg(test)]
#[path = "camera_tests.rs"]
mod tests;
