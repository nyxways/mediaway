//! [`EncodeSession`] — encoder + muxer composition (video, + optional audio track).

#![forbid(unsafe_code)]

use std::collections::VecDeque;

use crate::error::PipelineError;
use crate::filter::{FilterError, FrameFilter};
use mediaway_common::{AudioFrame, VideoFrame, VideoFrameStorage};
use mediaway_container::mp4;
use mediaway_encoder::{AudioEncoder, VideoEncoder};
use mediaway_sw::apm::{AudioProcessor, VoiceActivityDetector};
use smallvec::SmallVec;

/// The optional audio side of an [`EncodeSession`] — present only when opened via
/// [`EncodeSession::open_with_audio`]. See
/// `adr/0003-audio-track-and-apm-integration.md`.
struct AudioTrack {
    encoder: Box<dyn AudioEncoder>,
    track_id: u32,
    /// AEC3 + NS + AGC2, if [`EncodeSession::attach_audio_processor`] was called.
    processor: Option<AudioProcessor>,
    /// RNN voice-activity detector, if [`EncodeSession::attach_vad`] was called.
    vad: Option<VoiceActivityDetector>,
    /// One score per processed 10ms block, drained by
    /// [`EncodeSession::poll_vad_score`].
    vad_scores: VecDeque<f32>,
}

/// Encode frames straight to fragmented MP4 bytes.
///
/// Wraps one [`VideoEncoder`] + an [`mp4::Muxer`] (single-track, or two-track when
/// opened via [`open_with_audio`](Self::open_with_audio)), draining `poll_packet` into
/// the muxer on every [`write_frame`](Self::write_frame)/
/// [`write_audio_frame`](Self::write_audio_frame) call instead of making callers write
/// that loop themselves.
///
/// Generic over `E` — works with a concrete unboxed encoder (e.g. Windows
/// `AutoVideoEncoder`) or `Box<dyn VideoEncoder>` (cross-platform dispatch via
/// [`crate::platform::AutoEncoder::open`]) without imposing a `Box` where the
/// caller doesn't already have one. The audio encoder is always `Box<dyn AudioEncoder>`
/// internally, regardless of `E` — see `adr/0003-audio-track-and-apm-integration.md`
/// § Struct shape for why this does not become a second generic parameter.
pub struct EncodeSession<E: VideoEncoder> {
    encoder: E,
    muxer: mp4::Muxer<mp4::Live>,
    track_id: u32,
    filters: SmallVec<[Box<dyn FrameFilter>; 4]>,
    audio: Option<AudioTrack>,
}

impl<E: VideoEncoder> EncodeSession<E> {
    /// Register `encoder`'s stream as an MP4 track and begin streaming. Video-only —
    /// see [`open_with_audio`](Self::open_with_audio) for a session with an audio track.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] if the muxer rejects the encoder's stream info.
    pub fn open(encoder: E) -> Result<Self, PipelineError> {
        let mut open = mp4::Muxer::new();
        let track_id = open.add_track(encoder.stream_info().clone())?;
        Ok(Self {
            encoder,
            muxer: open.begin(),
            track_id,
            filters: SmallVec::new(),
            audio: None,
        })
    }

    /// Register `encoder`'s and `audio_encoder`'s streams as MP4 tracks (video first,
    /// then audio) and begin streaming.
    ///
    /// Both tracks must be known before this call: [`mp4::Muxer`] is typestate — tracks
    /// can only be added before `begin()`, so there is no way to add an audio track to
    /// a session already opened via [`open`](Self::open)
    /// (`adr/0003-audio-track-and-apm-integration.md` § Context).
    ///
    /// `mp4::Muxer::add_track` requires unique track ids and rejects a duplicate with
    /// [`mediaway_container::mp4::Error::InvalidTrack`] — two independently constructed
    /// encoders both typically report `id: 0` by default (unlike
    /// [`open`](Self::open)'s single-track case, where that default is never a
    /// conflict). This renumbers explicitly (video `0`, audio `1`) rather than trusting
    /// each encoder's own default — the same renumbering `tests/screen_mic_av_smoke.rs`
    /// used to do by hand via `StreamInfo::with_id` before migrating onto this
    /// constructor.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] if the muxer rejects either encoder's stream info.
    pub fn open_with_audio(
        encoder: E,
        audio_encoder: impl AudioEncoder + 'static,
    ) -> Result<Self, PipelineError> {
        let mut open = mp4::Muxer::new();
        let track_id = open.add_track(encoder.stream_info().clone().with_id(0))?;
        let audio_track_id = open.add_track(audio_encoder.stream_info().clone().with_id(1))?;
        Ok(Self {
            encoder,
            muxer: open.begin(),
            track_id,
            filters: SmallVec::new(),
            audio: Some(AudioTrack {
                encoder: Box::new(audio_encoder),
                track_id: audio_track_id,
                processor: None,
                vad: None,
                vad_scores: VecDeque::new(),
            }),
        })
    }

