#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test module may unwrap"
)]
#![allow(
    clippy::float_cmp,
    reason = "timestamps round-trip exact f64 literals through a plain getter, no arithmetic"
)]

use super::*;

#[test]
fn frame_count_matches_input_length() {
    let frames =
        DecodedVideoFrames::new(vec![0.0, 1000.0, 2000.0], vec![vec![1], vec![2], vec![3]]);
    assert_eq!(frames.frame_count(), 3);
}

#[test]
fn timestamp_and_luma_plane_read_back_by_index() {
    let frames = DecodedVideoFrames::new(vec![0.0, 33_333.0], vec![vec![10, 20], vec![30, 40]]);
    assert_eq!(frames.timestamp_us(0), 0.0);
    assert_eq!(frames.timestamp_us(1), 33_333.0);
    assert_eq!(frames.luma_plane(0), vec![10, 20]);
    assert_eq!(frames.luma_plane(1), vec![30, 40]);
}

#[test]
fn empty_frames_has_zero_count() {
    let frames = DecodedVideoFrames::new(Vec::new(), Vec::new());
    assert_eq!(frames.frame_count(), 0);
}
