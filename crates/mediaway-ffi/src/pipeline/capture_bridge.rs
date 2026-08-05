//! Capture-to-encode bridge: pushes one frame from a `device`-module capture
//! handle directly into an `EncodeSessionHandle`.
//!
//! Design: `adr/0005-capture-encode-bridge-c-abi.md` — no intermediate
//! `mediaway_{camera,desktop}_frame_t`, no extra copy (`VideoFrame` moves from
//! poll to push inside one Rust call), `release_frame` called unconditionally
//! after the push attempt for the GPU (Screen) case.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::device::{CameraCaptureHandle, DesktopCaptureHandle};
use crate::pipeline::session::EncodeSessionHandle;
use crate::pipeline::status::MediawayPipelineStatus;

/// Poll one frame from `capture` (Camera) and push it into `session`.
///
/// `*out_wrote_frame == false` is a valid "no new frame ready yet" result (the
/// underlying `poll_frame` returned `Ok(None)`), not an error — mirrors
/// `mediaway_camera_capture_poll_frame`'s own `out_has_frame` shape.
/// `mediaway_camera_capture_release_frame` is called internally after the push
/// attempt (documented no-op for the Camera backend today, called anyway for
/// contract symmetry — `adr/0005` §2).
///
/// # Safety
///
/// `session`/`capture` must both be valid, live handle pointers.
/// `out_wrote_frame` must be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_encode_session_write_frame_from_camera_capture(
    session: *mut EncodeSessionHandle,
    capture: *mut CameraCaptureHandle,
    out_wrote_frame: *mut bool,
) -> MediawayPipelineStatus {
    if session.is_null() || capture.is_null() || out_wrote_frame.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees both handles are valid, live pointers (function
    // contract).
    let session = unsafe { &mut *session };
    let capture = unsafe { &mut *capture };
    if session.poisoned || capture.poisoned {
        return MediawayPipelineStatus::HandlePoisoned;
    }
    // SAFETY: `out_wrote_frame` is checked non-null above; caller guarantees it
    // is writable (function contract).
    unsafe { out_wrote_frame.write(false) };

    let poll_result = catch_unwind(AssertUnwindSafe(|| capture.inner.poll_frame()));
    let frame = match poll_result {
        Ok(Ok(Some(frame))) => frame,
        Ok(Ok(None)) => return MediawayPipelineStatus::Ok,
        Ok(Err(err)) => return err.into(),
        Err(_) => {
            capture.poisoned = true;
            return MediawayPipelineStatus::InternalPanic;
        }
    };

    let write_result = catch_unwind(AssertUnwindSafe(|| session.inner.write_frame(&frame)));
    let release_result = catch_unwind(AssertUnwindSafe(|| capture.inner.release_frame()));

    let write_status = match write_result {
        Ok(Ok(())) => {
            // SAFETY: `out_wrote_frame` is checked non-null above (function contract).
            unsafe { out_wrote_frame.write(true) };
            MediawayPipelineStatus::Ok
        }
        Ok(Err(err)) => err.into(),
        Err(_) => {
            session.poisoned = true;
            MediawayPipelineStatus::InternalPanic
        }
    };
    match release_result {
        Ok(Ok(())) => write_status,
        Ok(Err(err)) => {
            if write_status == MediawayPipelineStatus::Ok {
                err.into()
            } else {
                write_status
            }
        }
        Err(_) => {
            capture.poisoned = true;
            if write_status == MediawayPipelineStatus::Ok {
                MediawayPipelineStatus::InternalPanic
            } else {
                write_status
            }
        }
    }
}

/// Poll one frame from `capture` (Screen) and push it into `session`.
///
/// Same shape as [`mediaway_encode_session_write_frame_from_camera_capture`], for
/// the Desktop/Screen capture handle instead of Camera. GPU frames pass through
/// Zero-Copy: the polled frame's `VideoFrameStorage::Gpu` handle moves straight
/// into `write_frame` with no CPU copy.
///
/// # Safety
///
/// `session`/`capture` must both be valid, live handle pointers.
/// `out_wrote_frame` must be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_encode_session_write_frame_from_desktop_capture(
    session: *mut EncodeSessionHandle,
    capture: *mut DesktopCaptureHandle,
    out_wrote_frame: *mut bool,
) -> MediawayPipelineStatus {
    if session.is_null() || capture.is_null() || out_wrote_frame.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees both handles are valid, live pointers (function
    // contract).
    let session = unsafe { &mut *session };
    let capture = unsafe { &mut *capture };
    if session.poisoned || capture.poisoned {
        return MediawayPipelineStatus::HandlePoisoned;
    }
    // SAFETY: `out_wrote_frame` is checked non-null above; caller guarantees it
    // is writable (function contract).
    unsafe { out_wrote_frame.write(false) };

    let poll_result = catch_unwind(AssertUnwindSafe(|| capture.inner.poll_frame()));
    let frame = match poll_result {
        Ok(Ok(Some(frame))) => frame,
        Ok(Ok(None)) => return MediawayPipelineStatus::Ok,
        Ok(Err(err)) => return err.into(),
        Err(_) => {
            capture.poisoned = true;
            return MediawayPipelineStatus::InternalPanic;
        }
    };

    let write_result = catch_unwind(AssertUnwindSafe(|| session.inner.write_frame(&frame)));
    // `adr/0005` §2: release the GPU frame slot unconditionally after the push
    // attempt, success or failure — never leave it held into the next poll.
    let release_result = catch_unwind(AssertUnwindSafe(|| capture.inner.release_frame()));

    let write_status = match write_result {
        Ok(Ok(())) => {
            // SAFETY: `out_wrote_frame` is checked non-null above (function contract).
            unsafe { out_wrote_frame.write(true) };
            MediawayPipelineStatus::Ok
        }
        Ok(Err(err)) => err.into(),
        Err(_) => {
            session.poisoned = true;
            MediawayPipelineStatus::InternalPanic
        }
    };
    match release_result {
        Ok(Ok(())) => write_status,
        Ok(Err(err)) => {
            if write_status == MediawayPipelineStatus::Ok {
                err.into()
            } else {
                write_status
            }
        }
        Err(_) => {
            capture.poisoned = true;
            if write_status == MediawayPipelineStatus::Ok {
                MediawayPipelineStatus::InternalPanic
            } else {
                write_status
            }
        }
    }
}
