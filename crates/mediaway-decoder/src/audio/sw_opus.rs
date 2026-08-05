//! Opus audio decode session over [`mediaway_sw::opus::decoder::OpusDecoder`].
//!
//! Cross-platform software backend, usable on any host regardless of an OS Opus
//! decoder MFT (e.g. Windows' inbox `CMSOpusDecMFT` — see
//! `mediaway-decoder`'s `windows::wmf::opus` module docs). Costly-path note:
//! `mediaway-sw`'s opus decoder copies PCM across the raw-pointer boundary
//! (see its module docs) — pure-SW, never Zero-Copy.

#![forbid(unsafe_code)]

use mediaway_common::{AudioFrame, Packet, StreamInfo};
use mediaway_sw::opus::config::OpusDecoderConfig;
use mediaway_sw::opus::error::OpusError;

use crate::AudioDecoder;
use crate::error::DecodeError;

/// Streaming Opus decoder session over `mediaway-sw`'s `unsafe-libopus` adapter.
#[derive(Debug)]
pub struct SwOpusAudioDecoder {
    inner: mediaway_sw::opus::decoder::OpusDecoder,
}

impl SwOpusAudioDecoder {
    /// Open an Opus decode session for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::InvalidInput`] when `config.time_base` does not divide
    /// evenly into a whole sample count at `config.sample_rate`, or
    /// [`DecodeError::Backend`] when `unsafe-libopus` rejects the sample rate / channel
    /// count.
    pub fn open(config: &OpusDecoderConfig) -> Result<Self, DecodeError> {
        match mediaway_sw::opus::decoder::OpusDecoder::open(config) {
            Ok(inner) => Ok(Self { inner }),
            Err(e) => Err(Self::map_err(&e)),
        }
    }

    const fn map_err(e: &OpusError) -> DecodeError {
        match e {
            OpusError::UnsupportedSampleFormat
            | OpusError::ConfigMismatch
            | OpusError::FrameSizeMismatch { .. }
            | OpusError::InvalidFrameDuration { .. } => DecodeError::InvalidInput,
            OpusError::Closed => DecodeError::Closed,
            _ => DecodeError::Backend, // non_exhaustive
        }
    }
}

impl AudioDecoder for SwOpusAudioDecoder {
    fn stream_info(&self) -> &StreamInfo {
        self.inner.stream_info()
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        self.inner
            .push_packet(packet)
            .map_err(|e| Self::map_err(&e))
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, DecodeError> {
        self.inner.poll_frame().map_err(|e| Self::map_err(&e))
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        self.inner.flush().map_err(|e| Self::map_err(&e))
    }
}
