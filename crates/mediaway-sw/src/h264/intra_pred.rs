//! H.264 `I_16x16` luma and chroma 8x8 intra prediction (ITU-T H.264 § 8.3.3, § 8.3.4).
//!
//! Only the four whole-block prediction modes each syntax element supports are
//! implemented — this crate's scope cut excludes `I_NxN` (4x4/8x8 per-sub-block intra
//! prediction), so the finer-grained modes from § 8.3.1/§ 8.3.2 are out of scope; see
//! `adr/0003-cavlc-i-slice-first-decode.md`.
//!
//! Every function here takes already-extracted neighbour sample arrays (not a picture
//! buffer/stride) so it stays a pure "samples in, prediction out" transform — [`super::decode`]
//! owns reading the reconstructed neighbour macroblocks' edge pixels and writing the
//! returned prediction block back into the picture.

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "sample values are 8-bit (0..=255); intermediate i32 arithmetic below is \
              explicitly clamped to that range before the final cast"
)]

use super::error::H264Error;

/// Average `values` with round-half-up, shifting by `shift` (i.e. dividing by `2^shift`).
fn rounded_average(values: &[u8], shift: u32) -> u8 {
    let sum: u32 = values.iter().map(|&v| u32::from(v)).sum();
    let round = 1u32 << (shift - 1);
    ((sum + round) >> shift) as u8
}

/// Predict a full 16x16 `I_16x16` luma block (ITU-T H.264 § 8.3.3).
///
/// `top`/`left` are the 16 reconstructed samples immediately above/left of the block (row
/// `-1`, column `-1`); `corner` is sample `(-1, -1)`. `None` means that neighbour is
/// unavailable (picture edge or first row/column of the picture in this crate's
/// single-slice-per-picture scope). Output is row-major, `out[y * 16 + x]`.
///
/// # Errors
///
/// [`H264Error::UnavailableIntraNeighbor`] if `mode` needs a neighbour that is `None`
/// (Vertical needs `top`; Horizontal needs `left`; Plane needs all three) — only reachable
/// from a non-conformant bitstream. [`H264Error::InvalidMbType`] if `mode > 3`.
pub(super) fn predict_16x16(
    mode: u8,
    top: Option<&[u8; 16]>,
    left: Option<&[u8; 16]>,
    corner: Option<u8>,
) -> Result<[u8; 256], H264Error> {
    match mode {
        0 => {
            let top = top.ok_or(H264Error::UnavailableIntraNeighbor)?;
            let mut out = [0u8; 256];
            for y in 0..16 {
                out[y * 16..y * 16 + 16].copy_from_slice(top);
            }
            Ok(out)
        }
        1 => {
            let left = left.ok_or(H264Error::UnavailableIntraNeighbor)?;
            let mut out = [0u8; 256];
            for (y, &sample) in left.iter().enumerate() {
                out[y * 16..y * 16 + 16].fill(sample);
            }
            Ok(out)
        }
        2 => Ok([luma_dc(top, left); 256]),
        3 => {
            let top = top.ok_or(H264Error::UnavailableIntraNeighbor)?;
            let left = left.ok_or(H264Error::UnavailableIntraNeighbor)?;
            let corner = corner.ok_or(H264Error::UnavailableIntraNeighbor)?;
            Ok(luma_plane(top, left, corner))
        }
        _ => Err(H264Error::InvalidMbType),
    }
}

/// `Intra16x16PredMode` DC (mode `2`): a single averaged value fills the whole block
/// (ITU-T H.264 § 8.3.3.3).
fn luma_dc(top: Option<&[u8; 16]>, left: Option<&[u8; 16]>) -> u8 {
    match (top, left) {
        (Some(t), Some(l)) => {
            let sum: u32 = t.iter().chain(l.iter()).map(|&v| u32::from(v)).sum();
            ((sum + 16) >> 5) as u8
        }
        (Some(t), None) => rounded_average(t, 4),
        (None, Some(l)) => rounded_average(l, 4),
        (None, None) => 128,
    }
}

/// `Intra16x16PredMode` Plane (mode `3`, ITU-T H.264 § 8.3.3.4).
#[allow(
    clippy::many_single_char_names,
    reason = "these names (a, b, c, H, V) are the spec's own variable names for this formula"
)]
fn luma_plane(top: &[u8; 16], left: &[u8; 16], corner: u8) -> [u8; 256] {
    let t = |i: i32| i32::from(top[i as usize]);
    let l = |i: i32| i32::from(left[i as usize]);
    let lt = i32::from(corner);

    let mut h = 0i32;
    let mut v = 0i32;
    for xp in 0..8i32 {
        let below = if xp <= 6 { t(6 - xp) } else { lt };
        h += (xp + 1) * (t(8 + xp) - below);
    }
    for yp in 0..8i32 {
        let below = if yp <= 6 { l(6 - yp) } else { lt };
        v += (yp + 1) * (l(8 + yp) - below);
    }
    let b = (5 * h + 32) >> 6;
    let c = (5 * v + 32) >> 6;
    let a = 16 * (l(15) + t(15));

    let mut out = [0u8; 256];
    for y in 0..16i32 {
        for x in 0..16i32 {
            let value = (a + b * (x - 7) + c * (y - 7) + 16) >> 5;
            out[(y * 16 + x) as usize] = value.clamp(0, 255) as u8;
        }
    }
    out
}

