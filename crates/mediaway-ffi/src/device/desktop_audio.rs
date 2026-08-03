//! Opaque desktop audio capture handle and its C ABI functions — Loopback +
//! `ProcessLoopback`.
//!
//! Handle shape and panic-safety strategy: `adr/0001-capture-c-abi.md` §2, §8. Split
//! out of the former unified `audio.rs` — `adr/0004-domain-feature-split.md`: grouped
//! with `desktop_video.rs` under the `desktop` Cargo feature (not `audio`), because
//! Loopback/`ProcessLoopback` capture *what the desktop is already rendering* — a
//! desktop-capture concept, not a real audio input device (same reasoning as the Rust
//! facade split, `mediaway-device/adr/0007-domain-crate-split.md`). Local platform
//! dispatch (§1) goes straight to `mediaway-device-windows-desktop`'s
//! `WindowsDesktopAudioCapture` (itself a thin wrapper over
//! `mediaway-device-windows-audio`'s shared WASAPI engine), not through the
//! `mediaway-device-windows` orchestrator crate.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_device::{CaptureError, Select};
use mediaway_device_desktop::{
    DesktopAudioCapture, DesktopAudioCaptureConfig, DesktopAudioSource, ProcessTreeScope,
};

use crate::device::buffer::{leak_boxed_slice, reclaim_boxed_slice};
use crate::device::status::MediawayDeviceStatus;
use crate::device::types::{
    MediawayDesktopAudioCaptureConfig, MediawayDesktopAudioFrame, MediawayDesktopAudioSourceKind,
    MediawayRational, MediawaySampleFormat,
};

/// Opaque desktop audio capture handle (`mediaway_desktop_audio_capture_t*` in the C
/// header).
///
/// `poll_frame` is called repeatedly (potentially hundreds of times per session) —
/// needs the same `poisoned` guard as `mediaway-container-ffi`'s
/// `MuxerHandle`/`DemuxerHandle`.
///
/// Thread-confined by convention: may be moved between threads, but must not be used
/// from two threads concurrently without external synchronization.
pub struct DesktopAudioCaptureHandle {
    poisoned: bool,
    inner: Box<dyn DesktopAudioCapture>,
}

/// Build a default-system-loopback capture config.
#[unsafe(no_mangle)]
pub const extern "C" fn mediaway_desktop_audio_capture_config_loopback(
    time_base: MediawayRational,
) -> MediawayDesktopAudioCaptureConfig {
    MediawayDesktopAudioCaptureConfig {
        source_kind: MediawayDesktopAudioSourceKind::Loopback,
        device_index: 0,
        process_id: 0,
        include_child_processes: false,
        time_base,
        sample_format: MediawaySampleFormat::F32,
    }
}

/// Build a per-process loopback capture config for `process_id`.
#[unsafe(no_mangle)]
pub const extern "C" fn mediaway_desktop_audio_capture_config_process_loopback(
    process_id: u32,
    include_child_processes: bool,
    time_base: MediawayRational,
) -> MediawayDesktopAudioCaptureConfig {
    MediawayDesktopAudioCaptureConfig {
        source_kind: MediawayDesktopAudioSourceKind::ProcessLoopback,
        device_index: 0,
        process_id,
        include_child_processes,
        time_base,
        sample_format: MediawaySampleFormat::F32,
    }
}

