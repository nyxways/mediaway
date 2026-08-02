#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn parse_bits_converts_bit_strings_to_value_and_length() {
    assert_eq!(parse_bits("1"), (1, 1));
    assert_eq!(parse_bits("01"), (1, 2));
    assert_eq!(parse_bits("000101"), (5, 6));
    assert_eq!(parse_bits(""), (0, 0));
}

#[test]
fn decode_4x4_residual_handles_empty_block() {
    // nC = 0 (VLC0), coeff_token(TotalCoeff=0, TrailingOnes=0) = "1".
    let data = [0b1000_0000u8];
    let mut reader = BitReader::new(&data);
    let residual = decode_4x4_residual(&mut reader, 0, false).unwrap();
    assert_eq!(residual.total_coeff, 0);
    assert_eq!(residual.raster, [0; 16]);
}

#[test]
fn decode_4x4_residual_places_single_dc_coefficient() {
    // nC = 0, coeff_token(1,1) = "01" (VLC0), sign "0" (+1), total_zeros(0,tc=1) = "1".
    // Bits: 01 0 1 -> "0101" -> byte 0x50.
    let data = [0b0101_0000u8];
    let mut reader = BitReader::new(&data);
    let residual = decode_4x4_residual(&mut reader, 0, false).unwrap();
    assert_eq!(residual.total_coeff, 1);
    let mut expected = [0i32; 16];
    expected[0] = 1;
    assert_eq!(residual.raster, expected);
}

#[test]
fn decode_4x4_residual_places_two_coefficients_with_a_zero_gap() {
    // nC = 0, coeff_token(TotalCoeff=2, TrailingOnes=2) = "001" (VLC0).
    // signs "01" -> level0=+1 (highest freq, decoded first), level1=-1.
    // total_zeros(TotalZeros=1, TotalCoeff=2) = "110". run_before(zerosLeft=1) run=1 -> "0".
    // Bits: 001 01 110 0 -> "0010111 0" -> byte 0x2E, 0x00.
    let data = [0x2E, 0x00];
    let mut reader = BitReader::new(&data);
    let residual = decode_4x4_residual(&mut reader, 0, false).unwrap();
    assert_eq!(residual.total_coeff, 2);
    let mut expected = [0i32; 16];
    expected[0] = -1; // zig-zag scan index 0 (DC) -> raster 0
    expected[4] = 1; // zig-zag scan index 2 -> raster 4 (one zero coefficient skipped)
    assert_eq!(residual.raster, expected);
}

#[test]
fn decode_4x4_residual_ac_only_skips_the_dc_scan_position() {
    // nC = 0, coeff_token(1,1) = "01", sign "0" (+1), total_zeros(TotalZeros=2, TotalCoeff=1)
    // = "010" (AC block: local scan position 2 == full zig-zag index 3 since DC is excluded).
    // Bits: 01 0 010 -> "010010" -> byte 0x48.
    let data = [0x48];
    let mut reader = BitReader::new(&data);
    let residual = decode_4x4_residual(&mut reader, 0, true).unwrap();
    assert_eq!(residual.total_coeff, 1);
    let mut expected = [0i32; 16];
    expected[8] = 1; // ZIGZAG_4X4[3] == 8
    assert_eq!(residual.raster, expected);
}

#[test]
fn decode_4x4_residual_uses_fixed_length_code_for_nc_at_least_8() {
    // nC = 9 -> FLC. code = 0 -> TotalCoeff=1, TrailingOnes=0 (per the FLC formula).
    let data = [0b0000_0000u8];
    let mut reader = BitReader::new(&data);
    let err = decode_4x4_residual(&mut reader, 9, false);
    // TrailingOnes=0 with TotalCoeff=1 needs a level_prefix read next; an all-zero buffer
    // eventually hits EOF while reading that unary code - still proves the FLC path chose
    // TotalCoeff=1 (a genuinely empty block would return Ok immediately, as in the test
    // above with a "1" codeword instead).
    assert_eq!(err, Err(H264Error::UnexpectedEof));
}

#[test]
fn decode_4x4_residual_rejects_total_coeff_above_max_num_coeff() {
    // nC = 9 -> FLC, code 0b111111 = 63 -> TotalCoeff=16, which exceeds an AC-only block's
    // maxNumCoeff = 15.
    let data = [0b1111_1100u8];
    let mut reader = BitReader::new(&data);
    assert_eq!(
        decode_4x4_residual(&mut reader, 9, true),
        Err(H264Error::InvalidCavlcCode)
    );
}

#[test]
fn decode_4x4_residual_errors_on_empty_input() {
    let mut reader = BitReader::new(&[]);
    assert_eq!(
        decode_4x4_residual(&mut reader, 0, false),
        Err(H264Error::UnexpectedEof)
    );
}

#[test]
fn decode_vlc_rejects_unmatched_codeword_within_bit_budget() {
    // All-zero bits never match any VLC0 coeff_token entry; decode_vlc must give up at
    // MAX_VLC_BITS rather than looping past the (deliberately longer) buffer.
    let data = [0u8; 4];
    let mut reader = BitReader::new(&data);
    assert_eq!(
        decode_4x4_residual(&mut reader, 0, false),
        Err(H264Error::InvalidCavlcCode)
    );
}

#[test]
fn decode_chroma_dc_residual_places_single_coefficient() {
    // coeff_token(1,1) = "1" (chroma DC table), sign "0" (+1),
    // total_zeros(TotalZeros=1, TotalCoeff=1) = "01" (chroma DC total_zeros table).
    // Bits: 1 0 01 -> "1001" -> byte 0x90.
    let data = [0x90u8];
    let mut reader = BitReader::new(&data);
    let residual = decode_chroma_dc_residual(&mut reader).unwrap();
    assert_eq!(residual.total_coeff, 1);
    assert_eq!(residual.c, [0, 1, 0, 0]);
}

#[test]
fn decode_chroma_dc_residual_handles_empty_block() {
    // coeff_token(TotalCoeff=0, TrailingOnes=0) = "01" (chroma DC table).
    let data = [0b0100_0000u8];
    let mut reader = BitReader::new(&data);
    let residual = decode_chroma_dc_residual(&mut reader).unwrap();
    assert_eq!(residual.total_coeff, 0);
    assert_eq!(residual.c, [0; 4]);
}
