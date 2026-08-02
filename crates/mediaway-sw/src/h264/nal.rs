//! Annex-B / AVCC NAL unit splitting, header parsing, and emulation-prevention removal
//! (ITU-T H.264 § 7.3.1, § 7.4.1.1, Annex B).

#![forbid(unsafe_code)]

use super::error::H264Error;
use mediaway_common::Bytes;

/// H.264 NAL unit type (`nal_unit_type`, ITU-T H.264 Table 7-1).
///
/// Only the values relevant to bitstream framing and header parsing get named variants;
/// anything else is carried as [`NalUnitType::Other`] so callers still see the raw value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NalUnitType {
    /// Coded slice of a non-IDR picture (type 1).
    NonIdrSlice,
    /// Coded slice data partition A/B/C (types 2-4).
    SliceDataPartition,
    /// Coded slice of an IDR picture (type 5).
    IdrSlice,
    /// Supplemental enhancement information (type 6).
    Sei,
    /// Sequence parameter set (type 7).
    Sps,
    /// Picture parameter set (type 8).
    Pps,
    /// Access unit delimiter (type 9).
    AccessUnitDelimiter,
    /// End of sequence (type 10).
    EndOfSequence,
    /// End of stream (type 11).
    EndOfStream,
    /// Filler data (type 12).
    FillerData,
    /// Any other type value (extensions, reserved, or unspecified), raw value kept.
    Other(u8),
}

impl NalUnitType {
    /// Decode the 5-bit `nal_unit_type` field.
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::NonIdrSlice,
            2..=4 => Self::SliceDataPartition,
            5 => Self::IdrSlice,
            6 => Self::Sei,
            7 => Self::Sps,
            8 => Self::Pps,
            9 => Self::AccessUnitDelimiter,
            10 => Self::EndOfSequence,
            11 => Self::EndOfStream,
            12 => Self::FillerData,
            other => Self::Other(other),
        }
    }
}

/// One parsed NAL unit: header fields plus RBSP payload (emulation-prevention bytes
/// removed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NalUnit {
    /// `nal_ref_idc` (`0..=3`): non-zero marks a reference picture / parameter set.
    pub ref_idc: u8,
    /// Decoded NAL unit type.
    pub unit_type: NalUnitType,
    /// Payload after the 1-byte NAL header, with `emulation_prevention_three_byte`
    /// removed (raw RBSP, ready for [`super::Sps::parse`] / [`super::Pps::parse`]).
    pub rbsp: Bytes,
}

impl NalUnit {
    /// Parse one NAL unit's header + RBSP from `data`, which must start at the NAL
    /// header byte (no start code / length prefix) and run to the end of this NAL unit
    /// (e.g. one element of [`split_annex_b`] or [`split_avcc`]).
    ///
    /// # Errors
    ///
    /// Returns [`H264Error::UnexpectedEof`] if `data` is empty.
    pub fn parse(data: &[u8]) -> Result<Self, H264Error> {
        let header = *data.first().ok_or(H264Error::UnexpectedEof)?;
        let ref_idc = (header >> 5) & 0b11;
        let unit_type = NalUnitType::from_u8(header & 0b1_1111);
        let rbsp = remove_emulation_prevention(&data[1..]);
        Ok(Self {
            ref_idc,
            unit_type,
            rbsp: Bytes::from(rbsp),
        })
    }
}

/// Remove `emulation_prevention_three_byte` (a `0x03` inserted after two consecutive
/// `0x00` bytes, ITU-T H.264 § 7.3.1) to recover the raw RBSP.
fn remove_emulation_prevention(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut zero_run = 0u32;
    for &byte in data {
        if zero_run >= 2 && byte == 0x03 {
            zero_run = 0;
            continue;
        }
        out.push(byte);
        zero_run = if byte == 0 { zero_run + 1 } else { 0 };
    }
    out
}

/// Positions (byte offset of the leading `0x00` of the 3-byte `00 00 01` marker) of every
/// Annex-B start code in `data`. A 4-byte `00 00 00 01` start code is one 3-byte marker
/// preceded by an extra `0x00`; that extra byte is left for [`trim_trailing_zeros`] to
/// strip from the *previous* NAL unit's tail, so it does not need special-casing here.
fn find_start_codes(data: &[u8]) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut index = 0usize;
    while index + 3 <= data.len() {
        if data[index] == 0 && data[index + 1] == 0 && data[index + 2] == 1 {
            positions.push(index);
            index += 3;
        } else {
            index += 1;
        }
    }
    positions
}

/// Drop trailing `0x00` padding bytes (trailing `cabac_zero_word` / start-code padding).
fn trim_trailing_zeros(data: &[u8]) -> &[u8] {
    let mut end = data.len();
    while end > 0 && data[end - 1] == 0 {
        end -= 1;
    }
    &data[..end]
}

/// Split Annex-B byte-stream data (`0x000001` / `0x00000001` start codes) into NAL unit
/// byte ranges (NAL header byte through the last non-zero byte before the next start
/// code, or end of input).
///
/// # Errors
///
/// Returns [`H264Error::NoStartCode`] if no start code is present anywhere in `data`.
pub fn split_annex_b(data: &[u8]) -> Result<Vec<&[u8]>, H264Error> {
    let marks = find_start_codes(data);
    if marks.is_empty() {
        return Err(H264Error::NoStartCode);
    }
    let mut units = Vec::with_capacity(marks.len());
    for pair in marks.windows(2) {
        let content_begin = pair[0] + 3;
        let content_end = pair[1];
        units.push(trim_trailing_zeros(&data[content_begin..content_end]));
    }
    if let Some(&last_mark) = marks.last() {
        units.push(trim_trailing_zeros(&data[last_mark + 3..]));
    }
    Ok(units)
}

/// Split AVCC length-prefixed NAL units (`AVCDecoderConfigurationRecord`-style framing,
/// as used by `extra_data` in [`mediaway_common::StreamInfo`]).
///
/// `length_size` is the number of bytes used for each length prefix (the
/// `lengthSizeMinusOne + 1` field of the AVC decoder configuration record; commonly `4`).
///
/// # Errors
///
/// Returns [`H264Error::InvalidLengthSize`] if `length_size` is not `1..=4`, or
/// [`H264Error::InvalidNalLength`] if a declared length prefix or NAL body would run past
/// the end of `data`.
pub fn split_avcc(data: &[u8], length_size: u8) -> Result<Vec<&[u8]>, H264Error> {
    if !(1..=4).contains(&length_size) {
        return Err(H264Error::InvalidLengthSize);
    }
    let length_size = usize::from(length_size);
    let mut units = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let prefix = data
            .get(pos..pos + length_size)
            .ok_or(H264Error::InvalidNalLength)?;
        let len = prefix
            .iter()
            .fold(0usize, |acc, &byte| (acc << 8) | usize::from(byte));
        pos += length_size;
        let unit = data
            .get(pos..pos + len)
            .ok_or(H264Error::InvalidNalLength)?;
        units.push(unit);
        pos += len;
    }
    Ok(units)
}

#[cfg(test)]
#[path = "nal_tests.rs"]
mod tests;