/// Open a desktop audio capture session for `config`.
///
/// Three outcomes: (1) `Ok` — builds the handle, writes it to `*out_capture`; (2) a
/// normal `Err` (e.g. an unsupported source/sample-format combination) — no handle
/// exists, `*out_capture` is set to `NULL`, the matching status is returned; (3) a
/// caught panic — same `NULL`/[`MediawayDeviceStatus::InternalPanic`] shape as (2).
///
/// # Safety
///
/// `config` must be a valid, readable [`MediawayDesktopAudioCaptureConfig`] pointer.
/// `out_capture` must be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_desktop_audio_capture_open(
    config: *const MediawayDesktopAudioCaptureConfig,
    out_capture: *mut *mut DesktopAudioCaptureHandle,
) -> MediawayDeviceStatus {
    if config.is_null() || out_capture.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `config` is valid for reads (function contract).
    let config = unsafe { *config };
    // SAFETY: `out_capture` is checked non-null above; caller guarantees it is
    // writable (function contract).
    unsafe { out_capture.write(std::ptr::null_mut()) };

    let result = catch_unwind(AssertUnwindSafe(|| {
        // This C ABI does not yet expose `Select::Id`/`Select::NameContains`
        // (ADR-0005) — `device_index == 0` maps to `Select::Default`; anything else is
        // rejected up front the same way every backend already hard-rejected a nonzero
        // index before this ADR.
        let source = match config.source_kind {
            MediawayDesktopAudioSourceKind::Loopback => {
                if config.device_index != 0 {
                    return Err(CaptureError::Unsupported);
                }
                DesktopAudioSource::Loopback {
                    select: Select::Default,
                }
            }
            MediawayDesktopAudioSourceKind::ProcessLoopback => {
                DesktopAudioSource::ProcessLoopback {
                    process_id: config.process_id,
                    tree_scope: if config.include_child_processes {
                        ProcessTreeScope::IncludeChildren
                    } else {
                        ProcessTreeScope::ProcessOnly
                    },
                }
            }
        };
        let rust_config = DesktopAudioCaptureConfig {
            source,
            time_base: config.time_base.into(),
            sample_format: config.sample_format.into(),
        };
        open_desktop_audio_capture(&rust_config)
    }));

    match result {
        Ok(Ok(capture)) => {
            let handle = Box::new(DesktopAudioCaptureHandle {
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

/// Local `#[cfg(windows)]` desktop-audio dispatch — mirrors `camera.rs`/
/// `desktop_video.rs`'s shape (`adr/0001-capture-c-abi.md` §1). No
/// `mediaway-device-linux` desktop-audio backend exists yet.
fn open_desktop_audio_capture(
    config: &DesktopAudioCaptureConfig,
) -> Result<Box<dyn DesktopAudioCapture>, CaptureError> {
    #[cfg(windows)]
    {
        use mediaway_device_windows_desktop::WindowsDesktopAudioCapture;
        let cap = WindowsDesktopAudioCapture::open(config)?;
        Ok(Box::new(cap))
    }

    #[cfg(not(windows))]
    {
        let _ = config;
        Err(CaptureError::NoBackend)
    }
}

/// Query the negotiated sample rate/channel count for an open capture session.
///
/// Takes a `const` handle: on a caught panic (not expected in practice — this only
/// reads already-negotiated format) this returns
/// [`MediawayDeviceStatus::InternalPanic`] without poisoning the handle, since there is
/// no mutable access here to record the flag.
///
/// # Safety
///
/// `capture` must be a live pointer returned by [`mediaway_desktop_audio_capture_open`].
/// `out_sample_rate`/`out_channels` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_desktop_audio_capture_format(
    capture: *const DesktopAudioCaptureHandle,
    out_sample_rate: *mut u32,
    out_channels: *mut u16,
) -> MediawayDeviceStatus {
    if capture.is_null() || out_sample_rate.is_null() || out_channels.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `capture` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &*capture };
    if handle.poisoned {
        return MediawayDeviceStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let info = handle.inner.stream_info();
        (info.sample_rate(), info.channels())
    }));

    match result {
        Ok((Some(sample_rate), Some(channels))) => {
            // SAFETY: `out_sample_rate`/`out_channels` are checked non-null above
            // (function contract).
            unsafe {
                out_sample_rate.write(sample_rate);
                out_channels.write(channels);
            }
            MediawayDeviceStatus::Ok
        }
        // Defensive only: every backend this crate wraps reports `StreamInfo::Audio`
        // for a desktop audio capture handle, so these are never actually `None` here.
        Ok(_) => MediawayDeviceStatus::Unsupported,
        Err(_) => MediawayDeviceStatus::InternalPanic,
    }
}

/// Pull the next PCM chunk if ready.
///
/// `*out_has_frame == false` is a valid "no samples yet" result, not an error;
/// `*out_frame` is only meaningful when `*out_has_frame == true`, and must then be
/// released with [`mediaway_desktop_audio_frame_free`].
///
/// # Safety
///
/// `capture` must be a live pointer returned by [`mediaway_desktop_audio_capture_open`].
/// `out_frame` must be a valid, writable pointer to a [`MediawayDesktopAudioFrame`].
/// `out_has_frame` must be a valid, writable `bool` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_desktop_audio_capture_poll_frame(
    capture: *mut DesktopAudioCaptureHandle,
    out_frame: *mut MediawayDesktopAudioFrame,
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
        // The FFI layer copies once (`Bytes::to_vec()` -> `into_boxed_slice()` ->
        // `Box::into_raw()`) into a fresh owned allocation — C has no refcounted-buffer
        // concept to hand across without inventing one (`adr/0001-capture-c-abi.md` §6).
        let (data_ptr, data_len) = leak_boxed_slice(frame.data.to_vec());
        Ok(Some(MediawayDesktopAudioFrame {
            pts: frame.pts,
            duration: frame.duration,
            sample_rate: frame.sample_rate,
            channels: frame.channels,
            sample_format: frame.format.into(),
            data: data_ptr,
            data_len,
        }))
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

/// Close a desktop audio capture session, freeing its handle.
///
/// **Blocks for up to one period interval** — joins the backend's worker thread
/// (`adr/0001-capture-c-abi.md` §9); this is a real, non-instantaneous cost, not merely
/// a pointer free. Always safe to call, including on a poisoned handle, or with
/// `capture == NULL` (a no-op, reported as `Ok`).
///
/// # Safety
///
/// `capture` must be null or a pointer previously returned by
/// [`mediaway_desktop_audio_capture_open`] and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_desktop_audio_capture_close(
    capture: *mut DesktopAudioCaptureHandle,
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

/// Free a frame returned by [`mediaway_desktop_audio_capture_poll_frame`].
///
/// Nulls the frame's `data` pointer/length afterward, making a double-free a visible
/// no-op instead of undefined behavior.
///
/// # Safety
///
/// `frame` must be null or a valid, writable pointer to a [`MediawayDesktopAudioFrame`]
/// whose `data`/`data_len` were produced by
/// [`mediaway_desktop_audio_capture_poll_frame`] and not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_desktop_audio_frame_free(frame: *mut MediawayDesktopAudioFrame) {
    if frame.is_null() {
        return;
    }
    // SAFETY: caller guarantees `frame` is a valid, writable pointer (function
    // contract).
    let frame = unsafe { &mut *frame };
    // SAFETY: `frame.data`/`frame.data_len` were produced by `leak_boxed_slice` via
    // `mediaway_desktop_audio_capture_poll_frame` (function contract).
    unsafe { reclaim_boxed_slice(frame.data, frame.data_len) };
    frame.data = std::ptr::null_mut();
    frame.data_len = 0;
}

#[cfg(test)]
#[path = "desktop_audio_tests.rs"]
mod tests;
