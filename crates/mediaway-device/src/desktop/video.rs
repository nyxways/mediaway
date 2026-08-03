//! Desktop video capture config (screen/window) and [`DesktopVideoCapture`] trait.
//!
//! Split out of `mediaway-device`'s former unified `video.rs` — see
//! `mediaway-device/adr/0007-domain-crate-split.md`. Structurally close to
//! `mediaway-device-camera`'s `CameraCapture` (same method shapes), kept as a separate
//! trait so each domain can grow independently.

#![forbid(unsafe_code)]

use crate::{CaptureError, Select};
use mediaway_common::{
    GpuDeviceHandle, NativeHandle, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
};

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

/// Desktop capture source selection.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DesktopCaptureSource {
    /// DXGI Desktop Duplication / display output.
    Screen {
        /// Which adapter output to open (`Select::Default` = primary,
        /// ordinal `0`). Resolved only among the outputs of the adapter
        /// that owns the config's `gpu_device` — a `Select::Id` naming an
        /// output on a different adapter is `CaptureError::InvalidInput`,
        /// not a global cross-adapter search.
        select: Select,
    },
    /// Window capture (platform-specific; may be unsupported).
    Window {
        /// Opaque HWND / window token as a native handle (never null/unset by construction).
        window: NativeHandle,
    },
}

/// Parameters for opening a desktop video capture session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopVideoCaptureConfig {
    /// What to capture.
    pub source: DesktopCaptureSource,
    /// Timestamp timebase for polled frames.
    pub time_base: Rational,
    /// Output path preference (Zero-Copy vs CPU).
    pub output: CaptureOutputPreference,
    /// GPU device handle for Zero-Copy capture when [`CaptureOutputPreference::ZeroCopyGpu`].
    ///
    /// Must own the adapter that serves the chosen output (`None` = unset → Zero-Copy open fails).
    pub gpu_device: Option<GpuDeviceHandle>,
}

impl DesktopVideoCaptureConfig {
    /// Screen capture for `select` (`Select::Default` = primary output).
    /// Prefer setting fields explicitly in apps.
    #[must_use]
    pub const fn screen(select: Select, time_base: Rational) -> Self {
        Self {
            source: DesktopCaptureSource::Screen { select },
            time_base,
            output: CaptureOutputPreference::ZeroCopyGpu,
            gpu_device: None,
        }
    }

    /// Window capture for opaque `HWND` / window token bits. Prefer setting fields explicitly.
    #[must_use]
    pub const fn window(window: NativeHandle, time_base: Rational) -> Self {
        Self {
            source: DesktopCaptureSource::Window { window },
            time_base,
            output: CaptureOutputPreference::ZeroCopyGpu,
            gpu_device: None,
        }
    }
}

/// Streaming desktop video capture (poll frames; release GPU resources explicitly).
///
/// Typical loop: [`poll_frame`](DesktopVideoCapture::poll_frame) → consume (e.g. encode
/// `push_frame`) → [`release_frame`](DesktopVideoCapture::release_frame) before the next poll.
pub trait DesktopVideoCapture {
    /// Stream metadata (size / timebase; codec is typically raw).
    fn stream_info(&self) -> &StreamInfo;

    /// Pull the next frame if ready. `Ok(None)` = no new frame yet (timeout / idle).
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure or access loss.
    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, CaptureError>;

    /// Release backend resources for the last GPU frame (e.g. DXGI `ReleaseFrame`).
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
    /// For most backends this frees the OS resource immediately. **Exception:**
    /// a handle returned by a *shared* capture session (see
    /// [`mediaway-device-windows` ADR-0006](https://github.com/nyxways/mediaway/blob/main/crates/mediaway-device-windows/adr/0006-shared-desktop-duplication.md))
    /// only releases this caller's interest — the underlying OS resource is
    /// freed only once every attached handle has closed.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure.
    fn close(&mut self) -> Result<(), CaptureError>;

    /// Block the calling thread until the next frame is ready or `timeout` elapses.
    ///
    /// **On an already-open session, a returned [`CaptureError::Timeout`] is
    /// not necessarily a failure** — for delta-based backends (DXGI Desktop
    /// Duplication) it may legitimately mean "nothing changed since the last
    /// released frame," not a backend error. Callers wanting a guaranteed
    /// always-fresh image regardless of whether content changed should use
    /// [`capture_desktop_video_once`] (a fresh session's first frame is
    /// always a full baseline image, not delta-gated) instead of retrying
    /// this on a long-lived session.
    ///
    /// Default implementation retries [`poll_frame`](Self::poll_frame),
    /// sleeping a short bounded interval between empty (`Ok(None)`) results,
    /// until a frame arrives or the deadline passes.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Timeout`] if `timeout` elapses with no frame.
    /// Otherwise propagates the same errors as `poll_frame`.
    fn capture_next_frame_blocking(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<VideoFrame, CaptureError> {
        /// Sleep interval between empty polls — bounded so a non-blocking
        /// `poll_frame` (WGC's `TryGetNextFrame`) does not busy-spin; short
        /// enough not to meaningfully add to a blocking backend's own
        /// internal wait (DXGI's `AcquireNextFrame`, ~16 ms).
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
/// a convenience for "I don't want to manage a session" callers (e.g. a
/// hotkey screenshot command).
///
/// **Pays a full session-open cost on every call** — for
/// [`DesktopCaptureSource::Screen`] that includes DXGI `DuplicateOutput`'s real
/// driver-level setup. **Do not** call this in a loop to build a recorder —
/// use an already-open [`DesktopVideoCapture`] session's
/// [`poll_frame`](DesktopVideoCapture::poll_frame) or
/// [`capture_next_frame_blocking`](DesktopVideoCapture::capture_next_frame_blocking)
/// instead.
///
/// **Does not support GPU-backed frames** ([`VideoFrameStorage::Gpu`], e.g.
/// [`DesktopCaptureSource::Screen`]'s normal output): this function closes the
/// session before returning, and for a shared session (`mediaway-device-windows`
/// ADR-0006) closing the last attached consumer tears down the backend
/// resource the frame's GPU handle points to. Callers that need a GPU-backed
/// frame must keep the session open themselves.
///
/// # Errors
///
/// Propagates `open`'s errors, or [`CaptureError::Timeout`] from
/// [`capture_next_frame_blocking`](DesktopVideoCapture::capture_next_frame_blocking).
/// Returns [`CaptureError::Unsupported`] instead of the captured frame when
/// its storage is [`VideoFrameStorage::Gpu`] (see above).
pub fn capture_desktop_video_once<C: DesktopVideoCapture>(
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

#[cfg(test)]
#[path = "video_tests.rs"]
mod tests;
