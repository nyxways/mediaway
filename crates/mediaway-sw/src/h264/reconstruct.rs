//! Per-macroblock pixel reconstruction: neighbour bookkeeping, plane read/write helpers,
//! and the `I_16x16`/`I_PCM` reconstruction paths that [`super::decode::decode_i_frame`]
//! drives in raster-scan order. `I_NxN` is recognized but rejected — see
//! `adr/0003-cavlc-i-slice-first-decode.md`.

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "pixel/plane indices below are bounded by macroblock geometry (4x4/8x8/16x16 \
              blocks, 0..=255 sample range); residual sums are clamped before the final cast"
)]

use super::bitreader::BitReader;
use super::cavlc;
use super::error::H264Error;
use super::intra_pred;
use super::macroblock::{self, MbType};
use super::pps::Pps;
use super::transform;

/// Z-order mapping of `luma4x4BlkIdx` (`0..16`) to `(x, y)` position within a macroblock,
/// in 4x4-block units (ITU-T H.264 § 6.4.3 / Figure 6-10's block numbering). Chroma's 2x2
/// block grid uses plain raster order instead (see [`xy_to_blk_chroma`]), which coincides
/// with this same Z-pattern at that size — 2x2 has no room for the pattern to diverge.
const BLK_XY: [(u8, u8); 16] = [
    (0, 0),
    (1, 0),
    (0, 1),
    (1, 1),
    (2, 0),
    (3, 0),
    (2, 1),
    (3, 1),
    (0, 2),
    (1, 2),
    (0, 3),
    (1, 3),
    (2, 2),
    (3, 2),
    (2, 3),
    (3, 3),
];

/// Inverse of [`BLK_XY`]: block index for a given `(x, y)` position.
fn xy_to_blk(x: u8, y: u8) -> usize {
    BLK_XY
        .iter()
        .position(|&(bx, by)| bx == x && by == y)
        .unwrap_or(0)
}

/// Reconstructed picture planes for one frame, sized to the full macroblock grid (before
/// [`super::decode`] crops down to the SPS's cropped `width`/`height`).
pub(super) struct Picture {
    pub(super) mb_width: usize,
    pub(super) mb_height: usize,
    pub(super) y: Vec<u8>,
    pub(super) u: Vec<u8>,
    pub(super) v: Vec<u8>,
    pub(super) y_stride: usize,
    pub(super) c_stride: usize,
}

impl Picture {
    pub(super) fn new(mb_width: usize, mb_height: usize) -> Self {
        let y_stride = mb_width * 16;
        let c_stride = mb_width * 8;
        Self {
            mb_width,
            mb_height,
            y: vec![0u8; y_stride * mb_height * 16],
            u: vec![0u8; c_stride * mb_height * 8],
            v: vec![0u8; c_stride * mb_height * 8],
            y_stride,
            c_stride,
        }
    }
}

/// Per-macroblock CAVLC neighbour context and `I_PCM` bookkeeping (ITU-T H.264 § 9.2.1),
/// threaded through the whole picture's macroblock loop.
pub(super) struct McbContext {
    mb_width: usize,
    luma_nz: Vec<[u8; 16]>,
    chroma_nz: [Vec<[u8; 4]>; 2],
    is_pcm: Vec<bool>,
}

impl McbContext {
    pub(super) fn new(num_mbs: usize, mb_width: usize) -> Self {
        Self {
            mb_width,
            luma_nz: vec![[0u8; 16]; num_mbs],
            chroma_nz: [vec![[0u8; 4]; num_mbs], vec![[0u8; 4]; num_mbs]],
            is_pcm: vec![false; num_mbs],
        }
    }
}

fn xy_to_blk_chroma(x: u8, y: u8) -> usize {
    usize::from(y) * 2 + usize::from(x)
}

fn luma_nz_at(ctx: &McbContext, mb: usize, blk: usize) -> u32 {
    if ctx.is_pcm[mb] {
        16
    } else {
        u32::from(ctx.luma_nz[mb][blk])
    }
}

fn chroma_nz_at(ctx: &McbContext, plane: usize, mb: usize, blk: usize) -> u32 {
    if ctx.is_pcm[mb] {
        16
    } else {
        u32::from(ctx.chroma_nz[plane][mb][blk])
    }
}

