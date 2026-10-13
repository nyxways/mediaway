//! AAC ADTS framing.

#![forbid(unsafe_code)]

use bytes::Bytes;

/// Strip ADTS if present; return raw AAC and optional `AudioSpecificConfig`.
#[must_use]
pub fn strip_adts(data: &[u8]) -> (Bytes, Option<Bytes>) {
    if data.len() < 7 || data[0] != 0xff || (data[1] & 0xf0) != 0xf0 {
        return (Bytes::copy_from_slice(data), None);
    }
    let protection_absent = (data[1] & 0x01) != 0;
    let header_len = if protection_absent { 7 } else { 9 };
    if data.len() < header_len {
        return (Bytes::copy_from_slice(data), None);
    }
    let object_type = ((data[2] >> 6) & 0x03) + 1;
    let sample_rate_index = (data[2] >> 2) & 0x0f;
    let channels = ((data[2] & 0x01) << 2) | ((data[3] >> 6) & 0x03);
    let a = (object_type << 3) | (sample_rate_index >> 1);
    let b = (sample_rate_index << 7) | (channels << 3);
    (
        Bytes::copy_from_slice(&data[header_len..]),
        Some(Bytes::from(vec![a, b])),
    )
}

/// What an `AudioSpecificConfig` says about the sound (ISO/IEC 14496-3 § 1.6.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioConfigInfo {
    /// The core sample rate in Hz (the `samplingFrequencyIndex`, or the explicit 24-bit rate).
    pub sample_rate: u32,
    /// The channel count `channelConfiguration` stands for; 0 when it does not state one (an
    /// in-band program config element, or a reserved value).
    pub channels: u16,
}

/// The sample rate and channel count in an `AudioSpecificConfig`; `None` when it is too short or
/// the rate index is reserved.
#[must_use]
pub fn parse_audio_specific_config(asc: &[u8]) -> Option<AudioConfigInfo> {
    const RATES: [u32; 13] = [
        96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025,
        8_000, 7_350,
    ];
    let mut at = 0usize;
    let mut read = |n: usize| -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = *asc.get(at / 8)?;
            v = (v << 1) | u32::from((byte >> (7 - at % 8)) & 1);
            at += 1;
        }
        Some(v)
    };
    if read(5)? == 31 {
        // audioObjectType escape: the real type follows in 6 more bits, which are not needed here.
        read(6)?;
    }
    let rate = match read(4)? {
        15 => read(24)?,
        index => *RATES.get(usize::try_from(index).ok()?)?,
    };
    let channels = match read(4)? {
        7 => 8,
        n @ 1..=6 => u16::try_from(n).ok()?,
        _ => 0,
    };
    Some(AudioConfigInfo {
        sample_rate: rate,
        channels,
    })
}
