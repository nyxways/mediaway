//! Unit tests for lacing sub-frame splitting.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::{Lacing, split};

#[test]
fn no_lacing_is_one_frame() {
    let body = [0u8, 1, 2, 3];
    let ranges = split(&body, 0, Lacing::None).unwrap();
    assert_eq!(&ranges[..], &[(0, 4)]);
}

#[test]
fn xiph_lacing_splits_three_frames() {
    // 2 explicit frames (count byte = 2 -> 3 frames total): sizes 4 and 300
    // (encoded as 255 + 45), then the remainder is the third frame.
    let mut body = vec![2u8]; // frame_count - 1 = 2
    body.push(4); // frame 1 size = 4
    body.push(255);
    body.push(45); // frame 2 size = 255 + 45 = 300
    body.extend(std::iter::repeat_n(0xAAu8, 4)); // frame 1 data
    body.extend(std::iter::repeat_n(0xBBu8, 300)); // frame 2 data
    body.extend(std::iter::repeat_n(0xCCu8, 7)); // frame 3 data (remainder)

    let ranges = split(&body, 0, Lacing::Xiph).unwrap();
    assert_eq!(ranges.len(), 3);
    assert_eq!(ranges[0].1 - ranges[0].0, 4);
    assert_eq!(ranges[1].1 - ranges[1].0, 300);
    assert_eq!(ranges[2].1 - ranges[2].0, 7);
}

#[test]
fn fixed_size_lacing_splits_evenly() {
    let mut body = vec![3u8]; // 4 frames total
    body.extend(std::iter::repeat_n(0u8, 40)); // 4 frames * 10 bytes each

    let ranges = split(&body, 0, Lacing::FixedSize).unwrap();
    assert_eq!(ranges.len(), 4);
    for r in &ranges {
        assert_eq!(r.1 - r.0, 10);
    }
}

#[test]
fn fixed_size_lacing_rejects_uneven_split() {
    let mut body = vec![2u8]; // 3 frames total
    body.extend(std::iter::repeat_n(0u8, 10)); // not divisible by 3
    assert!(split(&body, 0, Lacing::FixedSize).is_none());
}

#[test]
fn single_frame_lace_needs_no_size_fields() {
    let body = [0u8, 1, 2, 3, 4]; // frame_count byte = 0 -> 1 frame
    let ranges = split(&body, 0, Lacing::Xiph).unwrap();
    assert_eq!(&ranges[..], &[(1, 5)]);
}

#[test]
fn ebml_lacing_first_absolute_then_deltas() {
    // 3 frames: first size = 5 (1-byte VINT: marker 1xxxxxxx -> 0x85),
    // second size = first + delta; delta encoded as a 1-byte signed VINT
    // (bias 63 for a 1-byte VINT) of +2 -> raw = 63+2=65 -> with marker 0x80|65=0xC1.
    let mut body = vec![2u8]; // frame_count - 1 = 2 -> 3 frames
    body.push(0x80 | 5); // first size = 5
    body.push(0x80 | (63 + 2)); // delta = +2 -> second size = 7
    body.extend(std::iter::repeat_n(0xAAu8, 5));
    body.extend(std::iter::repeat_n(0xBBu8, 7));
    body.extend(std::iter::repeat_n(0xCCu8, 3)); // remainder = third frame

    let ranges = split(&body, 0, Lacing::Ebml).unwrap();
    assert_eq!(ranges.len(), 3);
    assert_eq!(ranges[0].1 - ranges[0].0, 5);
    assert_eq!(ranges[1].1 - ranges[1].0, 7);
    assert_eq!(ranges[2].1 - ranges[2].0, 3);
}

#[test]
fn malformed_lace_returns_none_not_panic() {
    let body = [5u8]; // claims 6 frames but no size/data bytes follow
    assert!(split(&body, 0, Lacing::Xiph).is_none());
    assert!(split(&body, 0, Lacing::Ebml).is_none());
}
