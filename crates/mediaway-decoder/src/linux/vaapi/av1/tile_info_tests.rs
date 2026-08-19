#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn uniform_trivial_single_superblock_needs_only_the_flag_bit() {
    let mut r = BitReader::new(&[0b1000_0000]); // uniform_tile_spacing_flag = 1, nothing else
    let info = parse(&mut r, false, 16, 16).unwrap();
    assert_eq!(
        info,
        TileInfo {
            sb_cols: 1,
            sb_rows: 1,
            context_update_tile_id: 0,
            uniform_tile_spacing_flag: true,
        }
    );
    assert_eq!(r.bits_read(), 1);
}

#[test]
fn uniform_two_superblock_columns_single_tile_stops_at_zero_increment() {
    // uniform_tile_spacing_flag = 1, increment_tile_cols_log2 = 0 (stop at the minimum).
    let mut r = BitReader::new(&[0b1000_0000]);
    let info = parse(&mut r, false, 20, 16).unwrap();
    assert_eq!(
        info,
        TileInfo {
            sb_cols: 2,
            sb_rows: 1,
            context_update_tile_id: 0,
            uniform_tile_spacing_flag: true,
        }
    );
    assert_eq!(r.bits_read(), 2);
}

#[test]
fn uniform_rejects_multi_tile_column_increment() {
    // uniform_tile_spacing_flag = 1, increment_tile_cols_log2 = 1 -> 2 tile columns.
    let mut r = BitReader::new(&[0b1100_0000]);
    assert_eq!(parse(&mut r, false, 20, 16), Err(DecodeError::Unsupported));
}

#[test]
fn non_uniform_single_tile_covering_full_width() {
    // uniform_tile_spacing_flag = 0, then ns(2) = 1 (covers both superblock columns in one
    // tile), then the row pass reads nothing (ns(1) is always 0 without consuming bits).
    let mut r = BitReader::new(&[0b0100_0000]);
    let info = parse(&mut r, false, 20, 16).unwrap();
    assert_eq!(
        info,
        TileInfo {
            sb_cols: 2,
            sb_rows: 1,
            context_update_tile_id: 0,
            uniform_tile_spacing_flag: false,
        }
    );
    assert_eq!(r.bits_read(), 2);
}

#[test]
fn truncated_input_is_invalid() {
    let mut r = BitReader::new(&[]);
    assert_eq!(parse(&mut r, false, 16, 16), Err(DecodeError::InvalidInput));
}
