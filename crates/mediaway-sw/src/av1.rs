//! AV1 encode via [`rav1e`](https://github.com/xiph/rav1e), a pure-Rust,
//! BSD-2-Clause AV1 encoder.
//!
//! No GPU/vendor SDK, no C toolchain (built with `default-features = false`:
//! no `asm`/`nasm`/`cc`, no `binaries`/CLI deps, no threadpool).
//!
//! `rav1e` is itself a complete AV1 encoder implementation; this module is a
//! thin sans-io adapter around its own `Config`/`Context`/`Frame`/`Packet`
//! API — not a reimplementation of AV1. See
//! `adr/0002-rav1e-av1-encode.md` for why `rav1e` and what surface this
//! exposes.
//!
//! Only 8-bit 4:2:0 planar input ([`PixelFormat::I420`]) is supported: this
//! is `rav1e`'s own native `Frame<u8>` / `ChromaSampling::Cs420` layout, so
//! no packed-to-planar or chroma resampling conversion is needed on the
//! per-frame path — the one copy that *is* unavoidable (packed [`Bytes`]
//! into `rav1e`'s own padded plane storage) is documented on
//! [`Av1Encoder::push_frame`].
//!
//! Shaped like `mediaway_encoder::VideoEncoder` (`push_frame` / `poll_packet`
//! / `flush` over `mediaway_common` types) without depending on that crate
//! yet — same staging rationale as `h264`/`pcm` (see
//! `adr/0001-h264-baseline-decoder-first.md`): the trait impl is deferred
//! until a factory wires `mediaway-sw` in as a fallback.

#![forbid(unsafe_code)]

use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};
use rav1e::prelude::{ChromaSampling, Config, Context, EncoderConfig, EncoderStatus, FrameType};
use thiserror::Error;

/// Errors from opening or running an [`Av1Encoder`] session.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Av1Error {
    /// Input pixel format is not [`PixelFormat::I420`], or the frame is
    /// GPU-backed ([`VideoFrameStorage::Gpu`]) — this software encoder only
    /// ever accepts CPU-resident planar 4:2:0 input.
    #[error("unsupported AV1 encode input (pixel format or storage kind)")]
    Unsupported,
    /// `rav1e` rejected the requested [`Av1EncoderConfig`] (dimensions,
    /// timebase, or other encoder-config constraint). The message is
    /// `rav1e`'s own `InvalidConfig` description.
    #[error("invalid AV1 encoder configuration: {0}")]
    InvalidConfig(String),
    /// Frame dimensions or buffer length do not match the session's config.
    #[error("invalid AV1 encode input (dimensions or buffer size mismatch)")]
    InvalidInput,
    /// `rav1e` backend failure (`EncoderStatus::Failure`, or an
    /// [`EncoderStatus`] variant `rav1e` documents as unreachable from the
    /// call site that produced it — mapped defensively rather than panicking).
    #[error("rav1e backend failure")]
    Backend,
    /// Session already flushed; no further frames are accepted.
    #[error("AV1 encoder session closed")]
    Closed,
}

/// Configuration for opening an [`Av1Encoder`] session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Av1EncoderConfig {
    /// Encoded width in pixels (`rav1e` requires ≥ 16 for non-still-picture).
    pub width: u32,
    /// Encoded height in pixels (`rav1e` requires ≥ 16 for non-still-picture).
    pub height: u32,
    /// Timestamp timebase for input frames and output packets.
    pub time_base: Rational,
    /// Target bitrate in bits per second (`0` = constant-quality mode using
    /// `rav1e`'s default quantizer, matching `VideoEncoderConfig`'s
    /// `bitrate_bps` convention elsewhere in the workspace).
    pub bitrate_bps: u32,
    /// `rav1e` speed preset, `0` (best quality, slowest) to `10` (fastest).
    /// Values above `10` are treated the same as `10` (`rav1e`'s own
    /// `with_speed_preset` clamp).
    pub speed: u8,
    /// Disable frame reordering / lookahead for lower latency at some cost
    /// to compression efficiency.
    pub low_latency: bool,
}

impl Av1EncoderConfig {
    /// Config for `width`x`height` at `time_base`, constant-quality mode,
    /// `rav1e`'s own default speed preset (`6`, "balance between quality and
    /// speed"), reordering enabled. Prefer setting fields explicitly in apps.
    #[must_use]
    pub const fn new(width: u32, height: u32, time_base: Rational) -> Self {
        Self {
            width,
            height,
            time_base,
            bitrate_bps: 0,
            speed: 6,
            low_latency: false,
        }
    }
}

