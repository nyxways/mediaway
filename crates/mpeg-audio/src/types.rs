//! Freestanding MPEG audio frame-header types (no Mediaway dependency).
//!
//! Layer III only (v1 scope) — see crate-local ADR-0001.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "crate-private helpers used by mux.rs/demux.rs; module itself is private"
)]

/// MPEG audio version (2-bit `ID` field combined with the layer's version bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MpegVersion {
    /// MPEG Version 1 (44100/48000/32000 Hz family).
    Mpeg1,
    /// MPEG Version 2 (22050/24000/16000 Hz family).
    Mpeg2,
    /// MPEG Version 2.5 (11025/12000/8000 Hz family, unofficial low-rate extension).
    Mpeg25,
}

/// Channel mode (2-bit field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChannelMode {
    /// Stereo.
    Stereo,
    /// Joint stereo (intensity/MS).
    JointStereo,
    /// Dual mono (two independent channels).
    DualChannel,
    /// Mono.
    Mono,
}

impl ChannelMode {
    pub(crate) const fn bits(self) -> u8 {
        match self {
            Self::Stereo => 0,
            Self::JointStereo => 1,
            Self::DualChannel => 2,
            Self::Mono => 3,
        }
    }

    pub(crate) const fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => Self::Stereo,
            1 => Self::JointStereo,
            2 => Self::DualChannel,
            _ => Self::Mono,
        }
    }
}

const MPEG1_SAMPLE_RATES: [u32; 3] = [44_100, 48_000, 32_000];
const MPEG2_SAMPLE_RATES: [u32; 3] = [22_050, 24_000, 16_000];
const MPEG25_SAMPLE_RATES: [u32; 3] = [11_025, 12_000, 8_000];

const MPEG1_LAYER3_BITRATES_KBPS: [u16; 14] = [
    32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];
const MPEG2_LAYER3_BITRATES_KBPS: [u16; 14] =
    [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

pub(crate) const fn sample_rate_table(version: MpegVersion) -> [u32; 3] {
    match version {
        MpegVersion::Mpeg1 => MPEG1_SAMPLE_RATES,
        MpegVersion::Mpeg2 => MPEG2_SAMPLE_RATES,
        MpegVersion::Mpeg25 => MPEG25_SAMPLE_RATES,
    }
}

pub(crate) const fn bitrate_table(version: MpegVersion) -> [u16; 14] {
    match version {
        MpegVersion::Mpeg1 => MPEG1_LAYER3_BITRATES_KBPS,
        MpegVersion::Mpeg2 | MpegVersion::Mpeg25 => MPEG2_LAYER3_BITRATES_KBPS,
    }
}

/// Layer III frame-length coefficient: `floor(coeff * bitrate_bps / sample_rate) + padding`.
pub(crate) const fn frame_len_coefficient(version: MpegVersion) -> u32 {
    match version {
        MpegVersion::Mpeg1 => 144_000,
        MpegVersion::Mpeg2 | MpegVersion::Mpeg25 => 72_000,
    }
}

/// Layer III frame header fields.
///
/// The padding bit is **not** part of this struct: real Layer III streams flip it
/// per frame (a bit-reservoir accounting detail owned by the encoder, not the
/// container), so it is a parameter of [`FrameHeader::frame_len`] and
/// `Muxer::write_frame` instead of a fixed per-session value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// MPEG version.
    pub version: MpegVersion,
    /// Bitrate in kbps — must be one of the 14 standard values for `version` (Layer III).
    pub bitrate_kbps: u16,
    /// Sample rate — must be one of the 3 standard rates for `version`.
    pub sample_rate: u32,
    /// Channel mode.
    pub channel_mode: ChannelMode,
}

impl FrameHeader {
    /// Total frame length in bytes (4-byte header included), Layer III formula.
    #[must_use]
    pub const fn frame_len(self, padding: bool) -> usize {
        let coeff = frame_len_coefficient(self.version) * self.bitrate_kbps as u32;
        let base = coeff / self.sample_rate;
        base as usize + if padding { 1 } else { 0 }
    }
}
