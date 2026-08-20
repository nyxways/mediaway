//! Opaque desktop video capture handle and its C ABI functions — Screen + Window.
//!
//! Handle shape and panic-safety strategy: `adr/0001-capture-c-abi.md` §2, §8. Split
//! out of the former unified `video.rs` (Camera+Screen+Window) —
//! `adr/0004-domain-feature-split.md`: Camera moved to `camera.rs`, under a separate
//! `camera` Cargo feature, so a Desktop-only build never links Media Foundation Camera
//! backend code. Local platform dispatch (§1) goes straight to
//! `mediaway-device-windows-desktop`, **not** through the `mediaway-device-windows`
//! orchestrator crate — that crate unconditionally depends on the Camera/Audio backends
//! too, which would defeat this feature's isolation. Screen is opened against the real
//! backend (`adr/0003-gpu-handle-c-abi.md`); Window deterministically returns
//! [`CaptureError::Unsupported`] regardless of platform (still § Deferred — a separate
//! HWND-input gap, unaffected by that ADR).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use mediaway_common::VideoFrameStorage;
use mediaway_device::desktop::{
    CaptureOutputPreference, CaptureSharing, DesktopCaptureSource, DesktopVideoCapture,
    DesktopVideoCaptureConfig,
};
use mediaway_device::{CaptureError, Select};

use crate::device::buffer::{leak_boxed_slice, reclaim_boxed_slice};
use crate::device::status::MediawayDeviceStatus;
use crate::device::types::{
    MediawayDesktopCaptureConfig, MediawayDesktopCaptureSourceKind, MediawayDesktopFrame,
    MediawayGpuBufferKind, MediawayGpuDeviceHandle, MediawayRational,
    MediawayVideoFrameStorageKind,
};

/// Zeroed placeholder for [`MediawayDesktopFrame::gpu_buffer`] when
/// `storage_kind == Cpu` — never read by a correct caller (the outer `storage_kind` tag
/// disambiguates), same "meaningless default, not a real value" idiom as
/// `MediawayPixelFormat`'s `#[non_exhaustive]` fallback in `types.rs`.
const EMPTY_GPU_BUFFER: crate::device::types::MediawayGpuBufferHandle =
    crate::device::types::MediawayGpuBufferHandle {
        kind: MediawayGpuBufferKind::DirectX11,
        native_a: 0,
        native_b: 0,
        subresource: 0,
        webgpu_texture_id: 0,
    };

/// Opaque desktop video capture handle (`mediaway_desktop_capture_t*` in the C header).
///
/// `poll_frame`/`release_frame` are called repeatedly (potentially hundreds of times
/// per session) — needs the same `poisoned` guard as `mediaway-container-ffi`'s
/// `MuxerHandle`/`DemuxerHandle`.
///
/// Thread-confined by convention: may be moved between threads, but must not be used
/// from two threads concurrently without external synchronization.
pub struct DesktopCaptureHandle {
    // `pub(crate)`, not private: `pipeline`'s capture-to-encode bridge
    // (`adr/pipeline/0005-capture-encode-bridge-c-abi.md`) polls/releases this
    // handle directly from another module of the same crate — still fully opaque
    // to C callers regardless of Rust-level visibility.
    pub(crate) poisoned: bool,
    pub(crate) inner: Box<dyn DesktopVideoCapture>,
}

/// Build a Screen capture config for output ordinal `output_index`.
///
/// `gpu_device` must be a live `MEDIAWAY_GPU_DEVICE_DIRECTX11` — see
/// `adr/0003-gpu-handle-c-abi.md` §2 for the caller's device-lifetime obligation.
#[unsafe(no_mangle)]
pub const extern "C" fn mediaway_desktop_capture_config_screen(
    output_index: u32,
    time_base: MediawayRational,
    gpu_device: MediawayGpuDeviceHandle,
) -> MediawayDesktopCaptureConfig {
    MediawayDesktopCaptureConfig {
        source_kind: MediawayDesktopCaptureSourceKind::Screen,
        source_index: output_index,
        time_base,
        gpu_device,
    }
}

