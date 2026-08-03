//! [`OpusDecoder`] — Opus decode session over `unsafe-libopus`'s raw C-shaped API.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "unsafe-libopus's C-shaped API takes i32 counts/lengths; sample_rate, frame sizes, \
              and packet lengths are always small in practice (Opus legal rates top out at \
              48 kHz, packets are bytes over the network) — `frame_size_samples` already \
              rejects values that would not fit i32 before they reach these casts."
)]

use std::collections::VecDeque;
use std::ptr;

use mediaway_common::{AudioFrame, Bytes, CodecKind, Packet, SampleFormat, StreamInfo};
use unsafe_libopus::{
    OpusDecoder as RawOpusDecoder, opus_decode_float, opus_decoder_create, opus_decoder_destroy,
    opus_strerror,
};

use crate::opus::config::{OpusDecoderConfig, frame_size_samples};
use crate::opus::error::OpusError;

/// Streaming Opus decoder session over `unsafe-libopus`'s C-shaped API.
///
/// Owns a `*mut unsafe_libopus::OpusDecoder` privately — the raw pointer
/// never appears in this crate's public API. [`Drop`] calls
/// `opus_decoder_destroy` exactly once.
///
/// # Costly path
///
/// [`push_packet`](Self::push_packet) decodes into an `f32` scratch buffer
/// via `unsafe-libopus`'s raw `*mut f32` output parameter, then copies the
/// decoded samples out into an owned [`Bytes`] — not Zero-Copy. Same
/// `unsafe-libopus` transpile cost note as [`crate::opus::OpusEncoder`]: no inline
/// asm/SIMD, ~20% higher CPU cost than the hand-tuned C reference per
/// upstream's own benchmark. See `docs/spec/caveats-and-clarity.md`.
#[derive(Debug)]
pub struct OpusDecoder {
    ptr: ptr::NonNull<RawOpusDecoder>,
    stream_info: StreamInfo,
    sample_rate: u32,
    channels: u16,
    max_frame_samples: usize,
    pcm_scratch: Vec<f32>,
    pending: VecDeque<AudioFrame>,
    closed: bool,
}

// SAFETY: same justification as `OpusEncoder`'s `unsafe impl Send` — upstream
// libopus decoder state is documented usable from any single thread at a
// time, never concurrently from two; every mutating method here takes
// `&mut self`, so safe Rust already enforces single-writer access.
unsafe impl Send for OpusDecoder {}

impl OpusDecoder {
    /// Open an Opus decoder session for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`OpusError::InvalidFrameDuration`] when `config.time_base`
    /// does not divide evenly into a whole sample count at
    /// `config.sample_rate`; [`OpusError::Backend`] when `unsafe-libopus`
    /// rejects the sample rate / channel count.
    pub fn open(config: &OpusDecoderConfig) -> Result<Self, OpusError> {
        let max_frame_samples = frame_size_samples(config.sample_rate, config.time_base)?;
        let channels = i32::from(config.channels);

        let mut err: i32 = 0;
        // SAFETY: `err` is a valid raw pointer to a live `i32` for the
        // duration of this call. `opus_decoder_create` writes a status code
        // into it and returns either a heap-allocated `*mut OpusDecoder`
        // (owned by this struct from here on, freed exactly once in `Drop`)
        // or null on failure.
        let raw = unsafe { opus_decoder_create(config.sample_rate as i32, channels, &raw mut err) };
        let Some(ptr) = ptr::NonNull::new(raw) else {
            return Err(OpusError::Backend {
                code: err,
                message: opus_strerror(err),
            });
        };

        Ok(Self {
            ptr,
            stream_info: StreamInfo::Audio {
                id: 0,
                codec: CodecKind::Opus,
                time_base: config.time_base,
                extra_data: Bytes::new(),
                sample_rate: config.sample_rate,
                channels: config.channels,
            },
            sample_rate: config.sample_rate,
            channels: config.channels,
            max_frame_samples,
            pcm_scratch: vec![0.0f32; max_frame_samples * usize::from(config.channels)],
            pending: VecDeque::new(),
            closed: false,
        })
    }

    /// Stream metadata for this session.
    #[must_use]
    pub const fn stream_info(&self) -> &StreamInfo {
        &self.stream_info
    }

