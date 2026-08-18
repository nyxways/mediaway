//! `tile_info()` (AV1 spec §5.9.15), single-tile-only acceptance.
//!
//! Inverse of `windows::d3d12_video_encode::bitstream_av1::write_tile_info`'s `tile_log2`/
//! min/max tile-count math (`bitstream_av1.rs:71-128`) — same spec section, same arithmetic,
//! read direction is new but the formula is already validated by that writer's
//! D3D12-driver-accepted output. See
//! [ADR-0003](../../../../adr/linux/0003-vaapi-av1-key-frame-decode.md) § Scope: this crate
//! accepts only streams whose `tile_info()` resolves to exactly one tile column and one tile
//! row (`TileCols == TileRows == 1`) — both the common uniform-spacing case (matching this
//! workspace's own AV1 encoders) and the non-uniform case are parsed correctly (consuming the
//! right number of bits either way), but only a single-tile result is accepted.

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

use super::bits::ns;

const MAX_TILE_WIDTH: u32 = 4096;
const MAX_TILE_AREA: u32 = 4096 * 2304;
const MAX_TILE_COLS: u32 = 64;
const MAX_TILE_ROWS: u32 = 64;

/// Fields this crate's VA-API picture-parameter buffer needs from a single-tile `tile_info()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TileInfo {
    /// Superblock columns (`sbCols`) — `width_in_sbs_minus_1[0]` is `sb_cols - 1` for this
    /// crate's always-exactly-one-tile scope.
    pub(super) sb_cols: u32,
    /// Superblock rows (`sbRows`) — `height_in_sbs_minus_1[0]` is `sb_rows - 1`.
    pub(super) sb_rows: u32,
    /// `context_update_tile_id` — always `0` for a single tile (only read, and nonzero, when
    /// `TileColsLog2 + TileRowsLog2 > 0`).
    pub(super) context_update_tile_id: u16,
    /// `uniform_tile_spacing_flag` — forwarded to `VADecPictureParameterBufferAV1`'s
    /// `pic_info_fields.uniform_tile_spacing_flag`.
    pub(super) uniform_tile_spacing_flag: bool,
}

/// `TileLog2(blkSize, target)` (AV1 spec §5.9.15): the smallest `k` such that
/// `blkSize << k >= target`.
const fn tile_log2(blk_size: u32, target: u32) -> u32 {
    let mut k = 0;
    while (blk_size << k) < target {
        k += 1;
    }
    k
}

