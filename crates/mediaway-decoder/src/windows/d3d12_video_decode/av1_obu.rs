//! AV1 OBU framing: `leb128()` (AV1 spec §4.10.5) + `obu_header()` (§5.3.2) read-side,
//! plus `split_obus()` splitting one packet's bytes into `(obu_type, payload)` pairs.
//!
//! **Not `mediaway_sw::h264::split_annex_b`** — AV1 does not use Annex-B start-code
//! framing at all (ADR-0005 § Reuse). This is the read-side mirror of
//! `mediaway-encoder-windows`'s `d3d12_video_encode/bitstream_av1.rs::{write_leb128,
//! obu_header_byte, wrap_obu}` (same spec sections, reversed direction) — a real "port the
//! shape, not the code" relationship, since that source is a writer.

#![forbid(unsafe_code)]

use crate::DecodeError;

/// AV1 OBU types this module's dispatch acts on (AV1 spec §6.2.2 Table 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ObuType {
    SequenceHeader,
    TemporalDelimiter,
    FrameHeader,
    TileGroup,
    Frame,
    RedundantFrameHeader,
    /// Metadata / padding / reserved types this module's decode dispatch safely skips.
    Other(u8),
}

impl ObuType {
    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::SequenceHeader,
            2 => Self::TemporalDelimiter,
            3 => Self::FrameHeader,
            4 => Self::TileGroup,
            6 => Self::Frame,
            7 => Self::RedundantFrameHeader,
            other => Self::Other(other),
        }
    }
}

/// One parsed OBU: type + payload bytes (header byte and `leb128` size field stripped).
#[derive(Debug, Clone, Copy)]
pub(super) struct Obu<'a> {
    pub(super) obu_type: ObuType,
    pub(super) payload: &'a [u8],
}

/// `leb128()` (AV1 spec §4.10.5): little-endian base-128 with a continuation bit, capped
/// at 8 bytes per the spec's own `leb128_bytes <= 8` constraint (matches the encoder-side
/// writer's `u64` value domain).
///
/// Returns `(value, bytes_consumed)`.
///
/// # Errors
///
/// [`DecodeError::InvalidInput`] on truncated input or a `leb128` value that does not
/// terminate within 8 bytes.
pub(super) fn read_leb128(data: &[u8]) -> Result<(u64, usize), DecodeError> {
    let mut value: u64 = 0;
    for i in 0..8usize {
        let &byte = data.get(i).ok_or(DecodeError::InvalidInput)?;
        let low7 = u64::from(byte & 0x7f);
        let shift = u32::try_from(i * 7).unwrap_or(u32::MAX);
        value |= low7.checked_shl(shift).ok_or(DecodeError::InvalidInput)?;
        if byte & 0x80 == 0 {
            return Ok((value, i + 1));
        }
    }
    Err(DecodeError::InvalidInput)
}

/// `obu_header()` (AV1 spec §5.3.2) for a non-extended OBU. Returns `(obu_type,
/// obu_has_size_field)`.
///
/// # Errors
///
/// [`DecodeError::InvalidInput`] if `obu_forbidden_bit != 0`.
/// [`DecodeError::Unsupported`] if `obu_extension_flag == 1` — temporal/spatial-layer
/// extension is out of this module's scope (ADR-0005 § Scope decision has no layering
/// concept at all).
const fn parse_obu_header(byte: u8) -> Result<(ObuType, bool), DecodeError> {
    let forbidden_bit = (byte >> 7) & 1;
    if forbidden_bit != 0 {
        return Err(DecodeError::InvalidInput);
    }
    let obu_type = (byte >> 3) & 0b1111;
    let extension_flag = (byte >> 2) & 1;
    if extension_flag != 0 {
        return Err(DecodeError::Unsupported);
    }
    let has_size_field = (byte >> 1) & 1;
    Ok((ObuType::from_u8(obu_type), has_size_field != 0))
}

/// Split one packet's bytes into a sequence of OBUs (temporal delimiter, sequence header,
/// and frame OBU — this module's own encoder-side session-prefix shape; AV1 spec §5.2/§5.3
/// places no constraint on which OBU types may appear in one packet).
///
/// # Errors
///
/// [`DecodeError::InvalidInput`] on truncated/malformed framing.
/// [`DecodeError::Unsupported`] on an OBU with `obu_extension_flag == 1` or
/// `obu_has_size_field == 0` — this module requires every OBU to carry an explicit
/// `leb128` length (AV1 spec §5.3.2's own recommended low-overhead-bitstream-format
/// practice; every OBU this backend's own encoder emits sets this bit, see
/// `mediaway-encoder-windows`'s `bitstream_av1.rs::obu_header_byte`).
pub(super) fn split_obus(data: &[u8]) -> Result<Vec<Obu<'_>>, DecodeError> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let &header_byte = data.get(pos).ok_or(DecodeError::InvalidInput)?;
        let (obu_type, has_size_field) = parse_obu_header(header_byte)?;
        if !has_size_field {
            return Err(DecodeError::Unsupported);
        }
        pos += 1;
        let (size, size_len) = read_leb128(data.get(pos..).ok_or(DecodeError::InvalidInput)?)?;
        pos += size_len;
        let size = usize::try_from(size).map_err(|_err| DecodeError::InvalidInput)?;
        let end = pos.checked_add(size).ok_or(DecodeError::InvalidInput)?;
        let payload = data.get(pos..end).ok_or(DecodeError::InvalidInput)?;
        out.push(Obu { obu_type, payload });
        pos = end;
    }
    Ok(out)
}

#[cfg(test)]
#[path = "av1_obu_tests.rs"]
mod tests;
