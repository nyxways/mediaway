//! Freestanding WAVE format types (no Mediaway dependency).

#![forbid(unsafe_code)]

/// `wFormatTag` sample encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SampleFormat {
    /// Integer PCM (`wFormatTag` 1).
    Pcm,
    /// IEEE float PCM (`wFormatTag` 3).
    Float,
}

impl SampleFormat {
    pub(crate) const fn tag(self) -> u16 {
        match self {
            Self::Pcm => 1,
            Self::Float => 3,
        }
    }

    pub(crate) const fn from_tag(tag: u16) -> Option<Self> {
        match tag {
            1 => Some(Self::Pcm),
            3 => Some(Self::Float),
            _ => None,
        }
    }
}

/// `fmt ` chunk contents — enough to interpret the raw PCM `data` chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveFormat {
    /// Sample encoding.
    pub sample_format: SampleFormat,
    /// Channel count.
    pub channels: u16,
    /// Samples per second.
    pub sample_rate: u32,
    /// Bits per sample (e.g. 16, 24, 32).
    pub bits_per_sample: u16,
}

impl WaveFormat {
    /// Bytes per frame (all channels, one sample period).
    #[must_use]
    pub const fn block_align(self) -> u16 {
        self.channels * (self.bits_per_sample / 8)
    }

    /// Bytes per second at this format.
    #[must_use]
    pub const fn byte_rate(self) -> u32 {
        self.sample_rate * self.block_align() as u32
    }
}
