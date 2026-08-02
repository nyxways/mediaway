//! H.264 4x4 integer inverse transform and dequantization (ITU-T H.264 § 8.5.9-8.5.13).
//!
//! Flat/default scaling lists only (`weightScale(i,j) == 16` everywhere,
//! `Flat_4x4_16`, § 8.5.9) — [`super::Sps::parse`] already only skips past
//! `scaling_list()` bodies rather than retaining their values (see that module's docs),
//! so this crate structurally cannot honor a stream that signals custom scaling lists;
//! flat scaling is by far the common case for Baseline-profile encoders. A stream with
//! non-default scaling lists will still decode without panicking, just with numerically
//! wrong dequantized values — a known, documented limitation (see
//! `adr/0003-cavlc-i-slice-first-decode.md`), not a silently-assumed one.
//!
//! **Provenance / verification:** [`NORM_ADJUST`] and [`QPI_TO_QPC`] were cross-checked
//! against `FFmpeg`'s `h264data.c` numeric constants (read only to fact-check the numbers
//! independently recalled from the spec, not copied as code). The luma Plane intra-pred
//! formula in [`super::intra_pred`] was independently verified via an algebraically
//! equivalent alternate formulation found in a public reference during this session; the
//! chroma Plane-mode constants were not — see the ADR's "Negative / Trade-offs" section.

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "raster/QP indices below are bounded by fixed 4x4/QP (0..=51) ranges checked \
              at each cast site or by the caller (running QP state uses checked wraparound)"
)]

use super::error::H264Error;

/// `normAdjust` (ITU-T H.264 § 8.5.9, the `m`-indexed part of `LevelScale4x4`), indexed
/// `[qP % 6][position class]` where position class `0` = both row/col even, `1` = both
/// odd, `2` = mixed (§ 8.5.9's `v` pattern).
const NORM_ADJUST: [[i64; 3]; 6] = [
    [10, 16, 13],
    [11, 18, 14],
    [13, 20, 16],
    [14, 23, 18],
    [16, 25, 20],
    [18, 29, 23],
];

/// Default (`Flat_4x4_16`) weight scale factor used everywhere — see module docs on the
/// custom-scaling-list scope cut.
const FLAT_WEIGHT_SCALE: i64 = 16;

/// `QPi` (`Clip3(0, 51, QPy + chroma_qp_index_offset)`) to `QPc` mapping for `QPi` `30..=51`
/// (ITU-T H.264 Table 8-15); `QPi < 30` maps to itself.
const QPI_TO_QPC: [i32; 22] = [
    29, 30, 31, 32, 32, 33, 34, 34, 35, 35, 36, 36, 37, 37, 37, 38, 38, 38, 39, 39, 39, 39,
];

/// Derive the chroma quantization parameter `QPc` from the luma `qp` and
/// `pps.chroma_qp_index_offset` (ITU-T H.264 § 8.5.8, Table 8-15).
#[must_use]
pub(super) fn qpc_from_qp(qp: i32, chroma_qp_index_offset: i32) -> i32 {
    let qpi = (qp + chroma_qp_index_offset).clamp(0, 51);
    if qpi < 30 {
        qpi
    } else {
        QPI_TO_QPC[usize::try_from(qpi - 30).unwrap_or(0)]
    }
}

/// Position class (`v(i,j)` in § 8.5.9) for [`NORM_ADJUST`]'s second index.
const fn position_class(row: usize, col: usize) -> usize {
    if row % 2 == 0 && col % 2 == 0 {
        0
    } else if row % 2 == 1 && col % 2 == 1 {
        1
    } else {
        2
    }
}

/// Dequantize the AC/normal coefficients of a 4x4 block (ITU-T H.264 § 8.5.12.1). `raster`
/// position `0` (the DC coefficient) is dequantized the same way as any other position —
/// callers decoding an `I_16x16` macroblock overwrite raster position `0` afterward with
/// the separately-dequantized luma DC value (see [`dequant_luma_dc`]); AC-only residual
/// blocks already carry `0` at that position, so dequantizing it is a harmless no-op there.
///
/// # Errors
///
/// [`H264Error::FieldOverflow`] if any intermediate product overflows `i64` (only reachable
/// with an adversarially large decoded coefficient level).
pub(super) fn dequant_normal(raster: &[i32; 16], qp: i32) -> Result<[i32; 16], H264Error> {
    let mut out = [0i32; 16];
    for row in 0..4 {
        for col in 0..4 {
            let idx = row * 4 + col;
            let c = i64::from(raster[idx]);
            if c == 0 {
                continue;
            }
            out[idx] = dequant_scalar(c, qp, position_class(row, col))?;
        }
    }
    Ok(out)
}

