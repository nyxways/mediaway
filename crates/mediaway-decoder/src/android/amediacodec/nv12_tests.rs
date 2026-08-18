#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Build a `stride`/`slice_height`-padded NV12 buffer (luma plane at offset `0`, chroma plane
/// immediately after at `stride * slice_height`, matching `AMediaCodec`'s documented
/// semi-planar layout) filled with a sequential wrapping byte pattern so mis-copies (wrong
/// row/offset/crop) are easy to spot: expectations are computed by indexing straight back into
/// this same buffer, not by re-deriving values.
fn padded_nv12(stride: usize, slice_height: usize) -> Vec<u8> {
    let uv_rows = slice_height / 2;
    let mut data = vec![0u8; stride * slice_height + stride * uv_rows];
    let mut next = 0u8;
    for byte in &mut data {
        *byte = next;
        next = next.wrapping_add(1);
    }
    data
}

#[test]
fn tightly_packed_no_crop_round_trips_unchanged() {
    let width = 4u32;
    let height = 4u32;
    let data = padded_nv12(width as usize, height as usize);
    let crop = CropRect {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };

    let out = strip_and_crop_nv12(&data, width, height, crop);
    assert_eq!(out.as_ref(), data.as_slice());
}

#[test]
fn strips_stride_slice_height_padding_and_applies_crop() {
    let stride = 12u32;
    let slice_height = 10u32;
    let data = padded_nv12(stride as usize, slice_height as usize);
    let crop = CropRect {
        left: 2,
        top: 2,
        right: 10,
        bottom: 8,
    };
    let out_width = (crop.right - crop.left) as usize;
    let out_height = (crop.bottom - crop.top) as usize;

    let out = strip_and_crop_nv12(&data, stride, slice_height, crop);
    let expected_len = out_width * out_height + out_width * (out_height / 2);
    assert_eq!(out.len(), expected_len);

    let stride_usize = stride as usize;
    let mut expected = Vec::with_capacity(expected_len);
    for row in 0..out_height {
        let src_row = crop.top as usize + row;
        let start = src_row * stride_usize + crop.left as usize;
        expected.extend_from_slice(&data[start..start + out_width]);
    }
    let uv_offset = stride_usize * slice_height as usize;
    for row in 0..out_height / 2 {
        let src_row = crop.top as usize / 2 + row;
        let start = uv_offset + src_row * stride_usize + crop.left as usize;
        expected.extend_from_slice(&data[start..start + out_width]);
    }

    assert_eq!(out.as_ref(), expected.as_slice());
}

#[test]
fn missing_slice_height_key_falls_back_to_height_at_the_caller() {
    // This module never guesses `slice_height` itself — the caller resolves the
    // zero-means-"same as height" quirk before calling `strip_and_crop_nv12` (see
    // `super::video::adopt_output_format`). Verify only that a `slice_height` equal to the
    // crop's `bottom` behaves like an unpadded buffer for the chroma plane offset.
    let width = 6u32;
    let height = 4u32;
    let data = padded_nv12(width as usize, height as usize);
    let crop = CropRect {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };

    let out = strip_and_crop_nv12(&data, width, height, crop);
    assert_eq!(out.as_ref(), data.as_slice());
}

#[test]
fn out_of_range_crop_zero_fills_instead_of_panicking() {
    let stride = 4u32;
    let slice_height = 4u32;
    let data = vec![0u8; 4]; // far too small for the declared stride/slice_height
    let crop = CropRect {
        left: 0,
        top: 0,
        right: stride,
        bottom: slice_height,
    };

    let out = strip_and_crop_nv12(&data, stride, slice_height, crop);
    let expected_len = (stride * slice_height) as usize + (stride * slice_height / 2) as usize;
    assert_eq!(out.len(), expected_len);
    assert!(out.as_ref().iter().all(|&b| b == 0));
}

#[test]
fn zero_size_crop_returns_empty_bytes() {
    let data = padded_nv12(8, 8);
    let crop = CropRect {
        left: 4,
        top: 4,
        right: 4,
        bottom: 4,
    };

    let out = strip_and_crop_nv12(&data, 8, 8, crop);
    assert!(out.is_empty());
}