    /// Submit one compressed packet. Decodes synchronously; retrieve the PCM
    /// frame via [`poll_frame`](Self::poll_frame).
    ///
    /// An empty `packet.payload` is treated as a lost-packet hint (passed to
    /// `unsafe-libopus` as a null data pointer / zero length), which enables
    /// its built-in packet-loss concealment rather than being rejected as an
    /// invalid zero-byte packet.
    ///
    /// # Costly path
    ///
    /// Output buffer capacity is fixed at session open (`sample_rate *
    /// time_base` samples — see [`OpusDecoderConfig::time_base`]). A packet
    /// that decodes to more samples than that (e.g. a non-standard
    /// multi-frame Opus packet longer than the configured duration) fails
    /// with [`OpusError::Backend`] (`OPUS_BUFFER_TOO_SMALL`) instead of
    /// growing the buffer — this crate never re-buffers, the same contract
    /// [`crate::opus::OpusEncoder::push_frame`] applies symmetrically on encode.
    ///
    /// # Errors
    ///
    /// Returns [`OpusError::Closed`] after [`flush`](Self::flush);
    /// [`OpusError::Backend`] on `unsafe-libopus` decode failure.
    pub fn push_packet(&mut self, packet: &Packet) -> Result<(), OpusError> {
        if self.closed {
            return Err(OpusError::Closed);
        }

        let (data_ptr, data_len) = if packet.payload.is_empty() {
            (ptr::null(), 0)
        } else {
            (packet.payload.as_ptr(), packet.payload.len() as i32)
        };

        // SAFETY: `self.ptr` is a live decoder owned by this struct;
        // `data_ptr`/`data_len` are either both null/0 (packet-loss
        // concealment) or a valid pointer/length pair borrowed from
        // `packet.payload` for the duration of this call;
        // `self.pcm_scratch` has capacity for `self.max_frame_samples *
        // channels` samples, matching the `frame_size` argument below.
        let decoded_samples = unsafe {
            opus_decode_float(
                self.ptr.as_ptr(),
                data_ptr,
                data_len,
                self.pcm_scratch.as_mut_ptr(),
                self.max_frame_samples as i32,
                0,
            )
        };
        if decoded_samples < 0 {
            return Err(OpusError::Backend {
                code: decoded_samples,
                message: opus_strerror(decoded_samples),
            });
        }

        let sample_count = decoded_samples as usize * usize::from(self.channels);
        // `self.pcm_scratch` is a reused per-session scratch buffer; each
        // output `AudioFrame` needs its own owned `Bytes` so frames already
        // sitting in `pending` stay valid across the next `push_packet` call.
        let mut data = Vec::with_capacity(sample_count * size_of::<f32>());
        for sample in &self.pcm_scratch[..sample_count] {
            data.extend_from_slice(&sample.to_le_bytes());
        }

        self.pending.push_back(AudioFrame {
            pts: packet.pts,
            duration: packet.duration,
            sample_rate: self.sample_rate,
            channels: self.channels,
            format: SampleFormat::F32,
            data: Bytes::from(data),
        });
        Ok(())
    }

    /// Pull the next decoded frame, if any.
    pub fn poll_frame(&mut self) -> Result<Option<AudioFrame>, OpusError> {
        Ok(self.pending.pop_front())
    }

    /// Signal end-of-input. `push_packet` always decodes and enqueues
    /// synchronously, so `flush` only marks the session closed — call
    /// [`poll_frame`](Self::poll_frame) beforehand to drain any pending frame.
    ///
    /// # Errors
    ///
    /// Never fails; returns [`Result`] to match the decoder session shape.
    pub const fn flush(&mut self) -> Result<(), OpusError> {
        self.closed = true;
        Ok(())
    }
}

impl Drop for OpusDecoder {
    fn drop(&mut self) {
        // SAFETY: `self.ptr` was created by `opus_decoder_create` in `open`
        // and is never shared, cloned, or destroyed anywhere else.
        unsafe { opus_decoder_destroy(self.ptr.as_ptr()) };
    }
}

#[cfg(test)]
#[path = "decoder_tests.rs"]
mod tests;