/// Shared dequant arithmetic (§ 8.5.12.1's `qP >= 24` branch and its complement), used by
/// both normal-coefficient and DC-coefficient dequantization with different `qP` ranges.
fn dequant_scalar(c: i64, qp: i32, position_class: usize) -> Result<i32, H264Error> {
    let level_scale = FLAT_WEIGHT_SCALE
        .checked_mul(NORM_ADJUST[usize::try_from(qp.rem_euclid(6)).unwrap_or(0)][position_class])
        .ok_or(H264Error::FieldOverflow)?;
    let product = c.checked_mul(level_scale).ok_or(H264Error::FieldOverflow)?;
    let shift = qp.div_euclid(6);
    let d = if qp >= 24 {
        let up = u32::try_from(shift - 4).map_err(|_err| H264Error::FieldOverflow)?;
        product.checked_shl(up).ok_or(H264Error::FieldOverflow)?
    } else {
        let down = u32::try_from(4 - shift).map_err(|_err| H264Error::FieldOverflow)?;
        let round = 1i64.checked_shl(down - 1).ok_or(H264Error::FieldOverflow)?;
        product
            .checked_add(round)
            .ok_or(H264Error::FieldOverflow)?
            .checked_shr(down)
            .ok_or(H264Error::FieldOverflow)?
    };
    i32::try_from(d).map_err(|_err| H264Error::FieldOverflow)
}

/// Dequantize 16 luma DC coefficients (already inverse-Hadamard-transformed by
/// [`inverse_hadamard_4x4`]) using the luma QP (ITU-T H.264 § 8.5.10). `raster` is indexed
/// the same way as a normal 4x4 block (`row * 4 + col`), but here each entry is one
/// `I_16x16` luma 4x4 sub-block's DC value, not a pixel-position coefficient.
///
/// # Errors
///
/// [`H264Error::FieldOverflow`] on adversarial-input arithmetic overflow.
pub(super) fn dequant_luma_dc(raster: &[i32; 16], qp: i32) -> Result<[i32; 16], H264Error> {
    let mut out = [0i32; 16];
    for (idx, &f) in raster.iter().enumerate() {
        let c = i64::from(f);
        if c == 0 {
            out[idx] = 0;
            continue;
        }
        let level_scale = FLAT_WEIGHT_SCALE
            .checked_mul(NORM_ADJUST[usize::try_from(qp.rem_euclid(6)).unwrap_or(0)][0])
            .ok_or(H264Error::FieldOverflow)?;
        let product = c.checked_mul(level_scale).ok_or(H264Error::FieldOverflow)?;
        let shift = qp.div_euclid(6);
        let d = if qp >= 36 {
            let up = u32::try_from(shift - 6).map_err(|_err| H264Error::FieldOverflow)?;
            product.checked_shl(up).ok_or(H264Error::FieldOverflow)?
        } else {
            let down = u32::try_from(6 - shift).map_err(|_err| H264Error::FieldOverflow)?;
            let round = 1i64.checked_shl(down - 1).ok_or(H264Error::FieldOverflow)?;
            product
                .checked_add(round)
                .ok_or(H264Error::FieldOverflow)?
                .checked_shr(down)
                .ok_or(H264Error::FieldOverflow)?
        };
        out[idx] = i32::try_from(d).map_err(|_err| H264Error::FieldOverflow)?;
    }
    Ok(out)
}

/// Dequantize the chroma DC 2x2 block (already inverse-Hadamard-transformed by
/// [`inverse_hadamard_2x2`]) using the chroma QP (ITU-T H.264 § 8.5.11.2, `ChromaArrayType
/// == 1` / 4:2:0 only — the only formula this crate needs).
///
/// # Errors
///
/// [`H264Error::FieldOverflow`] on adversarial-input arithmetic overflow.
pub(super) fn dequant_chroma_dc(c: &[i32; 4], qpc: i32) -> Result<[i32; 4], H264Error> {
    let mut out = [0i32; 4];
    for (idx, &f) in c.iter().enumerate() {
        let value = i64::from(f);
        if value == 0 {
            continue;
        }
        let level_scale = FLAT_WEIGHT_SCALE
            .checked_mul(NORM_ADJUST[usize::try_from(qpc.rem_euclid(6)).unwrap_or(0)][0])
            .ok_or(H264Error::FieldOverflow)?;
        let product = value
            .checked_mul(level_scale)
            .ok_or(H264Error::FieldOverflow)?;
        let up = u32::try_from(qpc.div_euclid(6)).map_err(|_err| H264Error::FieldOverflow)?;
        let shifted = product.checked_shl(up).ok_or(H264Error::FieldOverflow)?;
        out[idx] = i32::try_from(shifted >> 5).map_err(|_err| H264Error::FieldOverflow)?;
    }
    Ok(out)
}