/// Predict a full 8x8 chroma block, one plane at a time (ITU-T H.264 § 8.3.4; 4:2:0 only).
/// Mode numbering differs from luma: `0` = DC, `1` = Horizontal, `2` = Vertical, `3` =
/// Plane. `top`/`left`/`corner` and unavailability follow [`predict_16x16`]. Output is
/// row-major, `out[y * 8 + x]`.
///
/// # Errors
///
/// Same conditions as [`predict_16x16`], with Vertical needing `top` and Horizontal
/// needing `left` (the luma mode numbers for those two are swapped for chroma).
pub(super) fn predict_chroma_8x8(
    mode: u8,
    top: Option<&[u8; 8]>,
    left: Option<&[u8; 8]>,
    corner: Option<u8>,
) -> Result<[u8; 64], H264Error> {
    match mode {
        0 => Ok(chroma_dc(top, left)),
        1 => {
            let left = left.ok_or(H264Error::UnavailableIntraNeighbor)?;
            let mut out = [0u8; 64];
            for (y, &sample) in left.iter().enumerate() {
                out[y * 8..y * 8 + 8].fill(sample);
            }
            Ok(out)
        }
        2 => {
            let top = top.ok_or(H264Error::UnavailableIntraNeighbor)?;
            let mut out = [0u8; 64];
            for y in 0..8 {
                out[y * 8..y * 8 + 8].copy_from_slice(top);
            }
            Ok(out)
        }
        3 => {
            let top = top.ok_or(H264Error::UnavailableIntraNeighbor)?;
            let left = left.ok_or(H264Error::UnavailableIntraNeighbor)?;
            let corner = corner.ok_or(H264Error::UnavailableIntraNeighbor)?;
            Ok(chroma_plane(top, left, corner))
        }
        _ => Err(H264Error::InvalidMbType),
    }
}

/// Chroma DC (mode `0`): each 4x4 quadrant of the 8x8 block averages a different subset of
/// neighbours (ITU-T H.264 § 8.3.4.1) — top-left and bottom-right combine top+left when
/// both are available, top-right prefers top (falls back to left), bottom-left prefers
/// left (falls back to top).
fn chroma_dc(top: Option<&[u8; 8]>, left: Option<&[u8; 8]>) -> [u8; 64] {
    let top_left = &top.map(|t| &t[0..4]);
    let top_right = &top.map(|t| &t[4..8]);
    let left_top = &left.map(|l| &l[0..4]);
    let left_bottom = &left.map(|l| &l[4..8]);

    let tl = combine_or_fallback(*top_left, *left_top);
    let tr = single_or_fallback(*top_right, *left_top);
    let bl = single_or_fallback(*left_bottom, *top_left);
    let br = combine_or_fallback(*top_right, *left_bottom);

    let mut out = [0u8; 64];
    for y in 0..8 {
        for x in 0..8 {
            out[y * 8 + x] = match (x < 4, y < 4) {
                (true, true) => tl,
                (false, true) => tr,
                (true, false) => bl,
                (false, false) => br,
            };
        }
    }
    out
}

/// Average `primary` and `secondary` together when both are available (8 samples -> `>>
/// 3`); otherwise average whichever one is available (4 samples -> `>> 2`); `128` if
/// neither is available.
fn combine_or_fallback(primary: Option<&[u8]>, secondary: Option<&[u8]>) -> u8 {
    match (primary, secondary) {
        (Some(p), Some(s)) => {
            let sum: u32 = p.iter().chain(s.iter()).map(|&v| u32::from(v)).sum();
            ((sum + 4) >> 3) as u8
        }
        (Some(p), None) => rounded_average(p, 2),
        (None, Some(s)) => rounded_average(s, 2),
        (None, None) => 128,
    }
}

/// Average `primary` (4 samples -> `>> 2`) if available, else fall back to `secondary`,
/// else `128`.
fn single_or_fallback(primary: Option<&[u8]>, secondary: Option<&[u8]>) -> u8 {
    primary
        .or(secondary)
        .map_or(128, |samples| rounded_average(samples, 2))
}

/// Chroma Plane (mode `3`, ITU-T H.264 § 8.3.4.4).
#[allow(
    clippy::many_single_char_names,
    clippy::trivially_copy_pass_by_ref,
    reason = "these names (a, b, c, H, V) are the spec's own variable names for this \
              formula; top/left are exactly at clippy's by-value threshold (8 bytes) and \
              every call site already holds them as references (from `Option<&[u8; 8]>`)"
)]
fn chroma_plane(top: &[u8; 8], left: &[u8; 8], corner: u8) -> [u8; 64] {
    let t = |i: i32| i32::from(top[i as usize]);
    let l = |i: i32| i32::from(left[i as usize]);
    let lt = i32::from(corner);

    let mut h = 0i32;
    let mut v = 0i32;
    for xp in 0..4i32 {
        let below = if xp <= 2 { t(2 - xp) } else { lt };
        h += (xp + 1) * (t(4 + xp) - below);
    }
    for yp in 0..4i32 {
        let below = if yp <= 2 { l(2 - yp) } else { lt };
        v += (yp + 1) * (l(4 + yp) - below);
    }
    let b = (17 * h + 16) >> 5;
    let c = (17 * v + 16) >> 5;
    let a = 16 * (l(7) + t(7));

    let mut out = [0u8; 64];
    for y in 0..8i32 {
        for x in 0..8i32 {
            let value = (a + b * (x - 3) + c * (y - 3) + 16) >> 5;
            out[(y * 8 + x) as usize] = value.clamp(0, 255) as u8;
        }
    }
    out
}

#[cfg(test)]
#[path = "intra_pred_tests.rs"]
mod tests;
