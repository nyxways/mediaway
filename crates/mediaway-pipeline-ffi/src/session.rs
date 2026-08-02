//! Opaque encode-session handle and its C ABI functions.
//!
//! Panic-safety strategy: `adr/0001-auto-encode-c-abi.md` §7.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_common::{Bytes, VideoFrame, VideoFrameStorage};
use mediaway_pipeline::EncodeSession;

use crate::buffer::{borrow_slice, leak_boxed_slice};
use crate::encoder::AutoEncoderHandle;
use crate::status::MediawayPipelineStatus;
use crate::types::{MediawayVideoFrame, MediawayVideoFrameStorageKind};

/// Opaque encode-session handle (`mediaway_encode_session_t*` in the C header).
///
/// `write_frame` is called repeatedly — needs the same `poisoned` guard as
/// `mediaway-container-ffi`'s `MuxerHandle`/`DemuxerHandle`.
///
/// Thread-confined by convention: may be moved between threads, but must not be
/// used from two threads concurrently without external synchronization.
pub struct EncodeSessionHandle {
    poisoned: bool,
    inner: EncodeSession<AutoEncoderHandle>,
}

/// Register `encoder`'s stream as an MP4 track and begin streaming.
///
/// **Non-obvious ownership rule:** this function takes ownership of `encoder`
/// **unconditionally** — success or failure — because `EncodeSession::open`
/// takes its encoder by value in Rust. On the `Err` path (the muxer rejects the
/// encoder's stream info) the moved-in encoder is simply dropped as part of
/// unwinding the `Result`, same as Rust itself would do. After calling this
/// function, `encoder` is invalid regardless of the returned status; do **not**
/// call [`mediaway_auto_encoder_close`](crate::mediaway_auto_encoder_close) on
/// it afterward (double-free).
///
/// # Safety
///
/// `encoder` must be a live pointer returned by
/// [`mediaway_auto_encoder_open`](crate::mediaway_auto_encoder_open) and not
/// already consumed. `out_session` must be a valid, writable, non-null
/// out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_encode_session_open(
    encoder: *mut AutoEncoderHandle,
    out_session: *mut *mut EncodeSessionHandle,
) -> MediawayPipelineStatus {
    if encoder.is_null() || out_session.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `out_session` is writable (function contract).
    unsafe { out_session.write(std::ptr::null_mut()) };
    // SAFETY: caller guarantees `encoder` is a live, not-yet-consumed pointer
    // returned by `mediaway_auto_encoder_open` (function contract). This
    // unconditionally consumes `encoder` — see the doc comment above.
    let encoder = unsafe { Box::from_raw(encoder) };

    let result = catch_unwind(AssertUnwindSafe(move || EncodeSession::open(*encoder)));

    match result {
        Ok(Ok(session)) => {
            let handle = Box::new(EncodeSessionHandle {
                poisoned: false,
                inner: session,
            });
            // SAFETY: `out_session` is checked non-null above (function contract).
            unsafe { out_session.write(Box::into_raw(handle)) };
            MediawayPipelineStatus::Ok
        }
        Ok(Err(err)) => err.into(),
        Err(_) => MediawayPipelineStatus::InternalPanic,
    }
}

