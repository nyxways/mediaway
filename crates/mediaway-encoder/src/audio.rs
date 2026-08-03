//! Audio encode config and [`AudioEncoder`] trait.

#![forbid(unsafe_code)]

pub mod sw_opus;
#[cfg(test)]
mod sw_opus_tests;

use crate::error::EncodeError;
use mediaway_common::{AudioFrame, CodecKind, Packet, Rational, SampleFormat, StreamInfo};

/// Parameters for opening an audio encoder session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioEncoderConfig {
    /// Output codec (Stage 1 Windows: [`CodecKind::Aac`]).
    pub codec: CodecKind,
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Input PCM format.
    pub sample_format: SampleFormat,
    /// Timestamp timebase.
    pub time_base: Rational,
    /// Target bitrate in bits per second (`0` = backend default).
    pub bitrate_bps: u32,
}

impl AudioEncoderConfig {
    /// AAC stereo defaults; bitrate left to the backend.
    #[must_use]
    pub const fn aac_stereo(sample_rate: u32, time_base: Rational) -> Self {
        Self {
            codec: CodecKind::Aac,
            sample_rate,
            channels: 2,
            sample_format: SampleFormat::F32,
            time_base,
            bitrate_bps: 0,
        }
    }
}

/// Streaming hardware (or backend) audio encoder.
pub trait AudioEncoder {
    /// Stream metadata (updated when extradata becomes available).
    fn stream_info(&self) -> &StreamInfo;

    /// Submit one audio buffer.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError`] when the buffer is rejected or the session failed.
    fn push_frame(&mut self, frame: &AudioFrame) -> Result<(), EncodeError>;

    /// Pull the next compressed packet, if any.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError`] on backend failure.
    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError>;

    /// Signal end-of-input; drain with [`poll_packet`](Self::poll_packet).
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError`] on backend failure.
    fn flush(&mut self) -> Result<(), EncodeError>;
}
