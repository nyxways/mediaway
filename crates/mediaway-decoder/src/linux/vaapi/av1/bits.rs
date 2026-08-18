//! AV1-specific variable-length bit decoders — `uvlc()`, `su(n)`, `ns(n)` (AV1 Bitstream &
//! Decoding Process Specification §4.10.3, §4.10.6, §4.10.7) — built directly on
//! [`mediaway_sw::h264::BitReader`]'s raw MSB-first `read_bit`/`read_bits` primitives.
//!
//! AV1 never uses H.264/HEVC's `ue(v)`/`se(v)` Exp-Golomb, but the same raw-bit-extraction
//! primitive underlies both — see
//! [ADR-0003](../../../../adr/linux/0003-vaapi-av1-key-frame-decode.md) § Bitstream parsing
//! for why this crate reuses `BitReader` rather than re-implementing MSB-first bit reads, and
//! implements these three decoders locally (no H.264/HEVC equivalent exists to port from).

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

/// `uvlc()` (AV1 spec §4.10.3): count leading zero bits until a `1` "done" bit, then read that
/// many more bits and add `2^leadingZeros - 1` — the same shape as H.264/HEVC's `ue(v)`
/// Exp-Golomb, reimplemented locally (not delegated to [`BitReader::read_ue`]) because that
/// method's error type and its `leadingZeros >= 32` overflow behavior (an error) differ from
/// this function's ([`DecodeError`], and the spec's own "return `(1 << 32) - 1`" convention).
#[allow(
    dead_code,
    reason = "exercised by bits_tests.rs; no field in this crate's KEY_FRAME-only, \
              decoder-model-disabled scope currently needs uvlc() (AV1's other two \
              non-Exp-Golomb VLCs, su()/ns(), are both real call sites in \
              frame_header.rs/tile_info.rs) — kept for API completeness matching this ADR's \
              own 'implement AV1's own small set of variable-length decoders' design table \
              entry, ready for whatever future field (e.g. timing_info()) needs it"
)]
pub(super) fn uvlc(r: &mut BitReader<'_>) -> Result<u32, DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let mut leading_zeros = 0u32;
    loop {
        let done = r.read_bit().map_err(map_err)?;
        if done != 0 {
            break;
        }
        leading_zeros += 1;
        if leading_zeros >= 32 {
            return Ok(u32::MAX);
        }
    }
    if leading_zeros == 0 {
        return Ok(0);
    }
    let value = r.read_bits(leading_zeros).map_err(map_err)?;
    let bias = 1u32
        .checked_shl(leading_zeros)
        .unwrap_or(u32::MAX)
        .saturating_sub(1);
    Ok(value.saturating_add(bias))
}

/// `su(n)` (AV1 spec §4.10.6): read `n` bits as unsigned (`f(n)`), then reinterpret the top
/// bit as a sign (two's-complement-style: subtract `2^n` when set). `n` must be `1..=32`.
pub(super) fn su(r: &mut BitReader<'_>, n: u32) -> Result<i32, DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let value = r.read_bits(n).map_err(map_err)?;
    let sign_mask = 1u32.checked_shl(n - 1).ok_or(DecodeError::InvalidInput)?;
    let value = i64::from(value);
    let signed = if value & i64::from(sign_mask) != 0 {
        value - 2 * i64::from(sign_mask)
    } else {
        value
    };
    i32::try_from(signed).map_err(|_| DecodeError::InvalidInput)
}

/// `ns(n)` (AV1 spec §4.10.7): non-symmetric unsigned encoding of a value in `0..n`. Used by
/// `tile_info()`'s non-uniform tile-spacing path (`width_in_sbs_minus_1`/
/// `height_in_sbs_minus_1`, AV1 spec §5.9.15) — see [`super::tile_info`].
#[allow(
    clippy::many_single_char_names,
    reason = "w/m/v are the AV1 spec's own ns(n) variable names (§4.10.7); longer synonyms \
              would obscure the direct correspondence to the spec pseudocode"
)]
pub(super) fn ns(reader: &mut BitReader<'_>, n: u32) -> Result<u32, DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    if n <= 1 {
        return Ok(0);
    }
    // FloorLog2(n) + 1, i.e. the bit width needed to represent 0..=n-1.
    let w = u32::BITS - n.leading_zeros();
    let m = (1u32 << w) - n;
    let v = reader.read_bits(w - 1).map_err(map_err)?;
    if v < m {
        return Ok(v);
    }
    let extra_bit = reader.read_bit().map_err(map_err)?;
    Ok((v << 1) - m + extra_bit)
}

#[cfg(test)]
#[path = "bits_tests.rs"]
mod tests;
