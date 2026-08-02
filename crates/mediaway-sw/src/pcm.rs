//! PCM / raw audio passthrough.
//!
//! PCM audio is already uncompressed, so a "codec" here is a typed
//! passthrough: [`PcmEncoder`] / [`PcmDecoder`] validate sample format, rate,
//! and channel count against a configured [`PcmFormat`] and move the payload
//! [`Bytes`] through unchanged — no re-encode, no resample. The only copy
//! involved is the cheap reference-counted [`Bytes`] clone needed to move
//! data out of a borrowed [`AudioFrame`]/[`Packet`] into a newly owned one
//! (see [`PcmEncoder::push_frame`] / [`PcmDecoder::push_packet`]); the
//! underlying sample buffer itself is never copied.
//!
//! Shaped like the push/poll session pattern used elsewhere in the
//! workspace: `push_frame` / `poll_packet` / `flush` (encode direction,
//! mirrors `mediaway_encoder::AudioEncoder`) and `push_packet` / `poll_frame`
//! / `flush` (decode direction). `mediaway-sw` does not depend on
//! `mediaway-encoder` / `mediaway-decoder` yet — same staging rationale as
//! `h264` (see `adr/0001-h264-baseline-decoder-first.md`): the actual trait
//! impl is deferred until a factory wires `mediaway-sw` in as a fallback.

#![forbid(unsafe_code)]

use std::collections::VecDeque;

use mediaway_common::{AudioFrame, Bytes, CodecKind, Packet, Rational, SampleFormat, StreamInfo};
use thiserror::Error;

/// Errors from PCM passthrough encode/decode.
///
/// All variants come from a caller-provided frame not matching the session's
/// configured [`PcmFormat`], or the session already being closed — none are
/// ever raised for a rejected bitstream, since PCM has no encoding to
/// reject.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PcmError {
    /// Input sample format does not match the session's configured format.
    #[error("PCM sample format does not match the configured format")]
    SampleFormatMismatch,
    /// Input sample rate does not match the session's configured rate.
    #[error("PCM sample rate does not match the configured rate")]
    SampleRateMismatch,
    /// Input channel count does not match the session's configured count.
    #[error("PCM channel count does not match the configured channel count")]
    ChannelCountMismatch,
    /// Session already flushed; no further input is accepted.
    #[error("PCM passthrough session closed")]
    Closed,
}

/// PCM format a [`PcmEncoder`] / [`PcmDecoder`] session validates input
/// against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmFormat {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// PCM sample layout.
    pub sample_format: SampleFormat,
}

/// Parameters for opening a [`PcmEncoder`] / [`PcmDecoder`] session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmPassthroughConfig {
    /// Fixed PCM format every frame/packet must match.
    pub format: PcmFormat,
    /// Timestamp timebase for input frames/packets and output packets/frames.
    pub time_base: Rational,
}

impl PcmPassthroughConfig {
    /// Construct a config for the given format and timebase.
    #[must_use]
    pub const fn new(format: PcmFormat, time_base: Rational) -> Self {
        Self { format, time_base }
    }
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info(config: PcmPassthroughConfig) -> StreamInfo {
    StreamInfo::Audio {
        id: 0,
        codec: CodecKind::RawAudio,
        time_base: config.time_base,
        extra_data: Bytes::new(),
        sample_rate: config.format.sample_rate,
        channels: config.format.channels,
    }
}

/// Typed PCM passthrough "encoder".
///
/// Validates [`AudioFrame`]s against the configured [`PcmFormat`] and
/// re-packages the unchanged payload as [`Packet`]s. No compression happens
/// — PCM is already uncompressed.
///
/// Mirrors the push/poll shape of `mediaway_encoder::AudioEncoder`.
pub struct PcmEncoder {
    config: PcmPassthroughConfig,
    stream_info: StreamInfo,
    pending: VecDeque<Packet>,
    closed: bool,
}

impl PcmEncoder {
    /// Open a session, validating input against `config`.
    #[must_use]
    pub fn new(config: PcmPassthroughConfig) -> Self {
        Self {
            config,
            stream_info: stream_info(config),
            pending: VecDeque::new(),
            closed: false,
        }
    }

    /// Stream metadata for this session.
    #[must_use]
    pub const fn stream_info(&self) -> &StreamInfo {
        &self.stream_info
    }

