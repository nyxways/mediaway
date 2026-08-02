//! EBML variable-length integer (VINT) decode — [RFC 8794](https://www.rfc-editor.org/rfc/rfc8794)
//! (`docs/standards/registry.toml` id `rfc-8794-ebml`).
//!
//! A VINT's first byte encodes its total length `L` (1..=8) as a unary
//! prefix: the position of the leading `1` bit (`VINT_MARKER`) counted from
//! the most significant bit gives `L`. Every bit after the marker — the rest
//! of the first byte plus all following bytes — is `VINT_DATA` (`7*L` bits).
//!
//! Two decodes share this shape but differ in what counts as "the value":
//! - **Element size** ([`decode_size`]): the marker is stripped; an all-1s
//!   `VINT_DATA` is the reserved "unknown size" sentinel ([`VintSize::unknown`]).
//! - **Element ID** ([`decode_id`]): the marker bit is *kept* — the ID is the
//!   raw `L` bytes read as a big-endian integer (RFC 8794 §7).
//!
//! These are public, low-level, and usable standalone (probe/debug tooling),
//! per the workspace "low-level APIs stay first-class" rule.

#![forbid(unsafe_code)]

use crate::Error;

/// Decoded element **size** VINT (marker stripped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VintSize {
    /// Size in bytes, or the `VINT_DATA` bit pattern when [`Self::unknown`].
    pub value: u64,
    /// `true` when every `VINT_DATA` bit is `1` (RFC 8794 "unknown size").
    pub unknown: bool,
}

/// Total VINT byte length `L` (1..=8) from the first byte's marker position.
///
/// `0x00` has no marker bit within 8 bytes — [`Error::ReservedVint`].
const fn vint_length(first_byte: u8) -> Result<u8, Error> {
    if first_byte == 0 {
        return Err(Error::ReservedVint);
    }
    // leading_zeros() on u8 is 0..=7 here (first_byte != 0), so +1 is 1..=8.
    Ok(first_byte.leading_zeros() as u8 + 1)
}

/// Decode an element **size** VINT at the start of `buf` (marker stripped).
///
/// Returns `(size, bytes_consumed)`. [`Error::Incomplete`] means `buf` is a
/// truncated prefix — callers feeding a growing sans-io buffer should wait
/// for more bytes and retry, not treat it as malformed.
///
/// # Errors
///
/// [`Error::Incomplete`] on a truncated buffer; [`Error::ReservedVint`] on an
/// invalid (all-zero) leading byte.
pub fn decode_size(buf: &[u8]) -> Result<(VintSize, usize), Error> {
    let first = *buf.first().ok_or(Error::Incomplete)?;
    let len = vint_length(first)? as usize;
    if buf.len() < len {
        return Err(Error::Incomplete);
    }
    let shift = 8 - len as u32; // 0..=7
    let mask = ((1u16 << shift) - 1) as u8;
    let mut value = u64::from(first & mask);
    for &b in &buf[1..len] {
        value = (value << 8) | u64::from(b);
    }
    let data_bits = 7 * len as u32; // 7..=56
    let unknown = value == (1u64 << data_bits) - 1;
    Ok((VintSize { value, unknown }, len))
}

/// Decode an element **ID** VINT at the start of `buf` (marker bits kept).
///
/// Returns `(id, bytes_consumed)`. `WebM` element IDs are at most 4 bytes;
/// a longer marker yields [`Error::Unsupported`] rather than overflowing.
///
/// # Errors
///
/// [`Error::Incomplete`] on a truncated buffer; [`Error::ReservedVint`] on an
/// invalid leading byte; [`Error::Unsupported`] for IDs longer than 4 bytes.
pub fn decode_id(buf: &[u8]) -> Result<(u32, usize), Error> {
    let first = *buf.first().ok_or(Error::Incomplete)?;
    let len = vint_length(first)? as usize;
    if len > 4 {
        return Err(Error::Unsupported("element ID longer than 4 bytes"));
    }
    if buf.len() < len {
        return Err(Error::Incomplete);
    }
    let mut value: u32 = 0;
    for &b in &buf[..len] {
        value = (value << 8) | u32::from(b);
    }
    Ok((value, len))
}

/// Encode an element **ID** (marker bits already included, matching
/// [`decode_id`]'s raw representation) into `out`, using the minimal byte
/// length that holds the value without a leading zero byte.
///
/// Never panics: `id == 0` has no valid EBML representation (every `ids`
/// constant this crate writes is non-zero), but rather than panic on
/// caller misuse this writes a single `0x00` byte — round-trips back to
/// [`Error::ReservedVint`] on decode instead of crashing the writer.
pub fn encode_id(id: u32, out: &mut Vec<u8>) {
    let len = (4 - (id.leading_zeros() / 8) as usize).max(1);
    out.extend_from_slice(&id.to_be_bytes()[4 - len..]);
}

/// Encode an element **size** VINT (marker stripped from `value`, marker bit
/// added on write) into `out`.
///
/// Uses the minimal byte length `L` (1..=8) that fits `value` in `7*L` data
/// bits. The all-1s `VINT_DATA` pattern is reserved for "unknown size"
/// ([`decode_size`]), so a `value` that would exactly fill all-1s bumps to
/// the next length.
///
/// Never panics: a `value` that doesn't fit even 8 bytes' worth of VINT data
/// (56 bits — not reachable for any size this crate itself ever writes, but
/// `push_frame`'s caller-supplied `track_number` is technically unbounded)
/// saturates to the largest representable 8-byte value rather than crashing.
pub fn encode_size(value: u64, out: &mut Vec<u8>) {
    let mut len = 1u32;
    while len < 8 && value >= (1u64 << (7 * len)) - 1 {
        len += 1;
    }
    let max_at_len = (1u64 << (7 * len)) - 2; // reserve the all-1s "unknown" pattern
    let value = value.min(max_at_len);
    let marker = 1u64 << (7 * len);
    let encoded = marker | value;
    out.extend_from_slice(&encoded.to_be_bytes()[8 - len as usize..]);
}

/// Write the reserved "unknown size" VINT of length `len` (1..=8) into `out`
/// — all `VINT_DATA` bits set to `1`, marker bit set.
///
/// Used for a `Segment` mux writes as always-unknown-size (streaming: total
/// length isn't known upfront). `len` outside `1..=8` clamps rather than
/// panics (this crate only ever calls it with the literal `4`; kept total
/// for a public fn).
pub fn encode_unknown_size(len: u8, out: &mut Vec<u8>) {
    let len = len.clamp(1, 8);
    // First byte: (len - 1) leading zero bits, then the marker bit and all
    // remaining data bits set to 1 — e.g. len=4 -> 0b0001_1111 (0x1F).
    let first = ((1u16 << (9 - len)) - 1) as u8;
    out.push(first);
    for _ in 1..len {
        out.push(0xFF);
    }
}

#[cfg(test)]
#[path = "vint_tests.rs"]
mod tests;
