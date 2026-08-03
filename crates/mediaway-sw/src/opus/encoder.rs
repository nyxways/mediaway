//! [`OpusEncoder`] — Opus encode session over `unsafe-libopus`'s raw C-shaped API.

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
    OPUS_SET_BITRATE_REQUEST, OPUS_SET_INBAND_FEC_REQUEST, OPUS_SET_PACKET_LOSS_PERC_REQUEST,
    OpusEncoder as RawOpusEncoder, opus_encode_float, opus_encoder_create, opus_encoder_ctl,
    opus_encoder_destroy, opus_strerror,
};

use crate::opus::config::{OpusEncoderConfig, frame_size_samples};
use crate::opus::error::OpusError;

/// Output payload buffer allocated per encode call. `unsafe-libopus`'s own
/// `opus_encode_float` returns `OPUS_BUFFER_TOO_SMALL` if this is not enough
/// for one frame; 4000 bytes matches the buffer size libopus's own reference
/// `opus_demo` CLI allocates (a real Opus frame is far smaller in practice —
/// RFC 6716's own worst case is 1275 bytes — this is a safety margin, not a
/// measured typical size).
const MAX_PACKET_BYTES: usize = 4000;

fn backend_result(code: i32) -> Result<(), OpusError> {
    if code < 0 {
        Err(OpusError::Backend {
            code,
            message: opus_strerror(code),
        })
    } else {
        Ok(())
    }
}

/// Streaming Opus encoder session over `unsafe-libopus`'s C-shaped API.
///
/// Owns a `*mut unsafe_libopus::OpusEncoder` privately — the raw pointer
/// never appears in this crate's public API. [`Drop`] calls
/// `opus_encoder_destroy` exactly once.
///
/// # Costly path
///
/// [`push_frame`](Self::push_frame) copies caller PCM bytes into a `f32`
/// scratch buffer for `unsafe-libopus`'s raw `*const f32` input, then copies
/// the compressed output out of its `*mut u8` buffer into an owned [`Bytes`]
/// — not Zero-Copy (payload crosses the raw pointer boundary both ways).
/// `unsafe-libopus` is a `c2rust` transpile with no inline asm/SIMD paths;
/// upstream's own benchmark reports ~20% higher CPU cost than the hand-tuned
/// C reference libopus. See `docs/spec/caveats-and-clarity.md`.
#[derive(Debug)]
pub struct OpusEncoder {
    ptr: ptr::NonNull<RawOpusEncoder>,
    stream_info: StreamInfo,
    sample_rate: u32,
    channels: u16,
    frame_size_samples: usize,
    pcm_scratch: Vec<f32>,
    packet_scratch: Vec<u8>,
    pending: VecDeque<Packet>,
    closed: bool,
}

// SAFETY: upstream libopus documents its encoder state as usable from any
// single thread at a time — no thread affinity, but never concurrent access
// from two threads at once (the same contract `nyxie_voice::EncoderHandle`
// relies on in the sibling project this crate's ADR cites). Every mutating
// method here takes `&mut self`, so safe Rust already enforces single-writer
// access; only cross-thread *move* (`Send`), not shared access (`Sync`), is
// claimed.
unsafe impl Send for OpusEncoder {}

