//! Freestanding ADTS header types (no Mediaway dependency).

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "crate-private helpers used by mux.rs/demux.rs; module itself is private"
)]

/// MPEG-4 audio object type carried in ADTS's 2-bit `profile` field
/// (`profile = object_type - 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AacProfile {
    /// Object type 1.
    Main,
    /// Object type 2 — the common case for encoded AAC-LC streams.
    Lc,
    /// Object type 3.
    Ssr,
    /// Object type 4 (MPEG-4 ADTS only).
    Ltp,
}

impl AacProfile {
    pub(crate) const fn bits(self) -> u8 {
        match self {
            Self::Main => 0,
            Self::Lc => 1,
            Self::Ssr => 2,
            Self::Ltp => 3,
        }
    }

    pub(crate) const fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => Self::Main,
            2 => Self::Ssr,
            3 => Self::Ltp,
            _ => Self::Lc,
        }
    }
}

/// The 13 standard ADTS sampling frequencies, indexed 0..=12 (13..=15 are
/// reserved / "explicit frequency", not representable in an ADTS header alone).
const SAMPLE_RATES: [u32; 13] = [
    96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025, 8_000,
    7_350,
];

pub(crate) const fn sample_rate_from_index(index: u8) -> Option<u32> {
    if (index as usize) < SAMPLE_RATES.len() {
        Some(SAMPLE_RATES[index as usize])
    } else {
        None
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "SAMPLE_RATES has 13 entries; the index always fits u8"
)]
pub(crate) fn sampling_frequency_index(sample_rate: u32) -> Option<u8> {
    SAMPLE_RATES
        .iter()
        .position(|&rate| rate == sample_rate)
        .map(|i| i as u8)
}

/// Per-frame ADTS header fields (no CRC, single raw-data-block-per-frame — the
/// common case for AAC-LC streams).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdtsConfig {
    /// MPEG-4 audio object type.
    pub profile: AacProfile,
    /// Sample rate — must be one of the 13 standard ADTS rates.
    pub sample_rate: u32,
    /// Channel configuration (1..=7; 0 = channel config sent out-of-band, unsupported here).
    pub channels: u8,
}
