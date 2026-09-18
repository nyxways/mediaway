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
///
/// Push buffers, then [`poll_packet`](AudioEncoder::poll_packet) until `Ok(None)`, then
/// [`finish`](AudioEncoder::finish).
///
/// # Ending a stream
///
/// Call [`finish`](Self::finish): it flushes and hands back every packet still inside the
/// encoder. (Or [`flush`](Self::flush), then [`poll_packet`](Self::poll_packet) until
/// `Ok(None)`, when the encoder must stay usable.)
///
/// **Dropping an encoder that has not been flushed discards the frames still in its
/// pipeline** — silently, since `Drop` has nowhere to return them. That is the right way to
/// abandon an encode, and a data loss otherwise: hardware encoders are pipelined, so the
/// dropped frames are the *end* of the stream. Measured with the Windows hardware path in a
/// screen recorder, every recording that dropped its video encoder unflushed lost its final frame.
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

    /// End the stream: [`flush`](Self::flush), then collect every remaining packet.
    ///
    /// Consumes the encoder, so a finished encoder cannot be pushed to again. The packets come
    /// back in the order [`poll_packet`](Self::poll_packet) would have returned them. Anything
    /// already polled is not repeated.
    ///
    /// This is the default way to end a stream; dropping without it discards the encoder's
    /// in-flight frames (see the trait docs).
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError`] if the flush or a poll fails. Packets collected before the
    /// failure are lost with it; use `flush` and `poll_packet` directly to keep them.
    fn finish(mut self) -> Result<Vec<Packet>, EncodeError>
    where
        Self: Sized,
    {
        self.flush()?;
        let mut packets = Vec::new();
        while let Some(packet) = self.poll_packet()? {
            packets.push(packet);
        }
        Ok(packets)
    }
}

#[cfg(test)]
#[path = "audio_tests.rs"]
mod tests;