    /// Attach AEC3 + NS + AGC2 audio enhancement — subsequent
    /// [`write_audio_frame`](Self::write_audio_frame) calls push through `processor`
    /// before reaching the audio encoder. Replaces a previously attached processor, if
    /// any.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::NoAudioTrack`] if this session was opened via
    /// [`open`](Self::open) (no audio track to attach to) — use
    /// [`open_with_audio`](Self::open_with_audio) instead.
    pub fn attach_audio_processor(
        &mut self,
        processor: AudioProcessor,
    ) -> Result<&mut Self, PipelineError> {
        let audio = self.audio.as_mut().ok_or(PipelineError::NoAudioTrack)?;
        audio.processor = Some(processor);
        Ok(self)
    }

    /// Attach an RNN voice-activity detector — subsequent
    /// [`write_audio_frame`](Self::write_audio_frame) calls score each processed 10ms
    /// block, retrievable via [`poll_vad_score`](Self::poll_vad_score). Replaces a
    /// previously attached detector, if any.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::NoAudioTrack`] if this session was opened via
    /// [`open`](Self::open) (no audio track to attach to) — use
    /// [`open_with_audio`](Self::open_with_audio) instead.
    pub fn attach_vad(&mut self, vad: VoiceActivityDetector) -> Result<&mut Self, PipelineError> {
        let audio = self.audio.as_mut().ok_or(PipelineError::NoAudioTrack)?;
        audio.vad = Some(vad);
        Ok(self)
    }

    /// Append a filter to the chain (runs after previously pushed filters).
    /// Filters may be pushed at any point before or between `write_frame` calls.
    pub fn push_filter<F: FrameFilter>(&mut self, filter: F) -> &mut Self {
        self.filters.push(Box::new(filter));
        self
    }