/// Open a desktop video capture session for `config`.
///
/// `Screen` configs can succeed (`adr/0003-gpu-handle-c-abi.md`); `Window` still
/// deterministically returns [`MediawayDeviceStatus::Unsupported`] (separate HWND-input
/// gap, § Deferred). `gpu_device` is enforced, not merely documented: a
/// `NONE`/malformed one returns [`MediawayDeviceStatus::InvalidInput`]
/// (`adr/0003-gpu-handle-c-abi.md` §4). Three outcomes: (1) `Ok` — builds the handle,
/// writes it to `*out_capture`; (2) a normal `Err` — no handle exists, `*out_capture`
/// is set to `NULL`, the matching status is returned; (3) a caught panic — same
/// `NULL`/[`MediawayDeviceStatus::InternalPanic`] shape as (2).
///
/// # Safety
///
/// `config` must be a valid, readable [`MediawayDesktopCaptureConfig`] pointer.
/// `out_capture` must be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_desktop_capture_open(
    config: *const MediawayDesktopCaptureConfig,
    out_capture: *mut *mut DesktopCaptureHandle,
) -> MediawayDeviceStatus {
    if config.is_null() || out_capture.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `config` is valid for reads (function contract).
    let config = unsafe { *config };
    // SAFETY: `out_capture` is checked non-null above; caller guarantees it is
    // writable (function contract).
    unsafe { out_capture.write(std::ptr::null_mut()) };

    let result = catch_unwind(AssertUnwindSafe(|| open_desktop_capture(config)));

    match result {
        Ok(Ok(capture)) => {
            let handle = Box::new(DesktopCaptureHandle {
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

/// Dispatch shared by [`mediaway_desktop_capture_open`] — resolves `config` to an open
/// session against the matching backend.
fn open_desktop_capture(
    config: MediawayDesktopCaptureConfig,
) -> Result<Box<dyn DesktopVideoCapture>, CaptureError> {
    match config.source_kind {
        MediawayDesktopCaptureSourceKind::Window => Err(CaptureError::Unsupported),
        MediawayDesktopCaptureSourceKind::Screen => {
            // Not unwrapped/rejected here: `WindowsScreenCapture::open` already returns
            // `CaptureError::InvalidInput` for a `None` `gpu_device` internally —
            // letting it do so keeps this exactly the existing Rust-level rule, not a
            // new one invented at the FFI layer (`adr/0003-gpu-handle-c-abi.md` §4).
            let select = screen_select(config.source_index)?;
            let rust_config = DesktopVideoCaptureConfig {
                source: DesktopCaptureSource::Screen { select },
                time_base: config.time_base.into(),
                output: CaptureOutputPreference::ZeroCopyGpu,
                gpu_device: config.gpu_device.to_common(),
                // No C ABI knob for this yet — keep today's shareable-by-default behavior
                // unchanged; see `mediaway-device` ADR-0008.
                sharing: CaptureSharing::Shared,
            };
            open_screen_capture(&rust_config)
        }
    }
}

/// Resolve a C ABI `source_index` ordinal to a [`Select`] for Screen (ADR-0005): `0`
/// maps to [`Select::Default`] with no enumeration round trip.
///
/// # Errors
///
/// Returns [`CaptureError::InvalidInput`] when no output exists at `source_index`'s
/// ordinal. Returns [`CaptureError::NoBackend`] on a non-Windows platform (Linux's
/// screen capture is CPU-only and not wired into this dispatch — see
/// [`open_screen_capture`]).
#[allow(
    clippy::missing_const_for_fn,
    reason = "the windows branch calls a non-const enumerate_outputs(); constness would vary by cfg target"
)]
fn screen_select(source_index: u32) -> Result<Select, CaptureError> {
    if source_index == 0 {
        return Ok(Select::Default);
    }

    #[cfg(windows)]
    {
        let devices = mediaway_device::windows_desktop::enumerate_outputs()?;
        let entry = devices
            .into_iter()
            .find(|d| d.ordinal == source_index)
            .ok_or(CaptureError::InvalidInput)?;
        Ok(Select::Id(entry.id))
    }

    #[cfg(not(windows))]
    {
        let _ = source_index;
        Err(CaptureError::NoBackend)
    }
}

/// Screen dispatch — Windows only (`adr/0003-gpu-handle-c-abi.md`). `mediaway-device-linux`'s
/// screen capture requires `CaptureOutputPreference::CpuFramesOk` and rejects a GPU
/// device outright — a fundamentally different shape from this dispatch's
/// GPU-device-mandatory Screen config, needing its own config surface this ADR does not
/// design. Not wired here — same "Linux capture dispatch unverified this pass" gap
/// `adr/0001-capture-c-abi.md` § Deferred already logged for Camera.
fn open_screen_capture(
    config: &DesktopVideoCaptureConfig,
) -> Result<Box<dyn DesktopVideoCapture>, CaptureError> {
    #[cfg(windows)]
    {
        use mediaway_device::windows_desktop::WindowsScreenCapture;
        let cap = WindowsScreenCapture::open(config)?;
        Ok(Box::new(cap))
    }

    #[cfg(not(windows))]
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
/// `capture` must be a live pointer returned by [`mediaway_desktop_capture_open`].
/// `out_width`/`out_height` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_desktop_capture_geometry(
    capture: *const DesktopCaptureHandle,
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
        // Defensive only: every backend this crate wraps reports `StreamInfo::Video`
        // for a desktop video capture handle, so `geometry()` is never actually `None`
        // here.
        Ok(None) => MediawayDeviceStatus::Unsupported,
        Err(_) => MediawayDeviceStatus::InternalPanic,
    }
}

/// Pull the next video frame if ready.
///
/// `*out_has_frame == false` is a valid "no frame yet" result, not an error;
/// `*out_frame` is only meaningful when `*out_has_frame == true`, and must then be
/// released with [`mediaway_desktop_frame_free`].
///
/// # Safety
///
/// `capture` must be a live pointer returned by [`mediaway_desktop_capture_open`].
/// `out_frame` must be a valid, writable pointer to a [`MediawayDesktopFrame`].
/// `out_has_frame` must be a valid, writable `bool` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_desktop_capture_poll_frame(
    capture: *mut DesktopCaptureHandle,
    out_frame: *mut MediawayDesktopFrame,
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
/// Unlike [`mediaway_desktop_capture_poll_frame`], an `Ok` status unconditionally means
/// `*out_frame` was written — there is no separate has-frame flag
/// (`adr/0003-gpu-handle-c-abi.md` §5). Does **not** close the session — callers finish
/// reading the frame, then call
/// [`mediaway_desktop_capture_release_frame`]/[`mediaway_desktop_capture_close`]
/// themselves. This is the recommended way to capture a single Screen frame
/// (`gpu_buffer` stays valid for the whole read window, unlike a composed
/// open+capture+close convenience — see `adr/0003-gpu-handle-c-abi.md` § Context for
/// why that shape is not offered for Screen).
///
/// # Safety
///
/// `capture` must be a live pointer returned by [`mediaway_desktop_capture_open`].
/// `out_frame` must be a valid, writable pointer to a [`MediawayDesktopFrame`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_desktop_capture_poll_frame_blocking(
    capture: *mut DesktopCaptureHandle,
    timeout_ms: u32,
    out_frame: *mut MediawayDesktopFrame,
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

/// Convert a polled [`mediaway_common::VideoFrame`] to its C representation — shared by
/// [`mediaway_desktop_capture_poll_frame`] and
/// [`mediaway_desktop_capture_poll_frame_blocking`]. Handles both storage kinds
/// (`adr/0003-gpu-handle-c-abi.md` §3): `Cpu` copies once into a fresh owned allocation
/// (C has no refcounted-buffer concept to hand across without inventing one,
/// `adr/0001-capture-c-abi.md` §6); `Gpu` passes the backend's borrowed texture handle
/// through unchanged (never owned by this crate, never freed by
/// [`mediaway_desktop_frame_free`]).
///
/// # Errors
///
/// Returns [`MediawayDeviceStatus::Unsupported`] for a storage kind neither of the two
/// arms above matches — `VideoFrameStorage` is `#[non_exhaustive]`; defensive only, no
/// backend this crate wraps produces a third kind today.
fn convert_frame(
    frame: mediaway_common::VideoFrame,
) -> Result<MediawayDesktopFrame, MediawayDeviceStatus> {
    match frame.storage {
        VideoFrameStorage::Cpu { data } => {
            let (data_ptr, data_len) = leak_boxed_slice(data.to_vec());
            Ok(MediawayDesktopFrame {
                pts: frame.pts,
                duration: frame.duration,
                width: frame.width,
                height: frame.height,
                pixel_format: frame.format.into(),
                storage_kind: MediawayVideoFrameStorageKind::Cpu,
                data: data_ptr,
                data_len,
                gpu_buffer: EMPTY_GPU_BUFFER,
            })
        }
        VideoFrameStorage::Gpu(handle) => Ok(MediawayDesktopFrame {
            pts: frame.pts,
            duration: frame.duration,
            width: frame.width,
            height: frame.height,
            pixel_format: frame.format.into(),
            storage_kind: MediawayVideoFrameStorageKind::Gpu,
            data: std::ptr::null_mut(),
            data_len: 0,
            gpu_buffer: handle.into(),
        }),
        _ => Err(MediawayDeviceStatus::Unsupported),
    }
}

/// Release backend resources held by the last polled frame (e.g. DXGI `ReleaseFrame`).
///
/// Must be called before the next successful poll that acquires again.
///
/// # Safety
///
/// `capture` must be a live pointer returned by [`mediaway_desktop_capture_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_desktop_capture_release_frame(
    capture: *mut DesktopCaptureHandle,
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

/// Close a desktop video capture session, freeing its handle.
///
/// **Blocks for up to one frame interval** — joins the backend's worker thread
/// (`adr/0001-capture-c-abi.md` §9); this is a real, non-instantaneous cost, not merely
/// a pointer free. Always safe to call, including on a poisoned handle, or with
/// `capture == NULL` (a no-op, reported as `Ok`). In the unlikely event a panic occurs
/// while closing/dropping the handle, the allocation is deliberately leaked rather than
/// double-handled.
///
/// # Safety
///
/// `capture` must be null or a pointer previously returned by
/// [`mediaway_desktop_capture_open`] and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_desktop_capture_close(
    capture: *mut DesktopCaptureHandle,
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

/// Free a frame returned by [`mediaway_desktop_capture_poll_frame`].
///
/// Nulls the frame's `data` pointer/length afterward, making a double-free a visible
/// no-op instead of undefined behavior.
///
/// # Safety
///
/// `frame` must be null or a valid, writable pointer to a [`MediawayDesktopFrame`]
/// whose `data`/`data_len` were produced by [`mediaway_desktop_capture_poll_frame`] and
/// not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_desktop_frame_free(frame: *mut MediawayDesktopFrame) {
    if frame.is_null() {
        return;
    }
    // SAFETY: caller guarantees `frame` is a valid, writable pointer (function
    // contract).
    let frame = unsafe { &mut *frame };
    // SAFETY: `frame.data`/`frame.data_len` were produced by `leak_boxed_slice` via
    // `mediaway_desktop_capture_poll_frame` (function contract).
    unsafe { reclaim_boxed_slice(frame.data, frame.data_len) };
    frame.data = std::ptr::null_mut();
    frame.data_len = 0;
}

#[cfg(test)]
#[path = "desktop_video_tests.rs"]
mod tests;