impl OpusEncoder {
    /// Open an Opus encoder session for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`OpusError::InvalidFrameDuration`] when `config.time_base`
    /// does not divide evenly into a whole sample count at
    /// `config.sample_rate`; [`OpusError::Backend`] when `unsafe-libopus`
    /// rejects the sample rate / channel count / application, or a
    /// bitrate / in-band-FEC / packet-loss-percent `opus_encoder_ctl` call fails.
    pub fn open(config: &OpusEncoderConfig) -> Result<Self, OpusError> {
        let frame_size = frame_size_samples(config.sample_rate, config.time_base)?;
        let channels = i32::from(config.channels);

        let mut err: i32 = 0;
        // SAFETY: `err` is a valid `&mut i32` for the duration of this call.
        // `opus_encoder_create` writes a status code into it and returns
        // either a heap-allocated `*mut OpusEncoder` (owned by this struct
        // from here on, freed exactly once in `Drop`) or null on failure.
        let raw = unsafe {
            opus_encoder_create(
                config.sample_rate as i32,
                channels,
                config.application.to_raw(),
                &raw mut err,
            )
        };
        let Some(ptr) = ptr::NonNull::new(raw) else {
            return Err(OpusError::Backend {
                code: err,
                message: opus_strerror(err),
            });
        };

        if let Err(e) = configure(ptr, config) {
            // SAFETY: `ptr` was returned by `opus_encoder_create` immediately
            // above and has not been destroyed yet — destroying it here
            // avoids leaking encoder state on a ctl failure.
            unsafe { opus_encoder_destroy(ptr.as_ptr()) };
            return Err(e);
        }

        let channels_usize = usize::from(config.channels);
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
            frame_size_samples: frame_size,
            pcm_scratch: Vec::with_capacity(frame_size * channels_usize),
            packet_scratch: vec![0u8; MAX_PACKET_BYTES],
            pending: VecDeque::new(),
            closed: false,
        })
    }

    /// Stream metadata for this session.
    #[must_use]
    pub const fn stream_info(&self) -> &StreamInfo {
        &self.stream_info
    }

    /// Submit one PCM buffer. Encodes synchronously; retrieve the compressed
    /// packet via [`poll_packet`](Self::poll_packet).
    ///
    /// # Errors
    ///
    /// Returns [`OpusError::Closed`] after [`flush`](Self::flush);
    /// [`OpusError::UnsupportedSampleFormat`] when `frame.format` is not
    /// [`SampleFormat::F32`]; [`OpusError::ConfigMismatch`] when
    /// `frame.sample_rate`/`frame.channels` do not match the session;
    /// [`OpusError::FrameSizeMismatch`] when `frame.data.len()` is not
    /// exactly the session's configured Opus frame size — this crate never
    /// re-buffers/re-chunks (see [`OpusEncoderConfig::time_base`]);
    /// [`OpusError::Backend`] on `unsafe-libopus` encode failure.
    pub fn push_frame(&mut self, frame: &AudioFrame) -> Result<(), OpusError> {
        if self.closed {
            return Err(OpusError::Closed);
        }
        if frame.format != SampleFormat::F32 {
            return Err(OpusError::UnsupportedSampleFormat);
        }
        if frame.sample_rate != self.sample_rate || frame.channels != self.channels {
            return Err(OpusError::ConfigMismatch);
        }
        let expected_bytes =
            self.frame_size_samples * usize::from(self.channels) * size_of::<f32>();
        if frame.data.len() != expected_bytes {
            return Err(OpusError::FrameSizeMismatch {
                expected_samples: self.frame_size_samples,
                expected_bytes,
                actual_bytes: frame.data.len(),
            });
        }

        self.pcm_scratch.clear();
        self.pcm_scratch.extend(
            frame
                .data
                .chunks_exact(size_of::<f32>())
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        );

        // SAFETY: `self.ptr` is a live encoder owned by this struct;
        // `self.pcm_scratch` holds exactly `self.frame_size_samples *
        // channels` samples (checked above); `self.packet_scratch` is a
        // valid `MAX_PACKET_BYTES`-byte buffer for the duration of the call.
        let len = unsafe {
            opus_encode_float(
                self.ptr.as_ptr(),
                self.pcm_scratch.as_ptr(),
                self.frame_size_samples as i32,
                self.packet_scratch.as_mut_ptr(),
                self.packet_scratch.len() as i32,
            )
        };
        if len < 0 {
            return Err(OpusError::Backend {
                code: len,
                message: opus_strerror(len),
            });
        }

        self.pending.push_back(Packet {
            stream_id: self.stream_info.id(),
            pts: frame.pts,
            dts: frame.pts,
            duration: frame.duration,
            is_keyframe: true,
            is_discard: false,
            payload: Bytes::copy_from_slice(&self.packet_scratch[..len as usize]),
        });
        Ok(())
    }

    /// Pull the next compressed packet, if any.
    pub fn poll_packet(&mut self) -> Result<Option<Packet>, OpusError> {
        Ok(self.pending.pop_front())
    }

    /// Signal end-of-input. `push_frame` always encodes and enqueues
    /// synchronously (no internal lookahead buffer in this design), so
    /// `flush` only marks the session closed — call
    /// [`poll_packet`](Self::poll_packet) beforehand to drain any pending packet.
    ///
    /// # Errors
    ///
    /// Never fails; returns [`Result`] to match the encoder session shape.
    pub const fn flush(&mut self) -> Result<(), OpusError> {
        self.closed = true;
        Ok(())
    }
}

/// Applies optional bitrate / in-band-FEC / packet-loss-percent settings via
/// `opus_encoder_ctl!` right after `opus_encoder_create`.
fn configure(
    ptr: ptr::NonNull<RawOpusEncoder>,
    config: &OpusEncoderConfig,
) -> Result<(), OpusError> {
    if let Some(bitrate) = config.bitrate_bps {
        let bitrate = i32::try_from(bitrate).unwrap_or(i32::MAX);
        // SAFETY: `ptr` is a live encoder just created by `opus_encoder_create`
        // in `open`. `opus_encoder_ctl!` forwards to `opus_encoder_ctl_impl`,
        // an `unsafe fn` per `unsafe-libopus`'s own C-shaped contract.
        backend_result(unsafe {
            opus_encoder_ctl!(ptr.as_ptr(), OPUS_SET_BITRATE_REQUEST, bitrate)
        })?;
    }
    if config.inband_fec {
        // SAFETY: see above.
        backend_result(unsafe {
            opus_encoder_ctl!(ptr.as_ptr(), OPUS_SET_INBAND_FEC_REQUEST, 1i32)
        })?;
        let loss_percent = i32::from(config.packet_loss_percent);
        // SAFETY: see above.
        backend_result(unsafe {
            opus_encoder_ctl!(
                ptr.as_ptr(),
                OPUS_SET_PACKET_LOSS_PERC_REQUEST,
                loss_percent
            )
        })?;
    }
    Ok(())
}

impl Drop for OpusEncoder {
    fn drop(&mut self) {
        // SAFETY: `self.ptr` was created by `opus_encoder_create` in `open`
        // and is never shared, cloned, or destroyed anywhere else.
        unsafe { opus_encoder_destroy(self.ptr.as_ptr()) };
    }
}

#[cfg(test)]
#[path = "encoder_tests.rs"]
mod tests;
