//! Desktop audio capture config (loopback / process-loopback — "what's playing") and
//! [`DesktopAudioCapture`] trait.
//!
//! Grouped with screen/window video capture (this module), not with [`crate::audio`]'s
//! microphone/playback "Audio I/O", because loopback/process-loopback capture *what the
//! desktop is already rendering* — a desktop-capture concept, not a real audio input/output
//! device.

#![forbid(unsafe_code)]

use crate::{CaptureError, Select};
use mediaway_common::{AudioFrame, Rational, SampleFormat, StreamInfo};

/// Desktop audio capture source selection.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DesktopAudioSource {
    /// Default (or specific) render endpoint opened with WASAPI loopback.
    Loopback {
        /// Which render endpoint to open in loopback mode
        /// (`Select::Default` = default console render).
        select: Select,
    },
    /// Per-process WASAPI loopback (`VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`).
    ///
    /// Windows 10 2004+; capture is IEEE float at a fixed 48 kHz stereo layout on
    /// the Windows backend (mix-format queries are unsupported for this mode).
    ProcessLoopback {
        /// The process `tree_scope` is applied to — captured under
        /// [`ProcessTreeScope::IncludeChildren`], excluded under
        /// [`ProcessTreeScope::ExcludeProcessTree`].
        process_id: u32,
        /// Which of the two process-loopback modes to use.
        tree_scope: ProcessTreeScope,
    },
}

/// Which process tree a [`DesktopAudioSource::ProcessLoopback`] capture records.
///
/// Windows offers exactly two process-loopback modes, and this enum is a 1:1 image of
/// them — **there is no "the target process but not its children" mode**, so this enum
/// deliberately has no variant for one. A variant that claimed to be that mode existed
/// until 2026-09-18 (`ProcessOnly`); it selected
/// `PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE`, the exact inverse of its own
/// documentation, and recorded everything *but* the target without failing. See
/// `mediaway-device/adr/windows/0002-wasapi-capture.md` § Correction (2026-09-18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProcessTreeScope {
    /// Audio rendered by the target process and its descendants
    /// (`PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE`).
    IncludeChildren,
    /// Everything the desktop renders **except** the target process and its descendants
    /// (`PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE`).
    ///
    /// This is a "record everything else" mode, not a narrowing of
    /// [`Self::IncludeChildren`]: the target process is the one thing it leaves out. Its
    /// use case is a recorder capturing system audio without its own playback feeding
    /// back into the capture.
    ExcludeProcessTree,
}

/// Parameters for opening a desktop audio capture session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopAudioCaptureConfig {
    /// What to capture.
    pub source: DesktopAudioSource,
    /// Timestamp timebase for polled frames (often `1 / sample_rate`).
    pub time_base: Rational,
    /// Preferred PCM format when conversion is required (`F32` matches modern WASAPI mix).
    pub sample_format: SampleFormat,
}

impl DesktopAudioCaptureConfig {
    /// Default system loopback. Prefer setting fields explicitly in apps.
    #[must_use]
    pub const fn loopback(time_base: Rational) -> Self {
        Self {
            source: DesktopAudioSource::Loopback {
                select: Select::Default,
            },
            time_base,
            sample_format: SampleFormat::F32,
        }
    }

    /// Per-process loopback. Prefer setting fields explicitly in apps.
    #[must_use]
    pub const fn process_loopback(
        process_id: u32,
        tree_scope: ProcessTreeScope,
        time_base: Rational,
    ) -> Self {
        Self {
            source: DesktopAudioSource::ProcessLoopback {
                process_id,
                tree_scope,
            },
            time_base,
            sample_format: SampleFormat::F32,
        }
    }
}

/// Streaming desktop audio capture — poll PCM frames (worker may fill a bounded queue).
pub trait DesktopAudioCapture {
    /// Stream metadata — `StreamInfo::Audio` with real `sample_rate`/
    /// `channels`, `codec: CodecKind::RawAudio` for uncompressed PCM.
    fn stream_info(&self) -> &StreamInfo;

    /// Pull the next PCM chunk if ready. `Ok(None)` = no samples yet.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure.
    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, CaptureError>;

    /// End the session and free OS resources.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure.
    fn close(&mut self) -> Result<(), CaptureError>;
}
