//! Internal WASAPI capture config — shared by every audio-domain source (microphone,
//! render-endpoint loopback, process loopback) since
//! [`crate::windows_audio::WindowsWasapiCapture`]'s engine is one shared implementation,
//! wrapped per domain rather than duplicated.
//!
//! Structurally identical to the top-level `AudioCaptureConfig`/`AudioCaptureSource`/
//! `ProcessTreeScope`. [`crate::windows_audio`] (microphone) and [`crate::windows_desktop`]
//! (loopback/process-loopback) each translate their own public, domain-narrowed config
//! type into this one before calling [`crate::windows_audio::WindowsWasapiCapture::open`].

#![allow(unsafe_code)]

use crate::Select;
use mediaway_common::{Rational, SampleFormat};

/// Audio capture endpoint selection — every source [`crate::windows_audio::WindowsWasapiCapture`] can open.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WasapiSource {
    /// Default (or specific) microphone / capture endpoint.
    Microphone {
        /// Which capture endpoint to open (`Select::Default` = default console capture).
        select: Select,
    },
    /// Default (or specific) render endpoint opened with WASAPI loopback.
    Loopback {
        /// Which render endpoint to open in loopback mode
        /// (`Select::Default` = default console render).
        select: Select,
    },
    /// Per-process WASAPI loopback (`VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`).
    ProcessLoopback {
        /// The process the mode is applied to — captured in
        /// [`WasapiProcessTreeScope::IncludeChildren`], excluded in
        /// [`WasapiProcessTreeScope::ExcludeProcessTree`].
        process_id: u32,
        /// Which of the two Windows process-loopback modes to activate.
        tree_scope: WasapiProcessTreeScope,
    },
}

/// Which `PROCESS_LOOPBACK_MODE` a [`WasapiSource::ProcessLoopback`] capture activates.
///
/// Mirrors `crate::desktop::ProcessTreeScope`; see it for why there is no
/// "target process alone" variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WasapiProcessTreeScope {
    /// `PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE` — audio rendered by the target
    /// process and its descendants.
    IncludeChildren,
    /// `PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE` — everything the desktop renders
    /// **except** the target process and its descendants.
    ExcludeProcessTree,
}

/// Parameters for opening a [`crate::windows_audio::WindowsWasapiCapture`] session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasapiCaptureConfig {
    /// What to capture.
    pub source: WasapiSource,
    /// Timestamp timebase for polled frames (often `1 / sample_rate`).
    pub time_base: Rational,
    /// Preferred PCM format when conversion is required (`F32` matches modern WASAPI mix).
    pub sample_format: SampleFormat,
}