/// `nC` combination rule (ITU-T H.264 § 9.2.1): average when both neighbours are
/// available, use whichever one is, or `0` when neither is.
fn combine_nc(a: Option<u32>, b: Option<u32>) -> i32 {
    match (a, b) {
        (Some(a), Some(b)) => i32::try_from((a + b).div_ceil(2)).unwrap_or(0),
        (Some(only), None) | (None, Some(only)) => i32::try_from(only).unwrap_or(0),
        (None, None) => 0,
    }
}

/// CAVLC neighbour context (`nC`) for luma block `blk` (`0..16`, Z-order) of `mb_addr`,
/// including the DC block (which uses the same derivation as `blk == 0`).
fn luma_nc(ctx: &McbContext, mb_addr: usize, blk: usize) -> i32 {
    let (x, y) = BLK_XY[blk];
    let left = if x > 0 {
        Some(luma_nz_at(ctx, mb_addr, xy_to_blk(x - 1, y)))
    } else if !mb_addr.is_multiple_of(ctx.mb_width) {
        Some(luma_nz_at(ctx, mb_addr - 1, xy_to_blk(3, y)))
    } else {
        None
    };
    let top = if y > 0 {
        Some(luma_nz_at(ctx, mb_addr, xy_to_blk(x, y - 1)))
    } else if mb_addr >= ctx.mb_width {
        Some(luma_nz_at(ctx, mb_addr - ctx.mb_width, xy_to_blk(x, 3)))
    } else {
        None
    };
    combine_nc(left, top)
}

/// CAVLC neighbour context (`nC`) for chroma AC block `blk` (`0..4`, raster) of `mb_addr`
/// on `plane` (`0` = Cb, `1` = Cr). Chroma DC always uses the fixed `nC == -1` context
/// instead — see [`cavlc::decode_chroma_dc_residual`].
fn chroma_nc(ctx: &McbContext, plane: usize, mb_addr: usize, blk: usize) -> i32 {
    let (x, y) = BLK_XY[blk];
    let left = if x > 0 {
        Some(chroma_nz_at(
            ctx,
            plane,
            mb_addr,
            xy_to_blk_chroma(x - 1, y),
        ))
    } else if !mb_addr.is_multiple_of(ctx.mb_width) {
        Some(chroma_nz_at(
            ctx,
            plane,
            mb_addr - 1,
            xy_to_blk_chroma(1, y),
        ))
    } else {
        None
    };
    let top = if y > 0 {
        Some(chroma_nz_at(
            ctx,
            plane,
            mb_addr,
            xy_to_blk_chroma(x, y - 1),
        ))
    } else if mb_addr >= ctx.mb_width {
        Some(chroma_nz_at(
            ctx,
            plane,
            mb_addr - ctx.mb_width,
            xy_to_blk_chroma(x, 1),
        ))
    } else {
        None
    };
    combine_nc(left, top)
}

/// Read the `N` reconstructed samples immediately above `(x0, y0)` (row `-1`).
fn read_top<const N: usize>(plane: &[u8], stride: usize, x0: usize, y0: usize) -> [u8; N] {
    let mut out = [0u8; N];
    let start = (y0 - 1) * stride + x0;
    out.copy_from_slice(&plane[start..start + N]);
    out
}

/// Read the `N` reconstructed samples immediately left of `(x0, y0)` (column `-1`).
fn read_left<const N: usize>(plane: &[u8], stride: usize, x0: usize, y0: usize) -> [u8; N] {
    let mut out = [0u8; N];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = plane[(y0 + i) * stride + x0 - 1];
    }
    out
}

/// Read the reconstructed corner sample `(-1, -1)` relative to `(x0, y0)`.
fn read_corner(plane: &[u8], stride: usize, x0: usize, y0: usize) -> u8 {
    plane[(y0 - 1) * stride + x0 - 1]
}

/// Write an `N x N` prediction block (row-major) into `plane` at `(x0, y0)`.
fn write_block<const N: usize>(
    plane: &mut [u8],
    stride: usize,
    x0: usize,
    y0: usize,
    block: &[u8],
) {
    for row in 0..N {
        let start = (y0 + row) * stride + x0;
        plane[start..start + N].copy_from_slice(&block[row * N..row * N + N]);
    }
}

/// Add a decoded 4x4 residual to the (already-written) prediction already in `plane` at
/// `(x0, y0)`, clipping to `0..=255` (ITU-T H.264 § 8.5.13's implicit `Clip1`).
fn add_residual_4x4(plane: &mut [u8], stride: usize, x0: usize, y0: usize, residual: &[i32; 16]) {
    for ry in 0..4 {
        for rx in 0..4 {
            let idx = (y0 + ry) * stride + x0 + rx;
            let value = i32::from(plane[idx]) + residual[ry * 4 + rx];
            plane[idx] = value.clamp(0, 255) as u8;
        }
    }
}