    /// Push one frame and drain any packets it produces into the muxer.
    ///
    /// Frames pass through the [`push_filter`](Self::push_filter) chain (if any)
    /// before reaching the encoder. An empty chain costs nothing beyond one
    /// `is_empty()` check. A non-empty chain rejects `Gpu`-backed frames with
    /// [`FilterError::GpuFrameUnsupported`] — v1 filters are CPU-frame-only
    /// (see [ADR-0001](../../adr/0001-frame-filter-hook.md)).
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] on filter, encoder, or mux failure.
    pub fn write_frame(&mut self, frame: &VideoFrame) -> Result<(), PipelineError> {
        if self.filters.is_empty() {
            self.encoder.push_frame(frame)?; // unchanged fast path, zero clone
        } else {
            if matches!(frame.storage, VideoFrameStorage::Gpu(_)) {
                return Err(PipelineError::Filter(FilterError::GpuFrameUnsupported));
            }
            // clone: entry point into an owned filter chain — the caller only lent a
            // reference, but VideoFrame::clone() is a Bytes refcount bump (Cpu) or a
            // Copy of a small handle (Gpu, unreachable here), never a pixel memcpy.
            // Paid exactly once per frame, only when a filter chain is attached.
            let mut current = frame.clone();
            for filter in &mut self.filters {
                current = filter.process(current)?;
            }
            self.encoder.push_frame(&current)?;
        }
        self.drain()
    }

    /// Retarget the live CBR bitrate ceiling on the underlying encoder — see
    /// [`mediaway_encoder::VideoEncoder::set_bitrate`]. No session reopen, no dropped
    /// frames; takes effect from the next [`write_frame`](Self::write_frame) call.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::Encode`] if the underlying encoder was not opened in
    /// CBR mode or cannot retarget bitrate live (`EncodeError::Unsupported`), or on a
    /// backend failure.
    pub fn set_bitrate(&mut self, bitrate_bps: u32) -> Result<(), PipelineError> {
        self.encoder.set_bitrate(bitrate_bps)?;
        Ok(())
    }

    /// Push one microphone/capture-side audio frame and drain any packets it produces
    /// into the muxer.
    ///
    /// With no [`attach_audio_processor`](Self::attach_audio_processor) call, `frame`
    /// goes straight to the audio encoder (the fast path `tests/screen_mic_av_smoke.rs`
    /// now exercises via this method). With a processor attached, `frame` is pushed into it and every
    /// resulting processed 10ms block (zero, one, or several — `AudioProcessor`
    /// re-blocks internally) is scored by [`attach_vad`](Self::attach_vad)'s detector,
    /// if any, then pushed to the audio encoder. See
    /// `adr/0003-audio-track-and-apm-integration.md` § The write path.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::NoAudioTrack`] if this session was opened via
    /// [`open`](Self::open). Returns [`PipelineError::Apm`] on a transient
    /// `AudioProcessor` failure (the instance keeps working in degraded/passthrough
    /// mode afterward — see that error variant's docs). Otherwise returns
    /// [`PipelineError`] on encoder or mux failure. A [`VoiceActivityDetector`] failure
    /// is **not** propagated here — see [`poll_vad_score`](Self::poll_vad_score).
    pub fn write_audio_frame(&mut self, frame: &AudioFrame) -> Result<(), PipelineError> {
        let Self { audio, muxer, .. } = self;
        let Some(audio) = audio.as_mut() else {
            return Err(PipelineError::NoAudioTrack);
        };

        if let Some(processor) = audio.processor.as_mut() {
            processor.push_capture_frame(frame)?;
            while let Some(block) = processor.poll_processed_frame()? {
                if let Some(vad) = audio.vad.as_mut() {
                    // A disabled VAD's `analyze` errors forever (no honest scalar
                    // passthrough) — that must not block audio encoding, only stop
                    // producing new scores. See `adr/0003-audio-track-and-apm-integration.md`
                    // § Error handling.
                    if let Ok(score) = vad.analyze(&block) {
                        audio.vad_scores.push_back(score);
                    }
                }
                audio.encoder.push_frame(&block)?;
            }
        } else {
            audio.encoder.push_frame(frame)?;
        }
        Self::drain_audio(audio, muxer)
    }

    /// Feed a render-reference (far-end / about-to-be-played) frame to the attached
    /// [`AudioProcessor`], if any — the echo-cancellation reference signal
    /// (`AudioProcessor::push_render_frame`). Only meaningful for a caller that is also
    /// playing audio back (e.g. voice chat); a pure recorder never needs this.
    ///
    /// A no-op when this session has an audio track but no processor attached — there
    /// is nothing to feed the reference into.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::NoAudioTrack`] if this session was opened via
    /// [`open`](Self::open). Returns [`PipelineError::Apm`] on a transient
    /// `AudioProcessor` failure.
    pub fn write_audio_render_frame(&mut self, frame: &AudioFrame) -> Result<(), PipelineError> {
        let Some(audio) = self.audio.as_mut() else {
            return Err(PipelineError::NoAudioTrack);
        };
        if let Some(processor) = audio.processor.as_mut() {
            processor.push_render_frame(frame)?;
        }
        Ok(())
    }

    /// Pop the next voice-activity score produced by
    /// [`write_audio_frame`](Self::write_audio_frame), if any — one score per processed
    /// 10ms block, in production order. `None` when no [`attach_vad`](Self::attach_vad)
    /// detector is attached, or none is ready yet.
    ///
    /// If the attached [`VoiceActivityDetector`] becomes disabled (a caught backend
    /// panic), this silently stops producing new scores rather than surfacing an error
    /// here — see `adr/0003-audio-track-and-apm-integration.md` § Error handling.
    pub fn poll_vad_score(&mut self) -> Option<f32> {
        self.audio.as_mut()?.vad_scores.pop_front()
    }

    /// Flush the encoder(s) and muxer, returning the complete fMP4 byte stream.
    ///
    /// **Known gap, inherited from `mediaway-audio-apm`, not fixed here**: a trailing
    /// audio block shorter than 10ms sitting in an attached [`AudioProcessor`]'s
    /// internal buffer is not flushed — `AudioProcessor` has no "flush a partial block"
    /// method today. See `adr/0003-audio-track-and-apm-integration.md` § `finish()`.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] on encoder or mux failure.
    pub fn finish(mut self) -> Result<Vec<u8>, PipelineError> {
        self.encoder.flush()?;
        self.drain()?;
        if let Some(mut audio) = self.audio.take() {
            audio.encoder.flush()?;
            Self::drain_audio(&mut audio, &mut self.muxer)?;
        }
        self.muxer.flush();
        let mut bytes = Vec::new();
        self.muxer.poll_bytes(&mut bytes);
        Ok(bytes)
    }

    fn drain(&mut self) -> Result<(), PipelineError> {
        while let Some(mut pkt) = self.encoder.poll_packet()? {
            pkt.stream_id = self.track_id;
            self.muxer.push_packet(&pkt)?;
        }
        Ok(())
    }

    fn drain_audio(
        audio: &mut AudioTrack,
        muxer: &mut mp4::Muxer<mp4::Live>,
    ) -> Result<(), PipelineError> {
        while let Some(mut pkt) = audio.encoder.poll_packet()? {
            pkt.stream_id = audio.track_id;
            muxer.push_packet(&pkt)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
