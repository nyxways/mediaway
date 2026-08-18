//! AV1 OBU framing — `leb128()` size decoding and `obu_header()` splitting (AV1 spec §4.10.5,
//! §5.3.1, §5.3.2).
//!
//! AV1's own framing (length-prefixed OBUs, no start codes/emulation prevention) is
//! structurally simpler than H.264/HEVC Annex-B. This reader is the inverse of
//! `mediaway-encoder`'s `windows::d3d12_video_encode::bitstream_av1::{write_leb128,
//! obu_header_byte}` (`bitstream_av1.rs:40-56`) — same spec sections, same byte-layout
//! knowledge already validated by that writer's D3D12-driver-accepted output, but the
//! `leb128()` **decoding** loop itself (continuation-bit accumulation) is new code: the writer
//! only ever encodes. See
//! [ADR-0003](../../../../adr/linux/0003-vaapi-av1-key-frame-decode.md) § Bitstream parsing.

#![forbid(unsafe_code)]

use crate::DecodeError;

/// `obu_type` values this crate's OBU scanner recognizes (AV1 spec §6.2.2).
pub(super) const OBU_SEQUENCE_HEADER: u8 = 1;
#[allow(
    dead_code,
    reason = "exercised by obu_tests.rs and documents the OBU type table; av1.rs's \
              push_packet dispatch handles OBU_TEMPORAL_DELIMITER via its wildcard arm \
              (identical no-op body to every other ignored OBU type, so clippy::match_same_arms \
              forbids a separate named arm) rather than by name"
)]
pub(super) const OBU_TEMPORAL_DELIMITER: u8 = 2;
pub(super) const OBU_FRAME_HEADER: u8 = 3;
pub(super) const OBU_TILE_GROUP: u8 = 4;
pub(super) const OBU_FRAME: u8 = 6;

/// One parsed OBU: its `obu_type` plus the byte range of its payload within the caller's
/// buffer (after the header byte and `leb128` size field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Obu<'a> {
    pub(super) obu_type: u8,
    pub(super) payload: &'a [u8],
}

/// `leb128()` (AV1 spec §4.10.5): little-endian base-128 with a continuation bit. Returns the
/// decoded value and the number of bytes consumed (spec caps this at 8 bytes).
///
/// # Errors
///
/// Returns [`DecodeError::InvalidInput`] on truncated input or an encoding exceeding the
/// spec's 8-byte maximum.
pub(super) fn read_leb128(data: &[u8]) -> Result<(u64, usize), DecodeError> {
    let mut value: u64 = 0;
    for i in 0..8usize {
        let byte = *data.get(i).ok_or(DecodeError::InvalidInput)?;
        value |= u64::from(byte & 0x7f) << (i * 7);
        if byte & 0x80 == 0 {
            return Ok((value, i + 1));
        }
    }
    Err(DecodeError::InvalidInput)
}

/// Split `data` (one packet's full OBU stream) into successive OBUs.
///
/// Every OBU must carry an explicit `leb128`-coded size field (`obu_has_size_field == 1`) —
/// this crate assumes the low-overhead bitstream format's self-delimiting-OBU convention (see
/// ADR-0003 § Scope); a stream relying on external framing to delimit its last OBU, or an OBU
/// extension header (temporal/spatial scalability), is rejected as [`DecodeError::Unsupported`].
///
/// # Errors
///
/// Returns [`DecodeError::InvalidInput`] on truncated/malformed data (including a nonzero
/// `obu_forbidden_bit`), or [`DecodeError::Unsupported`] for `obu_extension_flag == 1` or
/// `obu_has_size_field == 0`.
pub(super) fn split_obus(data: &[u8]) -> Result<Vec<Obu<'_>>, DecodeError> {
    let mut obus = Vec::new();
    let mut offset = 0usize;
    while offset < data.len() {
        let header_byte = *data.get(offset).ok_or(DecodeError::InvalidInput)?;
        if header_byte & 0x80 != 0 {
            // obu_forbidden_bit must be 0 (AV1 spec §5.3.2).
            return Err(DecodeError::InvalidInput);
        }
        let obu_type = (header_byte >> 3) & 0b1111;
        let obu_extension_flag = (header_byte >> 2) & 1;
        let obu_has_size_field = (header_byte >> 1) & 1;
        if obu_extension_flag != 0 {
            return Err(DecodeError::Unsupported);
        }
        if obu_has_size_field == 0 {
            return Err(DecodeError::Unsupported);
        }
        let after_header = offset.checked_add(1).ok_or(DecodeError::InvalidInput)?;
        let size_field = data.get(after_header..).ok_or(DecodeError::InvalidInput)?;
        let (size, size_len) = read_leb128(size_field)?;
        let payload_start = after_header
            .checked_add(size_len)
            .ok_or(DecodeError::InvalidInput)?;
        let size = usize::try_from(size).map_err(|_| DecodeError::InvalidInput)?;
        let payload_end = payload_start
            .checked_add(size)
            .ok_or(DecodeError::InvalidInput)?;
        let payload = data
            .get(payload_start..payload_end)
            .ok_or(DecodeError::InvalidInput)?;
        obus.push(Obu { obu_type, payload });
        offset = payload_end;
    }
    Ok(obus)
}

#[cfg(test)]
#[path = "obu_tests.rs"]
mod tests;