/// Parse `tile_info()`, requiring the result to resolve to exactly one tile.
///
/// `mi_cols`/`mi_rows` are the frame's mode-info column/row counts (`2 * ((width + 7) >> 3)` /
/// `2 * ((height + 7) >> 3)`, AV1 spec §5.9.5 `compute_image_size`).
///
/// # Errors
///
/// Returns [`DecodeError::InvalidInput`] on truncated data, or [`DecodeError::Unsupported`]
/// when the stream signals more than one tile column or row.
pub(super) fn parse(
    r: &mut BitReader<'_>,
    use_128x128_superblock: bool,
    mi_cols: u32,
    mi_rows: u32,
) -> Result<TileInfo, DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let sb_shift = if use_128x128_superblock { 5 } else { 4 };
    let sb_size = sb_shift + 2;
    let sb_cols = if use_128x128_superblock {
        (mi_cols + 31) >> 5
    } else {
        (mi_cols + 15) >> 4
    };
    let sb_rows = if use_128x128_superblock {
        (mi_rows + 31) >> 5
    } else {
        (mi_rows + 15) >> 4
    };
    let max_tile_width_sb = MAX_TILE_WIDTH >> sb_size;
    let max_tile_area_sb = MAX_TILE_AREA >> (2 * sb_size);
    let min_log2_tile_cols = tile_log2(max_tile_width_sb, sb_cols);
    let max_log2_tile_cols = tile_log2(1, sb_cols.min(MAX_TILE_COLS));
    let max_log2_tile_rows = tile_log2(1, sb_rows.min(MAX_TILE_ROWS));
    let min_log2_tiles =
        min_log2_tile_cols.max(tile_log2(max_tile_area_sb, sb_rows.saturating_mul(sb_cols)));

    let uniform_tile_spacing_flag = r.read_bit().map_err(map_err)? != 0;

    let (tile_cols, tile_cols_log2) = if uniform_tile_spacing_flag {
        let mut tile_cols_log2 = min_log2_tile_cols;
        while tile_cols_log2 < max_log2_tile_cols {
            if r.read_bit().map_err(map_err)? == 0 {
                break;
            }
            tile_cols_log2 += 1;
        }
        // Uniform spacing always produces exactly `2^TileColsLog2` tile columns (the ceiling
        // division that derives `tileWidthSb` from `TileColsLog2` guarantees this) — no need to
        // walk `MiColStarts` to count them, unlike the non-uniform branch below.
        (1u32 << tile_cols_log2, tile_cols_log2)
    } else {
        let mut start_sb = 0u32;
        let mut tile_cols = 0u32;
        while start_sb < sb_cols {
            let max_width = (sb_cols - start_sb).min(max_tile_width_sb);
            let size_sb = ns(r, max_width)?
                .checked_add(1)
                .ok_or(DecodeError::InvalidInput)?;
            start_sb = start_sb
                .checked_add(size_sb)
                .ok_or(DecodeError::InvalidInput)?;
            tile_cols = tile_cols.checked_add(1).ok_or(DecodeError::InvalidInput)?;
        }
        (tile_cols, tile_log2(1, tile_cols))
    };
    if tile_cols != 1 {
        return Err(DecodeError::Unsupported);
    }

    let (tile_rows, tile_rows_log2) = if uniform_tile_spacing_flag {
        let min_log2_tile_rows = min_log2_tiles.saturating_sub(tile_cols_log2);
        let mut tile_rows_log2 = min_log2_tile_rows;
        while tile_rows_log2 < max_log2_tile_rows {
            if r.read_bit().map_err(map_err)? == 0 {
                break;
            }
            tile_rows_log2 += 1;
        }
        (1u32 << tile_rows_log2, tile_rows_log2)
    } else {
        // `tile_cols == 1` is already confirmed above, so the single column tile spans the
        // full `sb_cols` width — that is this non-uniform row budget's `widestTileSb`.
        let widest_tile_sb = sb_cols.max(1);
        let row_budget = if min_log2_tiles > 0 {
            (sb_rows.saturating_mul(sb_cols)) >> (min_log2_tiles + 1)
        } else {
            sb_rows.saturating_mul(sb_cols)
        };
        let max_tile_height_sb = (row_budget / widest_tile_sb).max(1);

        let mut start_sb = 0u32;
        let mut tile_rows = 0u32;
        while start_sb < sb_rows {
            let max_height = (sb_rows - start_sb).min(max_tile_height_sb);
            let size_sb = ns(r, max_height)?
                .checked_add(1)
                .ok_or(DecodeError::InvalidInput)?;
            start_sb = start_sb
                .checked_add(size_sb)
                .ok_or(DecodeError::InvalidInput)?;
            tile_rows = tile_rows.checked_add(1).ok_or(DecodeError::InvalidInput)?;
        }
        (tile_rows, tile_log2(1, tile_rows))
    };
    if tile_rows != 1 {
        return Err(DecodeError::Unsupported);
    }

    let context_update_tile_id = if tile_cols_log2 > 0 || tile_rows_log2 > 0 {
        let bits = tile_rows_log2
            .checked_add(tile_cols_log2)
            .ok_or(DecodeError::InvalidInput)?;
        let id = r.read_bits(bits).map_err(map_err)?;
        let _tile_size_bytes_minus_1 = r.read_bits(2).map_err(map_err)?;
        u16::try_from(id).map_err(|_| DecodeError::InvalidInput)?
    } else {
        0
    };

    Ok(TileInfo {
        sb_cols,
        sb_rows,
        context_update_tile_id,
        uniform_tile_spacing_flag,
    })
}

#[cfg(test)]
#[path = "tile_info_tests.rs"]
mod tests;
