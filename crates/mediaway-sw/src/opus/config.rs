//! [`OpusApplication`], [`OpusEncoderConfig`], [`OpusDecoderConfig`], and the
//! shared frame-size-from-timebase computation both sessions use.

use mediaway_common::Rational;

use crate::opus::error::OpusError;

/// Opus encoder application/use-case hint — mirrors upstream `OPUS_APPLICATION_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpusApplication {
    /// `OPUS_APPLICATION_VOIP` — tuned for speech intelligibility over lossy links.
    Voip,
    /// `OPUS_APPLICATION_AUDIO` — tuned for non-voice/music fidelity.
    Audio,
    /// `OPUS_APPLICATION_RESTRICTED_LOWDELAY` — disables lookahead/prediction
    /// for the lowest algorithmic delay (real-time low-latency use cases).
    RestrictedLowDelay,
}

impl OpusApplication {
    pub(crate) const fn to_raw(self) -> i32 {
        match self {
            Self::Voip => unsafe_libopus::OPUS_APPLICATION_VOIP,
            Self::Audio => unsafe_libopus::OPUS_APPLICATION_AUDIO,
            Self::RestrictedLowDelay => unsafe_libopus::OPUS_APPLICATION_RESTRICTED_LOWDELAY,
        }
    }
}

/// Parameters for opening an [`crate::opus::OpusEncoder`] session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpusEncoderConfig {
    /// Sample rate (Hz). `unsafe-libopus`'s own `opus_encoder_create` is the
    /// source of truth on legal values (8/12/16/24/48 kHz) — not re-validated here.
    pub sample_rate: u32,
    /// Channel count (1 or 2 — `unsafe-libopus` validates).
    pub channels: u16,
    /// Encoder application/use-case hint.
    pub application: OpusApplication,
    /// Duration of one input [`mediaway_common::AudioFrame`] / output
    /// [`mediaway_common::Packet`], in seconds (`num`/`den`) — e.g.
    /// `Rational::new(1, 50)` for Opus's standard 20 ms frame. Combined with
    /// `sample_rate`, this fixes the exact PCM sample count
    /// [`crate::opus::OpusEncoder::push_frame`] requires per call. Opus only
    /// accepts 2.5/5/10/20/40/60 ms frames — a non-legal duration surfaces as
    /// [`OpusError::Backend`] from `unsafe-libopus`'s own encode call, not
    /// hand-validated here (same "dependency's own validator is the source
    /// of truth" stance as `mediaway-sw`'s `rav1e` adapter).
    pub time_base: Rational,
    /// Target bitrate in bits per second. `None` leaves `unsafe-libopus`'s
    /// own internal default untouched (no `OPUS_SET_BITRATE_REQUEST` call).
    pub bitrate_bps: Option<u32>,
    /// Enable in-band forward error correction (adds redundancy for the
    /// previous frame so a receiver can conceal single-packet loss).
    pub inband_fec: bool,
    /// Expected packet loss percentage (0-100), a hint used to size FEC
    /// redundancy when `inband_fec` is set. Ignored when `inband_fec` is `false`.
    pub packet_loss_percent: u8,
}

impl OpusEncoderConfig {
    /// Config for `sample_rate`/`channels`/`application`/`time_base` with no
    /// bitrate override and FEC disabled — set `bitrate_bps` /
    /// `inband_fec` / `packet_loss_percent` explicitly for real-time voice use.
    #[must_use]
    pub const fn new(
        sample_rate: u32,
        channels: u16,
        application: OpusApplication,
        time_base: Rational,
    ) -> Self {
        Self {
            sample_rate,
            channels,
            application,
            time_base,
            bitrate_bps: None,
            inband_fec: false,
            packet_loss_percent: 0,
        }
    }
}

/// Parameters for opening an [`crate::opus::OpusDecoder`] session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpusDecoderConfig {
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Duration of one decoded output [`mediaway_common::AudioFrame`], in
    /// seconds — also the upper bound on decode buffer capacity per
    /// [`crate::opus::OpusDecoder::push_packet`] call (see that method's
    /// costly-path doc for the buffer-capacity caveat).
    pub time_base: Rational,
}

impl OpusDecoderConfig {
    /// Config for `sample_rate`/`channels`/`time_base`.
    #[must_use]
    pub const fn new(sample_rate: u32, channels: u16, time_base: Rational) -> Self {
        Self {
            sample_rate,
            channels,
            time_base,
        }
    }
}

/// Computes the exact PCM sample count for one Opus frame from `sample_rate`
/// and `time_base` (interpreted as the frame duration in seconds).
///
/// # Errors
///
/// Returns [`OpusError::InvalidFrameDuration`] when `time_base.den` is zero,
/// `sample_rate * time_base.num` does not divide evenly by `time_base.den`,
/// or the result is zero or does not fit `i32` (`unsafe-libopus`'s own frame
/// size parameter type).
pub(crate) fn frame_size_samples(
    sample_rate: u32,
    time_base: Rational,
) -> Result<usize, OpusError> {
    let invalid = || OpusError::InvalidFrameDuration {
        num: time_base.num,
        den: time_base.den,
        sample_rate,
    };
    if time_base.den == 0 {
        return Err(invalid());
    }
    let numerator = u64::from(sample_rate) * time_base.num;
    let denominator = u64::from(time_base.den);
    if !numerator.is_multiple_of(denominator) {
        return Err(invalid());
    }
    let samples = numerator / denominator;
    if samples == 0 || samples > u64::from(u32::MAX / 2) {
        return Err(invalid());
    }
    usize::try_from(samples).map_err(|_| invalid())
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
