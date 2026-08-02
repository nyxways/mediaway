//! Linux **window** capture: the same portal `ScreenCast` + `PipeWire`
//! plumbing as [`crate::screencast`], requesting
//! [`SourceType::Window`](ashpd::desktop::screencast::SourceType::Window)
//! instead of `Monitor`. See [ADR-0003](adr/0003-portal-window-capture.md).
//!
//! `xdg-desktop-portal`'s `ScreenCast` interface has carried a `Window`
//! source type bit since its first version — this is a real extension of the
//! existing screen-capture recipe, not a new subsystem: the portal's own
//! picker UI is what actually shows windows instead of monitors.

#![forbid(unsafe_code)]

use ashpd::desktop::screencast::SourceType;
use mediaway_common::{Bytes, CodecKind, Rational, StreamInfo, VideoFrame, VideoGeometry};
use mediaway_device::{CaptureError, CaptureSource, VideoCapture, VideoCaptureConfig};

use crate::screencast::{self, Session};

/// Linux window capture via the portal `ScreenCast` session
/// (`SourceType::Window`) and a `PipeWire` client stream.
///
/// See [`crate::screencast::LinuxScreenCapture`] for the shared Zero-Copy
/// status (CPU copy only this session) and **zero runtime hardware/session
/// verification** caveat, both identical here.
pub struct LinuxWindowCapture {
    inner: Option<Session>,
}

impl LinuxWindowCapture {
    /// Open a portal `ScreenCast` (`SourceType::Window`) + `PipeWire` session
    /// for `config`.
    ///
    /// The [`CaptureSource::Window`] `window` field is **ignored** — like
    /// `Screen`'s `output_index`, the portal's own picker UI chooses which
    /// window interactively; there is no programmatic "capture window with
    /// this handle" portal call the way `WGC`'s `CreateForWindow(HWND)` works
    /// on Windows. Any [`CaptureSource::Window`] value opens the picker.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for non-window sources or
    /// [`CaptureOutputPreference::ZeroCopyGpu`](mediaway_device::CaptureOutputPreference::ZeroCopyGpu)
    /// (see [`crate::screencast::LinuxScreenCapture`] docs). Returns other
    /// [`CaptureError`] variants when the portal handshake or `PipeWire`
    /// connection fails.
    pub fn open(config: &VideoCaptureConfig) -> Result<Self, CaptureError> {
        let CaptureSource::Window { window: _ } = config.source else {
            return Err(CaptureError::Unsupported);
        };
        let session = screencast::open_session(SourceType::Window, "Window", config)?;
        Ok(Self {
            inner: Some(session),
        })
    }
}

impl VideoCapture for LinuxWindowCapture {
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(inner) = self.inner.as_ref() {
            inner.stream_info()
        } else {
            closed_stream_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, CaptureError> {
        let inner = self.inner.as_ref().ok_or(CaptureError::Closed)?;
        inner.poll_frame()
    }

    fn release_frame(&mut self) -> Result<(), CaptureError> {
        if self.inner.is_none() {
            return Err(CaptureError::Closed);
        }
        Ok(())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        let Some(mut session) = self.inner.take() else {
            return Err(CaptureError::Closed);
        };
        session.close();
        Ok(())
    }
}

impl Drop for LinuxWindowCapture {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn closed_stream_info() -> &'static StreamInfo {
    use std::sync::OnceLock;
    static INFO: OnceLock<StreamInfo> = OnceLock::new();
    INFO.get_or_init(|| StreamInfo::Video {
        id: 0,
        codec: CodecKind::RawVideo,
        time_base: Rational::new(1, 30),
        geometry: VideoGeometry {
            width: 0,
            height: 0,
        },
        extra_data: Bytes::new(),
    })
}

#[cfg(test)]
#[path = "window_tests.rs"]
mod tests;
