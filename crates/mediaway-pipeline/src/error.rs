//! [`PipelineError`] — unifies encoder + mux failures behind one type.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Error from an [`crate::EncodeSession`] operation.
///
/// Does **not** derive `Clone + PartialEq + Eq` (unlike this crate's earlier shape) —
/// [`ApmError`](Self::Apm) wraps `mediaway_audio_apm::ApmError`, which itself wraps an
/// external `sonora::Error` (`#[source]`, no `Clone`/`PartialEq` upstream) and cannot
/// honestly support them either. See `adr/0003-audio-track-and-apm-integration.md`
/// § Consequences.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PipelineError {
    /// Encoder session failure.
    #[error("encoder error: {0}")]
    Encode(#[from] mediaway_encoder::EncodeError),
    /// Container mux failure.
    #[error("mux error: {0}")]
    Mux(#[from] mediaway_container::mp4::Error),
    /// Frame filter chain failure.
    #[error("filter error: {0}")]
    Filter(#[from] crate::filter::FilterError),
    /// Audio enhancement (AEC/NS/AGC/VAD) failure — see
    /// `adr/0003-audio-track-and-apm-integration.md`.
    #[error("audio processing error: {0}")]
    Apm(#[from] mediaway_audio_apm::ApmError),
    /// [`crate::EncodeSession::attach_audio_processor`]/[`crate::EncodeSession::attach_vad`]/
    /// [`crate::EncodeSession::write_audio_frame`]/[`crate::EncodeSession::write_audio_render_frame`]
    /// called on a session opened via [`crate::EncodeSession::open`] (video-only) instead
    /// of [`crate::EncodeSession::open_with_audio`].
    #[error("no audio track attached to this session — use EncodeSession::open_with_audio")]
    NoAudioTrack,
}
