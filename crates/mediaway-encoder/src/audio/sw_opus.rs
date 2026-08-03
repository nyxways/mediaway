//! Opus audio encode session over [`mediaway_sw::opus::OpusEncoder`].
//!
//! Cross-platform software backend wired into the encoder facade where no OS
//! encoder exists (Windows has an inbox Opus *decoder* MFT but no encoder —
//! see `mediaway-decoder`'s `windows::wmf::opus` module docs). Costly-path
//! note: `mediaway-sw`'s opus encoder copies PCM across the raw-pointer
//! boundary (see its module docs) — pure-SW, never Zero-Copy.

#![forbid(unsafe_code)]

use mediaway_common::{AudioFrame, CodecKind, Packet, SampleFormat, StreamInfo};
use mediaway_sw::opus::config::{OpusApplication, OpusEncoderConfig};
use mediaway_sw::opus::error::OpusError;

use crate::AudioEncoder;
use crate::error::EncodeError;

/// Streaming Opus encoder session over `mediaway-sw`'s `unsafe-libopus` adapter.
#[derive(Debug)]
pub struct SwOpusAudioEncoder {
    inner: mediaway_sw::opus::encoder::OpusEncoder,
}

impl SwOpusAudioEncoder {
    /// Open an Opus encode session for `config`.
    ///
    /// Requires [`CodecKind::Opus`], [`SampleFormat::F32`], and a
    /// `config.time_base` that is a legal Opus frame duration (2.5/5/10/20/40/60 ms)
    /// at `config.sample_rate`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] for non-Opus/F32 input, or
    /// [`EncodeError::InvalidInput`] when the frame duration is not a legal
    /// Opus frame size.
    pub fn open(config: &crate::audio::AudioEncoderConfig) -> Result<Self, EncodeError> {
        if config.codec != CodecKind::Opus {
            return Err(EncodeError::Unsupported);
        }
        if config.sample_format != SampleFormat::F32 {
            return Err(EncodeError::Unsupported);
        }
        let sw_config = OpusEncoderConfig {
            sample_rate: config.sample_rate,
            channels: config.channels,
            application: OpusApplication::Audio,
            time_base: config.time_base,
            bitrate_bps: (config.bitrate_bps > 0).then_some(config.bitrate_bps),
            inband_fec: false,
            packet_loss_percent: 0,
        };
        match mediaway_sw::opus::encoder::OpusEncoder::open(&sw_config) {
            Ok(inner) => Ok(Self { inner }),
            Err(e) => Err(Self::map_err(&e)),
        }
    }

    const fn map_err(e: &OpusError) -> EncodeError {
        match e {
            OpusError::UnsupportedSampleFormat
            | OpusError::ConfigMismatch
            | OpusError::FrameSizeMismatch { .. }
            | OpusError::InvalidFrameDuration { .. } => EncodeError::InvalidInput,
            OpusError::Closed => EncodeError::Closed,
            _ => EncodeError::Backend, // non_exhaustive
        }
    }
}

impl AudioEncoder for SwOpusAudioEncoder {
    fn stream_info(&self) -> &StreamInfo {
        // mediaway-sw's OpusEncoder keeps its StreamInfo private; reconstruct
        // the identical shape (id 0, Opus, config time_base/sample_rate/channels).
        self.inner.stream_info()
    }

    fn push_frame(&mut self, frame: &AudioFrame) -> Result<(), EncodeError> {
        self.inner.push_frame(frame).map_err(|e| Self::map_err(&e))
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        self.inner.poll_packet().map_err(|e| Self::map_err(&e))
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        self.inner.flush().map_err(|e| Self::map_err(&e))
    }
}
