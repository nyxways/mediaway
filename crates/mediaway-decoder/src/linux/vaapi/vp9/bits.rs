//! `s(n)` — the VP9 Bitstream & Decoding Process Specification's own signed-value decoder
//! (§4.9's generic syntax-element-type table, §6.2.8/§6.2.10's real use sites
//! `loop_filter_ref_deltas[i] s(6)` / `delta_q s(4)`), built on
//! [`mediaway_sw::h264::BitReader`]'s raw MSB-first `read_bit`/`read_bits` primitives (same
//! reuse precedent as this crate's H.264/AV1 siblings).
//!
//! **Real correction versus an earlier draft of this crate's own ADR**: VP9 `s(n)` is NOT the
//! same bit layout as AV1's `su(n)`. AV1's `su(n)` reads `n` bits *once* and reinterprets the
//! top bit of that same `n`-bit field as a sign (the value's magnitude and sign share one
//! `n`-bit read). VP9's `s(n)` instead reads `n` bits as a plain unsigned magnitude via `f(n)`,
//! then reads **one additional, separate** bit as the sign (`1` = negative) — `n + 1` bits
//! total, not `n`. This crate's own `adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md`
//! Addendum confirms this via the real primary spec text (fetched and extracted this session),
//! correcting an earlier open question that had assumed the `su(n)` shape instead.

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

/// `s(n)` (VP9 spec §4.9): read an `n`-bit unsigned magnitude, then one more bit as sign
/// (`1` = negative). `n` must be `1..=31` (this crate's only real call sites use `4` and `6`).
pub(super) fn s(r: &mut BitReader<'_>, n: u32) -> Result<i32, DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let magnitude = r.read_bits(n).map_err(map_err)?;
    let sign = r.read_bit().map_err(map_err)?;
    let magnitude = i32::try_from(magnitude).map_err(|_| DecodeError::InvalidInput)?;
    Ok(if sign != 0 { -magnitude } else { magnitude })
}

#[cfg(test)]
#[path = "bits_tests.rs"]
mod tests;