/// Inverse 4x4 core transform (ITU-T H.264 § 8.5.12.2): column pass, row pass, then the
/// final `(x + 32) >> 6` rounding/normalization. `d` holds dequantized coefficients in
/// raster order; the result is residual sample values (still signed, not yet added to a
/// prediction or clipped to `0..=255`).
#[must_use]
pub(super) fn inverse_transform_4x4(d: &[i32; 16]) -> [i32; 16] {
    let butterflied = hadamard_like_pass(d, false);
    let mut out = [0i32; 16];
    for (idx, &value) in butterflied.iter().enumerate() {
        out[idx] = (value + 32) >> 6;
    }
    out
}

/// Inverse 4x4 Hadamard transform for the 16 `I_16x16` luma DC coefficients (ITU-T H.264
/// § 8.5.10, applied before [`dequant_luma_dc`] — raw levels in, raw Hadamard output out,
/// no rounding/shift at this stage since that is folded into the DC dequant formula).
#[must_use]
pub(super) fn inverse_hadamard_4x4(c: &[i32; 16]) -> [i32; 16] {
    hadamard_like_pass(c, true)
}

/// Shared column-then-row separable butterfly for both the core inverse transform and the
/// luma DC Hadamard transform — they use the same structure, differing only in whether the
/// odd-indexed intermediate terms are halved (`>> 1`, core transform) or not (Hadamard is a
/// pure +/-1 transform).
fn hadamard_like_pass(input: &[i32; 16], is_hadamard: bool) -> [i32; 16] {
    // Column pass: raster index `row * 4 + col`.
    let mut cols = [0i32; 16];
    for col in 0..4 {
        let c0 = input[col];
        let c1 = input[4 + col];
        let c2 = input[8 + col];
        let c3 = input[12 + col];
        let (e0, e1, e2, e3) = butterfly(c0, c1, c2, c3, is_hadamard);
        cols[col] = e0 + e3;
        cols[4 + col] = e1 + e2;
        cols[8 + col] = e1 - e2;
        cols[12 + col] = e0 - e3;
    }
    // Row pass.
    let mut out = [0i32; 16];
    for row in 0..4 {
        let base = row * 4;
        let r0 = cols[base];
        let r1 = cols[base + 1];
        let r2 = cols[base + 2];
        let r3 = cols[base + 3];
        let (e0, e1, e2, e3) = butterfly(r0, r1, r2, r3, is_hadamard);
        out[base] = e0 + e3;
        out[base + 1] = e1 + e2;
        out[base + 2] = e1 - e2;
        out[base + 3] = e0 - e3;
    }
    out
}

/// One 1-D butterfly stage shared by the core transform and the Hadamard transform.
/// Overflow analysis: inputs here are dequantized coefficients or their first-pass
/// intermediates; a coefficient level is always a valid `i32` (CAVLC decode only returns
/// values that fit `i32`, see `cavlc::decode_levels`) and dequantization bounds its
/// `i64`-checked output back to `i32`, so these `i32` add/sub/shift operations stay many
/// orders of magnitude below `i32`'s overflow boundary for any input this crate's own
/// decode/dequant stages can produce.
const fn butterfly(v0: i32, v1: i32, v2: i32, v3: i32, is_hadamard: bool) -> (i32, i32, i32, i32) {
    if is_hadamard {
        (v0 + v2, v0 - v2, v1 - v3, v1 + v3)
    } else {
        (v0 + v2, v0 - v2, (v1 >> 1) - v3, v1 + (v3 >> 1))
    }
}

/// Inverse 2x2 Hadamard transform for the chroma DC block (ITU-T H.264 § 8.5.11.1),
/// `c == [c(0,0), c(0,1), c(1,0), c(1,1)]`.
#[must_use]
pub(super) const fn inverse_hadamard_2x2(c: &[i32; 4]) -> [i32; 4] {
    let (c00, c01, c10, c11) = (c[0], c[1], c[2], c[3]);
    [
        c00 + c01 + c10 + c11,
        c00 - c01 + c10 - c11,
        c00 + c01 - c10 - c11,
        c00 - c01 - c10 + c11,
    ]
}

#[cfg(test)]
#[path = "transform_tests.rs"]
mod tests;