/// Sans-io AV1 encoder session wrapping [`rav1e::Context`].
///
/// Push frames, then [`poll_packet`](Self::poll_packet) until `Ok(None)`,
/// then [`flush`](Self::flush) and drain again — mirrors
/// `mediaway_encoder::VideoEncoder`.
pub struct Av1Encoder {
    ctx: Context<u8>,
    stream_info: StreamInfo,
    width: usize,
    height: usize,
    chroma_width: usize,
    chroma_height: usize,
}

impl Av1Encoder {
    /// Open an AV1 encoder session for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`Av1Error::InvalidInput`] when `bitrate_bps` does not fit
    /// `i32` (`rav1e`'s own bitrate field type), or
    /// [`Av1Error::InvalidConfig`] when `rav1e` rejects the resulting
    /// encoder configuration (e.g. width/height below its 16px minimum).
    pub fn open(config: &Av1EncoderConfig) -> Result<Self, Av1Error> {
        let bitrate = i32::try_from(config.bitrate_bps).map_err(|_| Av1Error::InvalidInput)?;

        let mut enc = EncoderConfig::with_speed_preset(config.speed);
        enc.width = config.width as usize;
        enc.height = config.height as usize;
        enc.time_base =
            rav1e::prelude::Rational::new(config.time_base.num, u64::from(config.time_base.den));
        enc.chroma_sampling = ChromaSampling::Cs420;
        enc.low_latency = config.low_latency;
        if bitrate > 0 {
            enc.bitrate = bitrate;
            // Unconstrained quantizer ceiling in target-bitrate mode — mirrors
            // rav1e's own reference CLI convention (`src/bin/common.rs`).
            enc.quantizer = 255;
        }

        let ctx: Context<u8> = Config::new()
            .with_encoder_config(enc)
            .new_context()
            .map_err(|e| Av1Error::InvalidConfig(e.to_string()))?;

        // The AV1 sequence header only depends on static encoder config, not
        // on any frame having been pushed yet, so it is available immediately.
        let extra_data = Bytes::from(ctx.container_sequence_header());
        let (chroma_width, chroma_height) = ChromaSampling::Cs420
            .get_chroma_dimensions(config.width as usize, config.height as usize);

        Ok(Self {
            ctx,
            stream_info: StreamInfo::Video {
                id: 0,
                codec: CodecKind::Av1,
                time_base: config.time_base,
                geometry: VideoGeometry {
                    width: config.width,
                    height: config.height,
                },
                extra_data,
            },
            width: config.width as usize,
            height: config.height as usize,
            chroma_width,
            chroma_height,
        })
    }

    /// Stream metadata (`extra_data` carries the AV1 sequence header from
    /// session open — see [`open`](Self::open)).
    #[must_use]
    pub const fn stream_info(&self) -> &StreamInfo {
        &self.stream_info
    }

