//! Mediaway-typed RIFF/WAVE (PCM) mux + demux over [`riff_wave_core`].
//!
//! RIFF's chunk sizes must be known up front — there is no fragmented or
//! streamable profile in scope in the core crate, so this module exposes
//! [`riff_wave_core`]'s own whole-buffer shape (`push_packet`/`finish`, [`parse`])
//! rather than forcing it into the incremental push/poll
//! [`crate::Mux`]/[`crate::Demux`] traits, which this format cannot honestly
//! satisfy.

#![forbid(unsafe_code)]

use mediaway_common::{Bytes, CodecKind, Packet, Rational, StreamInfo};
use riff_wave_core::Muxer as CoreMuxer;
pub use riff_wave_core::{SampleFormat, WaveFormat};

/// WAV mux/demux error (same as [`riff_wave_core::Error`]).
pub type Error = riff_wave_core::Error;

/// Buffers pushed PCM samples for a single-track WAV file.
#[derive(Debug, Clone)]
pub struct Muxer {
    inner: CoreMuxer,
}

impl Muxer {
    /// Start an integer-PCM mux session.
    #[must_use]
    pub const fn new(sample_rate: u32, channels: u16, bits_per_sample: u16) -> Self {
        Self::with_format(WaveFormat {
            sample_format: SampleFormat::Pcm,
            channels,
            sample_rate,
            bits_per_sample,
        })
    }

    /// Start a mux session for an explicit [`WaveFormat`] (e.g. `Float` PCM).
    #[must_use]
    pub const fn with_format(format: WaveFormat) -> Self {
        Self {
            inner: CoreMuxer::new(format),
        }
    }

    /// Append raw interleaved PCM bytes (already encoded per the session's format).
    pub fn push_packet(&mut self, packet: &Packet) {
        self.inner.push_samples(&packet.payload);
    }

    /// Finalize and return the complete RIFF/WAVE byte stream.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.inner.finish()
    }
}

/// Parse a complete RIFF/WAVE buffer into a single-track [`StreamInfo`] + [`Packet`].
///
/// The whole `data` chunk becomes one packet (RIFF/WAVE carries no internal
/// frame boundaries). `Packet::duration` is the total PCM frame count
/// (`payload.len() / block_align`), in the same `1 / sample_rate` timebase as
/// [`StreamInfo::time_base`].
///
/// # Errors
///
/// Propagates [`riff_wave_core::Error`] parse failures.
pub fn parse(data: &[u8]) -> Result<(StreamInfo, Packet), Error> {
    let (format, payload) = riff_wave_core::parse(data)?;
    let stream = StreamInfo::Audio {
        id: 0,
        codec: CodecKind::RawAudio,
        time_base: Rational::new(1, format.sample_rate),
        extra_data: Bytes::new(),
        sample_rate: format.sample_rate,
        channels: format.channels,
    };
    let block_align = u64::from(format.block_align()).max(1);
    let payload_len = u64::try_from(payload.len()).unwrap_or(u64::MAX);
    let packet = Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: payload_len / block_align,
        is_keyframe: true,
        is_discard: false,
        payload,
    };
    Ok((stream, packet))
}

#[cfg(test)]
#[path = "wav_tests.rs"]
mod tests;
