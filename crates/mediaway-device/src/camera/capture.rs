//! Camera capture config and [`CameraCapture`] trait.
//!
//! Split out of `mediaway-device`'s former unified `video.rs` — see
//! `mediaway-device/adr/0007-domain-crate-split.md`. Structurally close to
//! `mediaway-device-desktop`'s `DesktopVideoCapture` (same method shapes), kept as a
//! separate trait rather than a shared one so this crate can grow camera-specific methods
//! (e.g. focus/exposure control) later without that leaking into desktop capture's surface.

#![forbid(unsafe_code)]

use crate::{CaptureError, Select};
use mediaway_common::{GpuDeviceHandle, Rational, StreamInfo, VideoFrame, VideoFrameStorage};

/// How the caller prefers to receive frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum CaptureOutputPreference {
    /// Prefer GPU handles ([`mediaway_common::VideoFrameStorage::Gpu`]).
    #[default]
    ZeroCopyGpu,
    /// Accept CPU frames (may readback — backends must document cost).
    CpuFramesOk,
}

/// Parameters for opening a camera capture session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraCaptureConfig {
    /// Which camera device to open (`Select::Default` = ordinal `0`).
    pub select: Select,
    /// Timestamp timebase for polled frames.
    pub time_base: Rational,
    /// Output path preference (Zero-Copy vs CPU). Every shipped backend today rejects
    /// [`CaptureOutputPreference::ZeroCopyGpu`] (`CaptureError::Unsupported`) — see each
    /// backend's own module doc.
    pub output: CaptureOutputPreference,
    /// GPU device handle for Zero-Copy capture when [`CaptureOutputPreference::ZeroCopyGpu`].
    pub gpu_device: Option<GpuDeviceHandle>,
}

impl CameraCaptureConfig {
    /// Default camera capture. Prefer setting fields explicitly in apps.
    #[must_use]
    pub const fn default_camera(time_base: Rational) -> Self {
        Self {
            select: Select::Default,
            time_base,
            output: CaptureOutputPreference::ZeroCopyGpu,
            gpu_device: None,
        }
    }
}

/// Streaming camera capture (poll frames; release GPU resources explicitly).
///
/// Typical loop: [`poll_frame`](CameraCapture::poll_frame) → consume (e.g. encode
/// `push_frame`) → [`release_frame`](CameraCapture::release_frame) before the next poll.
pub trait CameraCapture {
    /// Stream metadata (size / timebase; codec is typically raw).
    fn stream_info(&self) -> &StreamInfo;

    /// Pull the next frame if ready. `Ok(None)` = no new frame yet (timeout / idle).
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure or access loss.
    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, CaptureError>;

    /// Release backend resources for the last GPU frame.
    ///
    /// No-op when the last frame was CPU-owned or none was held. Must be called
    /// before the next successful [`poll_frame`](Self::poll_frame) that acquires again.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure.
    fn release_frame(&mut self) -> Result<(), CaptureError>;

    /// End the session and free OS resources.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure.
    fn close(&mut self) -> Result<(), CaptureError>;

    /// Block the calling thread until the next frame is ready or `timeout` elapses.
    ///
    /// Default implementation retries [`poll_frame`](Self::poll_frame), sleeping a short
    /// bounded interval between empty (`Ok(None)`) results, until a frame arrives or the
    /// deadline passes.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Timeout`] if `timeout` elapses with no frame. Otherwise
    /// propagates the same errors as `poll_frame`.
    fn capture_next_frame_blocking(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<VideoFrame, CaptureError> {
        /// Sleep interval between empty polls — bounded so a non-blocking `poll_frame`
        /// (Camera's queue pop) does not busy-spin.
        const RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(4);

        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(frame) = self.poll_frame()? {
                return Ok(frame);
            }
            if std::time::Instant::now() >= deadline {
                return Err(CaptureError::Timeout);
            }
            std::thread::sleep(RETRY_INTERVAL.min(deadline - std::time::Instant::now()));
        }
    }
}

/// Open a session via `open`, block for one frame, then release and close —
/// a convenience for "I don't want to manage a session" callers.
///
/// **Pays a full session-open cost on every call** — do not call this in a loop to build a
/// recorder; use an already-open [`CameraCapture`] session's
/// [`poll_frame`](CameraCapture::poll_frame)/
/// [`capture_next_frame_blocking`](CameraCapture::capture_next_frame_blocking) instead.
///
/// # Errors
///
/// Propagates `open`'s errors, or [`CaptureError::Timeout`] from
/// [`capture_next_frame_blocking`](CameraCapture::capture_next_frame_blocking). Returns
/// [`CaptureError::Unsupported`] instead of the captured frame when its storage is
/// [`VideoFrameStorage::Gpu`] — this function closes the session before returning, and a
/// GPU-backed frame's handle would dangle past that point.
pub fn capture_camera_once<C: CameraCapture>(
    open: impl FnOnce() -> Result<C, CaptureError>,
    timeout: std::time::Duration,
) -> Result<VideoFrame, CaptureError> {
    let mut session = open()?;
    let result = session.capture_next_frame_blocking(timeout);
    let _ = session.release_frame();
    let _ = session.close();
    match result {
        Ok(frame) if matches!(frame.storage, VideoFrameStorage::Gpu(_)) => {
            Err(CaptureError::Unsupported)
        }
        other => other,
    }
}
