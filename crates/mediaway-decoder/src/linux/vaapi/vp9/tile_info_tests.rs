#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap / expect"
)]

use super::*;

#[test]
fn calc_min_log2_tile_cols_is_zero_for_small_frames() {
    assert_eq!(calc_min_log2_tile_cols(1), 0);
    assert_eq!(calc_min_log2_tile_cols(64), 0);
}

#[test]
fn calc_max_log2_tile_cols_is_zero_for_small_frames() {
    // sb64_cols=1: (1 >> 1) = 0, not >= MIN_TILE_WIDTH_B64(4) -> loop never runs -> max_log2=0.
    assert_eq!(calc_max_log2_tile_cols(1), 0);
}

#[test]
fn parse_accepts_single_tile_small_frame_reading_no_increment_bits() {
    // mi_cols small -> sb64_cols=1 -> min==max==0 -> the tile_cols_log2 while loop never reads a
    // bit at all (matches this ADR's own addendum note: "increment_tile_cols_log2 is never read
    // at all for typical frame sizes"). Then tile_rows_log2 f(1) = 0.
    let data = [0b0_0000000]; // single bit: tile_rows_log2 = 0
    let mut r = BitReader::new(&data);
    // mi_cols = 8 -> sb64_cols = 1.
    assert!(parse(&mut r, 8).is_ok());
}

#[test]
fn parse_rejects_tile_rows_log2_one_without_increment() {
    // tile_rows_log2 = 1, then increment_tile_rows_log2 = 0 -> stays 1 -> rejected.
    let data = [0b1_0000000];
    let mut r = BitReader::new(&data);
    assert_eq!(parse(&mut r, 8), Err(DecodeError::Unsupported));
}

#[test]
fn parse_rejects_multi_tile_column_signal_on_large_frame() {
    // A very wide frame forces min_log2_tile_cols > 0, so tile_cols_log2 can never be 0 —
    // rejected before any bit is read for that field.
    // mi_cols for a huge width: sb64_cols must exceed MAX_TILE_WIDTH_B64 (64) to force
    // min_log2_tile_cols > 0. sb64_cols = mi_cols.div_ceil(8); mi_cols = (width+7)>>3.
    // Use mi_cols = 8 * 65 = 520 -> sb64_cols = 65 -> min_log2_tile_cols = 1 (64<<0=64 < 65).
    let data = [0u8; 4];
    let mut r = BitReader::new(&data);
    assert_eq!(parse(&mut r, 520), Err(DecodeError::Unsupported));
}

#[test]
fn parse_consumes_only_the_tile_rows_bit_for_a_tiny_frame() {
    let data = [0b0_1000000]; // tile_rows_log2 = 0, then a trailing marker bit
    let mut r = BitReader::new(&data);
    parse(&mut r, 8).unwrap();
    assert_eq!(r.read_bit().unwrap(), 1);
}