/// Push one frame and drain any packets it produces into the muxer.
///
/// # Safety
///
/// `session` must be a live pointer returned by [`mediaway_encode_session_open`]
/// and not yet passed to [`mediaway_encode_session_finish`] or
/// [`mediaway_encode_session_close`]. `frame` must point to a valid
/// [`MediawayVideoFrame`]. When `storage_kind == Cpu`, `raw_bytes`/`raw_bytes_len`
/// must describe a buffer valid for reads for the duration of this call (or be
/// null with length `0`). When `storage_kind == Gpu`, `gpu_buffer` must alias a
/// live GPU resource valid for reads for the duration of this call
/// (`adr/0002-gpu-frame-input-c-abi.md` §2/§8).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_encode_session_write_frame(
    session: *mut EncodeSessionHandle,
    frame: *const MediawayVideoFrame,
) -> MediawayPipelineStatus {
    if session.is_null() || frame.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `session` is a valid, live handle pointer
    // (function contract).
    let handle = unsafe { &mut *session };
    if handle.poisoned {
        return MediawayPipelineStatus::HandlePoisoned;
    }
    // SAFETY: caller guarantees `frame` is valid for reads (function contract).
    let view = unsafe { *frame };

    let storage = match view.storage_kind {
        MediawayVideoFrameStorageKind::Cpu => {
            // SAFETY: `view.raw_bytes`/`view.raw_bytes_len` describe a buffer valid
            // for this call (function contract).
            let Some(raw_bytes) = (unsafe { borrow_slice(view.raw_bytes, view.raw_bytes_len) })
            else {
                return MediawayPipelineStatus::InvalidArgument;
            };
            VideoFrameStorage::Cpu {
                data: Bytes::copy_from_slice(raw_bytes),
            }
        }
        MediawayVideoFrameStorageKind::Gpu => {
            let Some(gpu_buffer) = view.gpu_buffer.to_common() else {
                return MediawayPipelineStatus::InvalidArgument;
            };
            VideoFrameStorage::Gpu(gpu_buffer)
        }
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let video_frame = VideoFrame {
            pts: view.pts,
            duration: view.duration,
            width: view.width,
            height: view.height,
            format: view.pixel_format.into(),
            storage,
        };
        handle
            .inner
            .write_frame(&video_frame)
            .map_err(MediawayPipelineStatus::from)
    }));

    match result {
        Ok(Ok(())) => MediawayPipelineStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayPipelineStatus::InternalPanic
        }
    }
}

/// Flush the encoder and muxer, returning the complete fMP4 byte stream.
///
/// Consumes `session` **unconditionally** — success, failure, or a caught
/// panic — because `EncodeSession::finish` takes `self` by value in Rust. The
/// session handle is invalid after this call regardless of outcome; do **not**
/// call [`mediaway_encode_session_close`] on it afterward (double-free). The
/// returned buffer must be released with
/// [`mediaway_pipeline_ffi_buffer_free`](crate::mediaway_pipeline_ffi_buffer_free).
///
/// # Safety
///
/// `session` must be a live pointer returned by [`mediaway_encode_session_open`]
/// and not already consumed. `out_data` and `out_len` must be valid, writable,
/// non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_encode_session_finish(
    session: *mut EncodeSessionHandle,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> MediawayPipelineStatus {
    if session.is_null() || out_data.is_null() || out_len.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `session` is a live, not-yet-consumed pointer
    // (function contract). This unconditionally consumes `session` — see the
    // doc comment above.
    let handle = unsafe { Box::from_raw(session) };
    if handle.poisoned {
        return MediawayPipelineStatus::HandlePoisoned;
    }
    let EncodeSessionHandle { inner, .. } = *handle;

    let result = catch_unwind(AssertUnwindSafe(move || inner.finish()));

    match result {
        Ok(Ok(bytes)) => {
            let (ptr, len) = leak_boxed_slice(bytes);
            // SAFETY: `out_data`/`out_len` are checked non-null above (function contract).
            unsafe {
                out_data.write(ptr);
                out_len.write(len);
            }
            MediawayPipelineStatus::Ok
        }
        Ok(Err(err)) => err.into(),
        Err(_) => MediawayPipelineStatus::InternalPanic,
    }
}

/// Abandon a session without finishing it — no flush, no valid MP4 output.
///
/// Always safe to call, including on a poisoned handle. Added for the same
/// resource-cleanup symmetry as
/// [`mediaway_auto_encoder_close`](crate::mediaway_auto_encoder_close). In the
/// unlikely event a panic occurs while dropping the handle, the allocation is
/// deliberately leaked rather than double-handled — same reasoning as
/// `mediaway-container-ffi`'s `mediaway_muxer_close`.
///
/// # Safety
///
/// `session` must be null or a pointer previously returned by
/// [`mediaway_encode_session_open`] and not already consumed by this function or
/// [`mediaway_encode_session_finish`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_encode_session_close(session: *mut EncodeSessionHandle) {
    if session.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `session` is a valid, not-yet-freed handle
        // pointer (function contract).
        drop(unsafe { Box::from_raw(session) });
    }));
}