    /// Submit one PCM buffer. Produces exactly one packet, retrievable via
    /// [`poll_packet`](Self::poll_packet) — no compression, no reordering.
    ///
    /// # Errors
    ///
    /// Returns [`PcmError`] when `frame`'s format/rate/channels do not match
    /// the session's configured [`PcmFormat`], or the session is closed.
    pub fn push_frame(&mut self, frame: &AudioFrame) -> Result<(), PcmError> {
        if self.closed {
            return Err(PcmError::Closed);
        }
        if frame.format != self.config.format.sample_format {
            return Err(PcmError::SampleFormatMismatch);
        }
        if frame.sample_rate != self.config.format.sample_rate {
            return Err(PcmError::SampleRateMismatch);
        }
        if frame.channels != self.config.format.channels {
            return Err(PcmError::ChannelCountMismatch);
        }
        self.pending.push_back(Packet {
            stream_id: self.stream_info.id(),
            pts: frame.pts,
            dts: frame.pts,
            duration: frame.duration,
            is_keyframe: true,
            is_discard: false,
            // clone: Bytes share (refcount bump, no payload memcpy) — `frame`
            // is a borrowed &AudioFrame; Packet needs its own owned Bytes.
            payload: frame.data.clone(),
        });
        Ok(())
    }

    /// Pull the next packet, if any.
    pub fn poll_packet(&mut self) -> Result<Option<Packet>, PcmError> {
        Ok(self.pending.pop_front())
    }

    /// Signal end-of-input; drain remaining packets with
    /// [`poll_packet`](Self::poll_packet).
    ///
    /// # Errors
    ///
    /// Never fails; returns [`Result`] to match the encoder session shape.
    pub const fn flush(&mut self) -> Result<(), PcmError> {
        self.closed = true;
        Ok(())
    }
}

/// Typed PCM passthrough "decoder": re-packages [`Packet`] payloads as
/// [`AudioFrame`]s carrying the session's configured [`PcmFormat`].
///
/// Mirrors the push/poll shape of a `mediaway_decoder`-style session
/// (`push_packet` / `poll_frame` / `flush`).
pub struct PcmDecoder {
    config: PcmPassthroughConfig,
    stream_info: StreamInfo,
    pending: VecDeque<AudioFrame>,
    closed: bool,
}

impl PcmDecoder {
    /// Open a session, tagging output frames with `config`'s [`PcmFormat`].
    #[must_use]
    pub fn new(config: PcmPassthroughConfig) -> Self {
        Self {
            config,
            stream_info: stream_info(config),
            pending: VecDeque::new(),
            closed: false,
        }
    }

    /// Stream metadata for this session.
    #[must_use]
    pub const fn stream_info(&self) -> &StreamInfo {
        &self.stream_info
    }

    /// Submit one packet. Produces exactly one frame, retrievable via
    /// [`poll_frame`](Self::poll_frame).
    ///
    /// `packet`'s payload is trusted to already match the session's
    /// configured [`PcmFormat`] — a [`Packet`] carries no per-packet format
    /// fields to validate against (unlike [`AudioFrame`]); callers are
    /// responsible for only routing matching-format packets to this session.
    ///
    /// # Errors
    ///
    /// Returns [`PcmError::Closed`] when the session is closed.
    pub fn push_packet(&mut self, packet: &Packet) -> Result<(), PcmError> {
        if self.closed {
            return Err(PcmError::Closed);
        }
        self.pending.push_back(AudioFrame {
            pts: packet.pts,
            duration: packet.duration,
            sample_rate: self.config.format.sample_rate,
            channels: self.config.format.channels,
            format: self.config.format.sample_format,
            // clone: Bytes share (refcount bump, no payload memcpy) —
            // `packet` is a borrowed &Packet; AudioFrame needs its own owned
            // Bytes.
            data: packet.payload.clone(),
        });
        Ok(())
    }

    /// Pull the next decoded frame, if any.
    pub fn poll_frame(&mut self) -> Result<Option<AudioFrame>, PcmError> {
        Ok(self.pending.pop_front())
    }

    /// Signal end-of-input; drain remaining frames with
    /// [`poll_frame`](Self::poll_frame).
    ///
    /// # Errors
    ///
    /// Never fails; returns [`Result`] to match the decoder session shape.
    pub const fn flush(&mut self) -> Result<(), PcmError> {
        self.closed = true;
        Ok(())
    }
}

#[cfg(test)]
#[path = "pcm_tests.rs"]
mod tests;
