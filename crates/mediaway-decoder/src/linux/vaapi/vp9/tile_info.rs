//! `tile_info()` (VP9 spec §6.2.13), copied verbatim from the real primary spec text this
//! session (see `adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md` Addendum, § "closes
//! open question #3's second half"): a `while`-loop of single-bit "increment" flags bounded by
//! `calc_min_log2_tile_cols()`/`calc_max_log2_tile_cols()` — **not** AV1-style explicit
//! column/row counts, and not the same arithmetic as AV1's own `tile_log2` formula (VP9 uses a
//! fixed 64×64 superblock, unlike AV1's flexible 64/128 size).
//!
//! `calc_min_log2_tile_cols()`/`calc_max_log2_tile_cols()` themselves (VP9 spec §6.2.14, not
//! quoted verbatim in this crate's own ADR addendum) use the well-known
//! `MIN_TILE_WIDTH_B64 = 4`/`MAX_TILE_WIDTH_B64 = 64` constants (matching libvpx's own
//! `vp9_common.h`) — general VP9 domain knowledge, not itself primary-source-quoted this
//! session, flagged here per this ADR's own honesty convention. This crate accepts only
//! single-tile streams (`tile_cols_log2 == tile_rows_log2 == 0`), matching this workspace's own
//! VP9 encoder sibling's single-tile-only output.

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

const MAX_TILE_WIDTH_B64: u32 = 64;
const MIN_TILE_WIDTH_B64: u32 = 4;

const fn calc_min_log2_tile_cols(sb64_cols: u32) -> u32 {
    let mut min_log2 = 0u32;
    while (MAX_TILE_WIDTH_B64 << min_log2) < sb64_cols {
        min_log2 += 1;
    }
    min_log2
}

const fn calc_max_log2_tile_cols(sb64_cols: u32) -> u32 {
    let mut max_log2 = 1u32;
    while (sb64_cols >> max_log2) >= MIN_TILE_WIDTH_B64 {
        max_log2 += 1;
    }
    max_log2 - 1
}

/// Parse `tile_info()`, requiring the result to resolve to exactly one tile column and one tile
/// row. `mi_cols` is the frame's mode-info column count (`(FrameWidth + 7) >> 3` — VP9's mode
/// info unit is 8×8, unlike AV1's 4×4 — see `header::parse`).
///
/// # Errors
///
/// Returns [`DecodeError::InvalidInput`] on truncated data, or [`DecodeError::Unsupported`]
/// when the stream signals more than one tile column or row.
pub(super) fn parse(r: &mut BitReader<'_>, mi_cols: u32) -> Result<(), DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let sb64_cols = mi_cols.div_ceil(8);

    let min_log2_tile_cols = calc_min_log2_tile_cols(sb64_cols);
    let max_log2_tile_cols = calc_max_log2_tile_cols(sb64_cols);
    let mut tile_cols_log2 = min_log2_tile_cols;
    while tile_cols_log2 < max_log2_tile_cols {
        if r.read_bit().map_err(map_err)? == 0 {
            break;
        }
        tile_cols_log2 += 1;
    }
    if tile_cols_log2 != 0 {
        return Err(DecodeError::Unsupported);
    }

    let mut tile_rows_log2 = r.read_bit().map_err(map_err)?;
    if tile_rows_log2 == 1 {
        let increment = r.read_bit().map_err(map_err)?;
        tile_rows_log2 += increment;
    }
    if tile_rows_log2 != 0 {
        return Err(DecodeError::Unsupported);
    }

    Ok(())
}

#[cfg(test)]
#[path = "tile_info_tests.rs"]
mod tests;