    /// Submit one raw video frame for encoding.
    ///
    /// # Costly path
    ///
    /// Copies `frame`'s packed I420 [`Bytes`] into `rav1e`'s own padded,
    /// separately-allocated [`rav1e::Frame`] plane storage (one `memcpy` per
    /// plane via `Plane::copy_from_raw_u8`). `rav1e::Context::new_frame`
    /// always owns its plane buffers, so a Zero-Copy handoff of caller-owned
    /// `Bytes` into it is not possible through the public `rav1e` API — this
    /// mirrors how the reference `rav1e` CLI (`src/bin/decoder/y4m.rs`) fills
    /// frames from its own decoded input.
    ///
    /// # Errors
    ///
    /// Returns [`Av1Error::Unsupported`] when `frame.format` is not
    /// [`PixelFormat::I420`] or `frame.storage` is GPU-backed;
    /// [`Av1Error::InvalidInput`] when dimensions or buffer length do not
    /// match the session's config; [`Av1Error::Closed`] after
    /// [`flush`](Self::flush).
    pub fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), Av1Error> {
        if frame.format != PixelFormat::I420 {
            return Err(Av1Error::Unsupported);
        }
        if frame.width as usize != self.width || frame.height as usize != self.height {
            return Err(Av1Error::InvalidInput);
        }
        // `VideoFrameStorage` is `#[non_exhaustive]`; any non-CPU variant
        // (GPU-backed today, possibly more later) is rejected the same way —
        // this software encoder only ever accepts CPU planes.
        let VideoFrameStorage::Cpu { data } = &frame.storage else {
            return Err(Av1Error::Unsupported);
        };

        let y_len = self.width * self.height;
        let chroma_len = self.chroma_width * self.chroma_height;
        let expected_len = y_len + 2 * chroma_len;
        if data.len() != expected_len {
            return Err(Av1Error::InvalidInput);
        }

        let mut rav1e_frame = self.ctx.new_frame();
        rav1e_frame.planes[0].copy_from_raw_u8(&data[..y_len], self.width, 1);
        rav1e_frame.planes[1].copy_from_raw_u8(
            &data[y_len..y_len + chroma_len],
            self.chroma_width,
            1,
        );
        rav1e_frame.planes[2].copy_from_raw_u8(
            &data[y_len + chroma_len..expected_len],
            self.chroma_width,
            1,
        );

        match self.ctx.send_frame(rav1e_frame) {
            Ok(()) => Ok(()),
            Err(EncoderStatus::EnoughData) => Err(Av1Error::Closed),
            // `rav1e::Context::send_frame` never returns anything but
            // `EnoughData`/`Failure` per its own documented contract; the
            // rest are mapped defensively (same `Backend` outcome) rather
            // than panicking.
            Err(
                EncoderStatus::Failure
                | EncoderStatus::NeedMoreData
                | EncoderStatus::LimitReached
                | EncoderStatus::Encoded
                | EncoderStatus::NotReady,
            ) => Err(Av1Error::Backend),
        }
    }

    /// Pull the next compressed packet, if any.
    ///
    /// # Errors
    ///
    /// Returns [`Av1Error::Backend`] on a `rav1e` encode failure.
    pub fn poll_packet(&mut self) -> Result<Option<Packet>, Av1Error> {
        let stream_id = self.stream_info.id();
        loop {
            match self.ctx.receive_packet() {
                Ok(packet) => return Ok(Some(convert_packet(packet, stream_id))),
                // A frame advanced internal state without emitting a packet
                // yet — keep draining until a packet or a "no more for now"
                // status comes back, so one `poll_packet` call fully settles.
                Err(EncoderStatus::Encoded) => {}
                // `EnoughData` is never returned by `receive_packet` per its
                // own documented contract; mapped defensively (same "nothing
                // ready" outcome) rather than panicking.
                Err(
                    EncoderStatus::NeedMoreData
                    | EncoderStatus::LimitReached
                    | EncoderStatus::NotReady
                    | EncoderStatus::EnoughData,
                ) => return Ok(None),
                Err(EncoderStatus::Failure) => return Err(Av1Error::Backend),
            }
        }
    }

    /// Signal end-of-input; drain remaining packets with
    /// [`poll_packet`](Self::poll_packet).
    ///
    /// # Errors
    ///
    /// Never fails; returns [`Result`] to match the encoder session shape.
    pub fn flush(&mut self) -> Result<(), Av1Error> {
        self.ctx.flush();
        Ok(())
    }
}

/// Converts rav1e's `u64` frame ordinal to `Packet`'s `i64` timestamp field,
/// saturating instead of wrapping in the practically-unreachable case of a
/// session encoding more than `i64::MAX` frames.
fn pts_from_input_frameno(input_frameno: u64) -> i64 {
    i64::try_from(input_frameno).unwrap_or(i64::MAX)
}

fn convert_packet(packet: rav1e::Packet<u8>, stream_id: u32) -> Packet {
    Packet {
        stream_id,
        // rav1e tracks display order internally via `input_frameno`, not the
        // caller's original `VideoFrame::pts` — the public `rav1e` API has no
        // per-frame pts hook (see `push_frame`'s costly-path note for the
        // related plane-copy caveat). Packets are numbered by that ordinal in
        // the session's configured time_base rather than carrying the
        // caller-submitted timestamp through.
        pts: pts_from_input_frameno(packet.input_frameno),
        dts: pts_from_input_frameno(packet.input_frameno),
        duration: 1,
        is_keyframe: packet.frame_type == FrameType::KEY,
        is_discard: false,
        payload: Bytes::from(packet.data),
    }
}

#[cfg(test)]
#[path = "av1_tests.rs"]
mod tests;
