#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn qpc_from_qp_returns_qpi_directly_below_30() {
    assert_eq!(qpc_from_qp(20, 0), 20);
    assert_eq!(qpc_from_qp(29, 0), 29);
}

#[test]
fn qpc_from_qp_maps_table_8_15_range() {
    assert_eq!(qpc_from_qp(30, 0), 29);
    assert_eq!(qpc_from_qp(51, 0), 39);
    assert_eq!(qpc_from_qp(46, 0), 38);
}

#[test]
fn qpc_from_qp_clamps_offset_result_into_0_to_51() {
    // qp + offset = 56 -> clamped to 51 -> QPc = 39.
    assert_eq!(qpc_from_qp(51, 5), 39);
    // qp + offset = -5 -> clamped to 0 -> QPi < 30 -> QPc = 0.
    assert_eq!(qpc_from_qp(0, -5), 0);
}

#[test]
fn dequant_normal_matches_hand_computed_value_for_qp_at_least_24() {
    let mut raster = [0i32; 16];
    raster[0] = 1;
    // qp=28: shift=4, up=4-4=0, level_scale=16*normAdjust[4][0]=16*16=256, d=1*256<<0=256.
    let out = dequant_normal(&raster, 28).unwrap();
    assert_eq!(out[0], 256);
    assert_eq!(out[1..], [0; 15]);
}

#[test]
fn dequant_normal_matches_hand_computed_value_for_qp_below_24() {
    let mut raster = [0i32; 16];
    raster[0] = 1;
    // qp=0: shift=0, down=4, round=8, level_scale=16*normAdjust[0][0]=160, d=(160+8)>>4=10.
    let out = dequant_normal(&raster, 0).unwrap();
    assert_eq!(out[0], 10);
}

#[test]
fn dequant_luma_dc_matches_hand_computed_values() {
    let mut raster = [0i32; 16];
    raster[0] = 1;
    // qp=36 (>=36 branch): level_scale=16*normAdjust[0][0]=160, shift=6, up=0, d=160.
    assert_eq!(dequant_luma_dc(&raster, 36).unwrap()[0], 160);
    // qp=0 (<36 branch): down=6, round=32, d=(160+32)>>6=3.
    assert_eq!(dequant_luma_dc(&raster, 0).unwrap()[0], 3);
}

#[test]
fn dequant_chroma_dc_matches_hand_computed_value() {
    // qpc=5: level_scale=16*normAdjust[5][0]=16*18=288, up=0, out=(1*288)>>5=9.
    let out = dequant_chroma_dc(&[1, 0, 0, 0], 5).unwrap();
    assert_eq!(out, [9, 0, 0, 0]);
}

#[test]
fn inverse_transform_4x4_of_zero_is_zero() {
    assert_eq!(inverse_transform_4x4(&[0; 16]), [0; 16]);
}

#[test]
fn inverse_transform_4x4_spreads_a_dc_only_input_evenly() {
    let mut d = [0i32; 16];
    d[0] = 64;
    // Column then row butterfly of a lone DC=64 spreads it to all 16 positions at value
    // 64 pre-normalization; (64 + 32) >> 6 == 1 for all 16 output samples.
    assert_eq!(inverse_transform_4x4(&d), [1; 16]);
}

#[test]
fn inverse_hadamard_4x4_of_zero_is_zero() {
    assert_eq!(inverse_hadamard_4x4(&[0; 16]), [0; 16]);
}

#[test]
fn inverse_hadamard_4x4_spreads_a_dc_only_input_evenly() {
    let mut c = [0i32; 16];
    c[0] = 16;
    assert_eq!(inverse_hadamard_4x4(&c), [16; 16]);
}

#[test]
fn inverse_hadamard_2x2_of_dc_only_input_spreads_evenly() {
    assert_eq!(inverse_hadamard_2x2(&[4, 0, 0, 0]), [4, 4, 4, 4]);
}

#[test]
fn inverse_hadamard_2x2_of_all_ones_collapses_to_dc_only() {
    assert_eq!(inverse_hadamard_2x2(&[1, 1, 1, 1]), [4, 0, 0, 0]);
}
