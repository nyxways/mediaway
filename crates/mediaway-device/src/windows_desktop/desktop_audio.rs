//! Desktop audio capture (loopback / process-loopback) — a thin wrapper over
//! `mediaway-device-windows-audio`'s shared WASAPI engine
//! ([`crate::windows_audio::WindowsWasapiCapture`]).
//!
//! Loopback/process-loopback and microphone capture share one WASAPI capture-loop
//! implementation (`mediaway-device/adr/0007-domain-crate-split.md`'s "(b)" decision) — this
//! crate does not reimplement it, only translates [`DesktopAudioCaptureConfig`] into the
//! shared engine's internal config and delegates every [`DesktopAudioCapture`] method.

#![allow(unsafe_code)]

use crate::CaptureError;
use crate::desktop::{
    DesktopAudioCapture, DesktopAudioCaptureConfig, DesktopAudioSource, ProcessTreeScope,
};
use crate::windows_audio::{
    WasapiCaptureConfig, WasapiProcessTreeScope, WasapiSource, WindowsWasapiCapture,
};
use mediaway_common::{AudioFrame, StreamInfo};

/// [`DesktopAudioSource`]/[`ProcessTreeScope`] are `#[non_exhaustive]` — the exhaustive
/// matches below mean a future variant becomes a compile error here (same-crate match,
/// ADR-0021 merge), forcing an explicit review rather than silent mis-resolution.
fn to_wasapi_source(source: &DesktopAudioSource) -> WasapiSource {
    match source {
        DesktopAudioSource::Loopback { select } => WasapiSource::Loopback {
            select: select.clone(),
        },
        DesktopAudioSource::ProcessLoopback {
            process_id,
            tree_scope,
        } => {
            let tree_scope = match tree_scope {
                ProcessTreeScope::ProcessOnly => WasapiProcessTreeScope::ProcessOnly,
                ProcessTreeScope::IncludeChildren => WasapiProcessTreeScope::IncludeChildren,
            };
            WasapiSource::ProcessLoopback {
                process_id: *process_id,
                tree_scope,
            }
        }
    }
}

/// Windows desktop audio capture (loopback / process-loopback), via the shared WASAPI engine.
pub struct WindowsDesktopAudioCapture(WindowsWasapiCapture);

impl WindowsDesktopAudioCapture {
    /// Open desktop audio capture (loopback or process-loopback) for `config`.
    ///
    /// # Errors
    ///
    /// See `crate::windows_audio::WindowsWasapiCapture::open`.
    pub fn open(config: &DesktopAudioCaptureConfig) -> Result<Self, CaptureError> {
        let source = to_wasapi_source(&config.source);
        let inner = WindowsWasapiCapture::open(&WasapiCaptureConfig {
            source,
            time_base: config.time_base,
            sample_format: config.sample_format,
        })?;
        Ok(Self(inner))
    }
}

impl DesktopAudioCapture for WindowsDesktopAudioCapture {
    fn stream_info(&self) -> &StreamInfo {
        self.0.stream_info()
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, CaptureError> {
        self.0.poll_frame()
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        self.0.close()
    }
}
