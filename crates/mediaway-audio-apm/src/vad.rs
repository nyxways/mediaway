//! [`VoiceActivityDetector`] — standalone RNN voice-activity detector, via
//! `sonora_agc2::vad_wrapper`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_common::{AudioFrame, SampleFormat};
use sonora_agc2::vad_wrapper::VoiceActivityDetectorWrapper;
use sonora_simd::detect_backend;

use crate::error::ApmError;
use crate::pcm::bytes_to_f32;

/// `sonora`'s RNN VAD is a port of WebRTC's internal detector, which assumes
/// i16-scale PCM (`±32768`) for its spectral-energy silence threshold.
/// [`AudioFrame`]'s realistic mic-capture content is `f32` normalized to
/// `[-1, 1]` — feeding it unscaled means spectral energy never crosses the
/// detector's internal silence threshold, so [`analyze`](VoiceActivityDetector::analyze)
/// would report `0.0` for every input, including real speech (see
/// `adr/0001-sonora-audio-processing-adoption.md` § 5 — the empirically
/// verified boundary: silence-classified up to amplitude `~1.0`, real
/// detection from `~10.0` and up). `analyze` applies this scale internally
/// so callers never see or manage it.
const SONORA_PCM_SCALE: f32 = 32768.0;

/// Standalone RNN voice-activity detector, via `sonora_agc2::vad_wrapper`.
///
/// Independent of [`AudioProcessor`](crate::AudioProcessor) — `sonora`'s own
/// AGC2 uses this VAD internally but does not expose it, so this type wraps
/// the standalone `sonora-agc2` crate directly.
#[derive(Debug)]
pub struct VoiceActivityDetector {
    /// `None` after a caught backend panic — see [`is_disabled`](Self::is_disabled).
    inner: Option<VoiceActivityDetectorWrapper>,
    /// Samples per 10ms mono block (`sample_rate / 100`) — the exact length
    /// [`analyze`](Self::analyze) requires.
    frame_len: usize,
    /// Reused scaled-mono scratch buffer, sized to `frame_len`.
    scratch: Vec<f32>,
}

impl VoiceActivityDetector {
    /// Opens a new detector for `sample_rate` (Hz).
    ///
    /// # Errors
    /// [`ApmError::BackendPanicked`] if `sonora-agc2` panicked while
    /// constructing the detector.
    pub fn open(sample_rate: u32) -> Result<Self, ApmError> {
        let backend = detect_backend();
        let rate_i32 = i32::try_from(sample_rate).unwrap_or(i32::MAX);
        let build_result = catch_unwind(AssertUnwindSafe(|| {
            VoiceActivityDetectorWrapper::new(backend, rate_i32)
        }));
        let inner = match build_result {
            Ok(vad) => Some(vad),
            Err(_) => return Err(ApmError::BackendPanicked),
        };

        let frame_len = usize::try_from(sample_rate / 100).unwrap_or(0);
        Ok(Self {
            inner,
            frame_len,
            scratch: vec![0.0; frame_len],
        })
    }

    /// Speech probability in `[0, 1]` for one 10ms frame.
    ///
    /// **Intended input is [`AudioProcessor::poll_processed_frame`](crate::AudioProcessor::poll_processed_frame)'s
    /// output** — already exactly-10ms-blocked and post-NS, matching
    /// `sonora`'s own validated usage pattern (AGC2's internal VAD consumes
    /// post-NS audio). `frame`'s first channel is analyzed (multi-channel
    /// frames are not downmixed). No internal re-blocking ring buffer here,
    /// by design — `frame` must carry exactly `sample_rate / 100` samples
    /// per channel or this returns [`ApmError::FrameLengthMismatch`]; note
    /// only frame *length* is validated against the configured rate, since
    /// that is what `sonora`'s block-size contract actually depends on.
    ///
    /// # Sonora i16-scale caveat
    /// `frame` is scaled ×32768.0 internally before reaching `sonora` — do
    /// not pre-scale it yourself (see the module docs / ADR § 5).
    ///
    /// # Disabled behavior
    /// Unlike [`AudioProcessor`](crate::AudioProcessor), which falls back to
    /// raw PCM passthrough once disabled, a scalar VAD score has no honest
    /// passthrough equivalent — synthesizing a fixed probability (e.g.
    /// always `0.0`) would be silently and dangerously wrong for a caller
    /// gating on it (this mirrors why this workspace's `*-ffi`
    /// poisoned-handle precedent errors forever specifically when further
    /// output would be silently wrong, rather than degrading). Once
    /// disabled, every call returns [`ApmError::BackendPanicked`] —
    /// construct a new instance to retry.
    ///
    /// # Errors
    /// See above: [`ApmError::UnsupportedSampleFormat`],
    /// [`ApmError::FrameLengthMismatch`], or [`ApmError::BackendPanicked`].
    pub fn analyze(&mut self, frame: &AudioFrame) -> Result<f32, ApmError> {
        let Some(inner) = self.inner.as_mut() else {
            return Err(ApmError::BackendPanicked);
        };

        if frame.format != SampleFormat::F32 {
            return Err(ApmError::UnsupportedSampleFormat(frame.format));
        }

        let interleaved = bytes_to_f32(&frame.data);
        let channels = usize::from(frame.channels).max(1);
        let actual_frames = interleaved.len() / channels;
        if actual_frames != self.frame_len {
            return Err(ApmError::FrameLengthMismatch {
                expected: self.frame_len,
                actual: actual_frames,
            });
        }

        let scratch = &mut self.scratch;
        for (dst, chunk) in scratch.iter_mut().zip(interleaved.chunks(channels)) {
            *dst = chunk[0] * SONORA_PCM_SCALE;
        }

        let result = catch_unwind(AssertUnwindSafe(|| inner.analyze(scratch)));
        if let Ok(probability) = result {
            Ok(probability)
        } else {
            self.inner = None;
            Err(ApmError::BackendPanicked)
        }
    }

    /// `true` after a caught backend panic — every subsequent
    /// [`analyze`](Self::analyze) call returns
    /// [`ApmError::BackendPanicked`]. Construct a new instance to retry.
    #[must_use]
    pub const fn is_disabled(&self) -> bool {
        self.inner.is_none()
    }
}

#[cfg(test)]
#[path = "vad_tests.rs"]
mod tests;
