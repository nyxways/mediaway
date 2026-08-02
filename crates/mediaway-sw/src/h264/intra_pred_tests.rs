#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn predict_16x16_vertical_repeats_top_row() {
    let top = [10u8; 16];
    let out = predict_16x16(0, Some(&top), None, None).unwrap();
    assert_eq!(out, [10u8; 256]);
}

#[test]
fn predict_16x16_horizontal_repeats_left_column_per_row() {
    let mut left = [0u8; 16];
    for (i, v) in left.iter_mut().enumerate() {
        *v = i as u8;
    }
    let out = predict_16x16(1, None, Some(&left), None).unwrap();
    for y in 0..16 {
        assert_eq!(&out[y * 16..y * 16 + 16], [y as u8; 16].as_slice());
    }
}

#[test]
fn predict_16x16_dc_averages_uniform_top_and_left() {
    let top = [16u8; 16];
    let left = [16u8; 16];
    let out = predict_16x16(2, Some(&top), Some(&left), None).unwrap();
    assert_eq!(out, [16u8; 256]);
}

#[test]
fn predict_16x16_dc_falls_back_to_128_when_no_neighbors() {
    let out = predict_16x16(2, None, None, None).unwrap();
    assert_eq!(out, [128u8; 256]);
}

#[test]
fn predict_16x16_dc_uses_top_only_when_left_unavailable() {
    let top = [16u8; 16];
    let out = predict_16x16(2, Some(&top), None, None).unwrap();
    assert_eq!(out, [16u8; 256]);
}

#[test]
fn predict_16x16_plane_of_uniform_neighbors_reproduces_the_constant() {
    let top = [100u8; 16];
    let left = [100u8; 16];
    let out = predict_16x16(3, Some(&top), Some(&left), Some(100)).unwrap();
    assert_eq!(out, [100u8; 256]);
}

#[test]
fn predict_16x16_rejects_vertical_without_top() {
    assert_eq!(
        predict_16x16(0, None, Some(&[0; 16]), None),
        Err(H264Error::UnavailableIntraNeighbor)
    );
}

#[test]
fn predict_16x16_rejects_invalid_mode() {
    assert_eq!(
        predict_16x16(4, Some(&[0; 16]), Some(&[0; 16]), Some(0)),
        Err(H264Error::InvalidMbType)
    );
}

#[test]
fn predict_chroma_8x8_vertical_repeats_top_row() {
    let top = [7u8; 8];
    let out = predict_chroma_8x8(2, Some(&top), None, None).unwrap();
    assert_eq!(out, [7u8; 64]);
}

#[test]
fn predict_chroma_8x8_horizontal_repeats_left_column_per_row() {
    let mut left = [0u8; 8];
    for (i, v) in left.iter_mut().enumerate() {
        *v = (i as u8) * 10;
    }
    let out = predict_chroma_8x8(1, None, Some(&left), None).unwrap();
    for y in 0..8 {
        assert_eq!(&out[y * 8..y * 8 + 8], [(y as u8) * 10; 8].as_slice());
    }
}

#[test]
fn predict_chroma_8x8_dc_uses_per_quadrant_averaging_rules() {
    let top = [100u8; 8];
    let left = [50u8; 8];
    let out = predict_chroma_8x8(0, Some(&top), Some(&left), None).unwrap();

    // Top-left combines top+left: (400 + 200 + 4) >> 3 = 75.
    assert_eq!(out[0], 75);
    // Top-right prefers top only: avg4(100,100,100,100) = 100.
    assert_eq!(out[4], 100);
    // Bottom-left prefers left only: avg4(50,50,50,50) = 50.
    assert_eq!(out[4 * 8], 50);
    // Bottom-right combines top+left: same sums as top-left = 75.
    assert_eq!(out[4 * 8 + 4], 75);
}

#[test]
fn predict_chroma_8x8_dc_falls_back_to_128_with_no_neighbors() {
    let out = predict_chroma_8x8(0, None, None, None).unwrap();
    assert_eq!(out, [128u8; 64]);
}

#[test]
fn predict_chroma_8x8_plane_of_uniform_neighbors_reproduces_the_constant() {
    let top = [50u8; 8];
    let left = [50u8; 8];
    let out = predict_chroma_8x8(3, Some(&top), Some(&left), Some(50)).unwrap();
    assert_eq!(out, [50u8; 64]);
}

#[test]
fn predict_chroma_8x8_rejects_plane_without_corner() {
    assert_eq!(
        predict_chroma_8x8(3, Some(&[0; 8]), Some(&[0; 8]), None),
        Err(H264Error::UnavailableIntraNeighbor)
    );
}

#[test]
fn predict_chroma_8x8_rejects_invalid_mode() {
    assert_eq!(
        predict_chroma_8x8(4, Some(&[0; 8]), Some(&[0; 8]), Some(0)),
        Err(H264Error::InvalidMbType)
    );
}
