//! Desktop audio capture (loopback / process-loopback) — a thin wrapper over
//! `mediaway-device-windows-audio`'s shared WASAPI engine
//! ([`mediaway_device_windows_audio::WindowsWasapiCapture`]).
//!
//! Loopback/process-loopback and microphone capture share one WASAPI capture-loop
//! implementation (`mediaway-device/adr/0007-domain-crate-split.md`'s "(b)" decision) — this
//! crate does not reimplement it, only translates [`DesktopAudioCaptureConfig`] into the
//! shared engine's internal config and delegates every [`DesktopAudioCapture`] method.

#![allow(unsafe_code)]

use mediaway_common::{AudioFrame, StreamInfo};
use mediaway_device::CaptureError;
use mediaway_device_desktop::{
    DesktopAudioCapture, DesktopAudioCaptureConfig, DesktopAudioSource, ProcessTreeScope,
};
use mediaway_device_windows_audio::{
    WasapiCaptureConfig, WasapiProcessTreeScope, WasapiSource, WindowsWasapiCapture,
};

/// [`DesktopAudioSource`]/[`ProcessTreeScope`] are `#[non_exhaustive]` — a future variant
/// this crate doesn't know about yet is rejected as [`CaptureError::Unsupported`], the same
/// "future variant, reject rather than mis-resolve" convention `wasapi.rs`'s own
/// `resolve_endpoint`/`Select` handling already follows.
fn to_wasapi_source(source: &DesktopAudioSource) -> Result<WasapiSource, CaptureError> {
    match source {
        DesktopAudioSource::Loopback { select } => Ok(WasapiSource::Loopback {
            select: select.clone(),
        }),
        DesktopAudioSource::ProcessLoopback {
            process_id,
            tree_scope,
        } => {
            let tree_scope = match tree_scope {
                ProcessTreeScope::ProcessOnly => WasapiProcessTreeScope::ProcessOnly,
                ProcessTreeScope::IncludeChildren => WasapiProcessTreeScope::IncludeChildren,
                _ => return Err(CaptureError::Unsupported),
            };
            Ok(WasapiSource::ProcessLoopback {
                process_id: *process_id,
                tree_scope,
            })
        }
        _ => Err(CaptureError::Unsupported),
    }
}

/// Windows desktop audio capture (loopback / process-loopback), via the shared WASAPI engine.
pub struct WindowsDesktopAudioCapture(WindowsWasapiCapture);

impl WindowsDesktopAudioCapture {
    /// Open desktop audio capture (loopback or process-loopback) for `config`.
    ///
    /// # Errors
    ///
    /// See `mediaway_device_windows_audio::WindowsWasapiCapture::open`.
    pub fn open(config: &DesktopAudioCaptureConfig) -> Result<Self, CaptureError> {
        let source = to_wasapi_source(&config.source)?;
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