/// Byte-align `reader` (ITU-T H.264 `while( !byte_aligned() )`) for `I_PCM`'s raw samples.
fn align_to_byte(reader: &mut BitReader<'_>) -> Result<(), H264Error> {
    while !reader.bits_read().is_multiple_of(8) {
        reader.read_bit()?;
    }
    Ok(())
}

/// Decode one macroblock at raster address `mb_addr`, updating `picture` and `ctx` in
/// place and threading the running QP (`qp_prev`, ITU-T H.264 § 7.4.5) forward.
///
/// # Errors
///
/// [`H264Error::UnsupportedMbType`] for `I_NxN`; other [`H264Error`] variants propagate
/// from macroblock-header, CAVLC, or dequant/transform parsing.
pub(super) fn decode_macroblock(
    reader: &mut BitReader<'_>,
    pps: &Pps,
    mb_addr: usize,
    qp_prev: &mut i32,
    picture: &mut Picture,
    ctx: &mut McbContext,
) -> Result<(), H264Error> {
    let mb_col = mb_addr % ctx.mb_width;
    let mb_row = mb_addr / ctx.mb_width;
    let left_avail = mb_col > 0;
    let top_avail = mb_row > 0;

    let mb_type = MbType::from_raw(reader.read_ue()?)?;
    match mb_type {
        MbType::INxN => Err(H264Error::UnsupportedMbType),
        MbType::IPcm => {
            decode_i_pcm(reader, picture, mb_col, mb_row)?;
            ctx.luma_nz[mb_addr] = [0; 16];
            ctx.chroma_nz[0][mb_addr] = [0; 4];
            ctx.chroma_nz[1][mb_addr] = [0; 4];
            ctx.is_pcm[mb_addr] = true;
            *qp_prev = 0;
            Ok(())
        }
        MbType::I16x16 {
            pred_mode,
            cbp_luma,
            cbp_chroma,
        } => {
            let intra_chroma_pred_mode = macroblock::read_intra_chroma_pred_mode(reader)?;
            let mb_qp_delta = reader.read_se()?;
            // ITU-T H.264 § 7.4.5: QPy = (QPy,prev + mb_qp_delta + 52) % 52, kept in
            // 0..=52's non-negative range via `rem_euclid` (8-bit decode: QpBdOffsetY = 0).
            let qp = qp_prev
                .checked_add(mb_qp_delta)
                .ok_or(H264Error::FieldOverflow)?
                .rem_euclid(52);
            *qp_prev = qp;
            let qpc = transform::qpc_from_qp(qp, pps.chroma_qp_index_offset);

            reconstruct_luma_16x16(
                reader, picture, ctx, mb_addr, mb_col, mb_row, left_avail, top_avail, pred_mode,
                cbp_luma, qp,
            )?;
            reconstruct_chroma_8x8(
                reader,
                picture,
                ctx,
                mb_addr,
                mb_col,
                mb_row,
                left_avail,
                top_avail,
                intra_chroma_pred_mode,
                cbp_chroma,
                qpc,
            )?;
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn reconstruct_luma_16x16(
    reader: &mut BitReader<'_>,
    picture: &mut Picture,
    ctx: &mut McbContext,
    mb_addr: usize,
    mb_col: usize,
    mb_row: usize,
    left_avail: bool,
    top_avail: bool,
    pred_mode: u8,
    cbp_luma: u8,
    qp: i32,
) -> Result<(), H264Error> {
    let stride = picture.y_stride;
    let x0 = mb_col * 16;
    let y0 = mb_row * 16;

    let top = top_avail.then(|| read_top::<16>(&picture.y, stride, x0, y0));
    let left = left_avail.then(|| read_left::<16>(&picture.y, stride, x0, y0));
    let corner = (top_avail && left_avail).then(|| read_corner(&picture.y, stride, x0, y0));

    let pred = intra_pred::predict_16x16(pred_mode, top.as_ref(), left.as_ref(), corner)?;
    write_block::<16>(&mut picture.y, stride, x0, y0, &pred);

    let nc_dc = luma_nc(ctx, mb_addr, 0);
    let dc_residual = cavlc::decode_4x4_residual(reader, nc_dc, false)?;
    let dc_hadamard = transform::inverse_hadamard_4x4(&dc_residual.raster);
    let dc_dequant = transform::dequant_luma_dc(&dc_hadamard, qp)?;

    for (blk, &(bx, by)) in BLK_XY.iter().enumerate() {
        let (coeffs, nz) = if cbp_luma == 15 {
            let nc = luma_nc(ctx, mb_addr, blk);
            let ac = cavlc::decode_4x4_residual(reader, nc, true)?;
            (ac.raster, ac.total_coeff)
        } else {
            ([0i32; 16], 0)
        };
        ctx.luma_nz[mb_addr][blk] = nz;

        let mut dequant = transform::dequant_normal(&coeffs, qp)?;
        dequant[0] = dc_dequant[usize::from(by) * 4 + usize::from(bx)];
        let residual = transform::inverse_transform_4x4(&dequant);

        let bx0 = x0 + usize::from(bx) * 4;
        let by0 = y0 + usize::from(by) * 4;
        add_residual_4x4(&mut picture.y, stride, bx0, by0, &residual);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn reconstruct_chroma_8x8(
    reader: &mut BitReader<'_>,
    picture: &mut Picture,
    ctx: &mut McbContext,
    mb_addr: usize,
    mb_col: usize,
    mb_row: usize,
    left_avail: bool,
    top_avail: bool,
    pred_mode: u8,
    cbp_chroma: u8,
    qpc: i32,
) -> Result<(), H264Error> {
    let stride = picture.c_stride;
    let x0 = mb_col * 8;
    let y0 = mb_row * 8;

    for plane_idx in 0..2usize {
        let plane: &mut [u8] = if plane_idx == 0 {
            picture.u.as_mut_slice()
        } else {
            picture.v.as_mut_slice()
        };

        let top = top_avail.then(|| read_top::<8>(plane, stride, x0, y0));
        let left = left_avail.then(|| read_left::<8>(plane, stride, x0, y0));
        let corner = (top_avail && left_avail).then(|| read_corner(plane, stride, x0, y0));
        let pred = intra_pred::predict_chroma_8x8(pred_mode, top.as_ref(), left.as_ref(), corner)?;
        write_block::<8>(plane, stride, x0, y0, &pred);

        if cbp_chroma == 0 {
            ctx.chroma_nz[plane_idx][mb_addr] = [0; 4];
            continue;
        }

        let dc = cavlc::decode_chroma_dc_residual(reader)?;
        let dc_hadamard = transform::inverse_hadamard_2x2(&dc.c);
        let dc_dequant = transform::dequant_chroma_dc(&dc_hadamard, qpc)?;

        for (blk, &dc_value) in dc_dequant.iter().enumerate() {
            let bx = blk % 2;
            let by = blk / 2;
            let (coeffs, nz) = if cbp_chroma == 2 {
                let nc = chroma_nc(ctx, plane_idx, mb_addr, blk);
                let ac = cavlc::decode_4x4_residual(reader, nc, true)?;
                (ac.raster, ac.total_coeff)
            } else {
                ([0i32; 16], 0)
            };
            ctx.chroma_nz[plane_idx][mb_addr][blk] = nz;

            let mut dequant = transform::dequant_normal(&coeffs, qpc)?;
            dequant[0] = dc_value;
            let residual = transform::inverse_transform_4x4(&dequant);

            let bx0 = x0 + bx * 4;
            let by0 = y0 + by * 4;
            add_residual_4x4(plane, stride, bx0, by0, &residual);
        }
    }
    Ok(())
}

fn decode_i_pcm(
    reader: &mut BitReader<'_>,
    picture: &mut Picture,
    mb_col: usize,
    mb_row: usize,
) -> Result<(), H264Error> {
    align_to_byte(reader)?;

    let y_stride = picture.y_stride;
    let x0 = mb_col * 16;
    let y0 = mb_row * 16;
    for row in 0..16 {
        for col in 0..16 {
            let sample = reader.read_bits(8)?;
            picture.y[(y0 + row) * y_stride + x0 + col] = sample as u8;
        }
    }

    let c_stride = picture.c_stride;
    let cx0 = mb_col * 8;
    let cy0 = mb_row * 8;
    for plane_idx in 0..2usize {
        let plane: &mut [u8] = if plane_idx == 0 {
            picture.u.as_mut_slice()
        } else {
            picture.v.as_mut_slice()
        };
        for row in 0..8 {
            for col in 0..8 {
                let sample = reader.read_bits(8)?;
                plane[(cy0 + row) * c_stride + cx0 + col] = sample as u8;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "reconstruct_tests.rs"]
mod tests;
