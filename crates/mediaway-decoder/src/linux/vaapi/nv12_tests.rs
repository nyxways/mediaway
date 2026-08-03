#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Build a stride-padded NV12 buffer (`pitch > width`) with a distinct byte pattern per plane
/// so mis-copies (wrong offset/pitch/plane order) are easy to spot in assertions.
fn padded_nv12(width: usize, height: usize, pitch: usize) -> (Vec<u8>, u32, u32) {
    assert!(pitch >= width);
    let uv_rows = height / 2;
    let y_offset = 0usize;
    let uv_offset = pitch * height; // planes back-to-back, chroma right after luma rows
    let mut data = vec![0u8; uv_offset + pitch * uv_rows];

    let mut next = 0u8;
    for row in 0..height {
        for col in 0..width {
            data[y_offset + row * pitch + col] = next;
            next = next.wrapping_add(1);
        }
    }
    for row in 0..uv_rows {
        for col in 0..width {
            data[uv_offset + row * pitch + col] = next;
            next = next.wrapping_add(1);
        }
    }

    (
        data,
        u32::try_from(y_offset).unwrap(),
        u32::try_from(uv_offset).unwrap(),
    )
}

#[test]
fn copies_tightly_packed_nv12_regardless_of_stride_padding() {
    let width = 16u32;
    let height = 16u32;
    let pitch = 24usize; // padded stride, larger than width
    let (data, y_offset, uv_offset) = padded_nv12(width as usize, height as usize, pitch);
    let pitch_u32 = u32::try_from(pitch).unwrap();

    let out = copy_nv12_from_planes(
        &data, width, height, pitch_u32, y_offset, pitch_u32, uv_offset,
    );

    let expected_len = (width * height + width * height / 2) as usize;
    assert_eq!(out.len(), expected_len);

    // Reconstruct the same 0..=255 wrapping pattern the fixture used and compare, proving the
    // stride was stripped correctly (no leftover padding bytes, correct row order).
    let mut expected = vec![0u8; expected_len];
    let mut next = 0u8;
    let width_usize = width as usize;
    let height_usize = height as usize;
    for row in 0..height_usize {
        for col in 0..width_usize {
            expected[row * width_usize + col] = next;
            next = next.wrapping_add(1);
        }
    }
    let y_plane_bytes = width_usize * height_usize;
    for row in 0..height_usize / 2 {
        for col in 0..width_usize {
            expected[y_plane_bytes + row * width_usize + col] = next;
            next = next.wrapping_add(1);
        }
    }
    assert_eq!(out.as_ref(), expected.as_slice());
}

#[test]
fn tightly_packed_input_round_trips_unchanged() {
    let width = 4u32;
    let height = 4u32;
    let len = (width * height + width * height / 2) as usize;
    let data: Vec<u8> = (0..len)
        .map(|i| u8::try_from(i % 256).expect("i % 256 always fits in u8"))
        .collect();

    let out = copy_nv12_from_planes(&data, width, height, width, 0, width, width * height);
    assert_eq!(out.as_ref(), data.as_slice());
}

#[test]
fn out_of_range_offsets_zero_fill_instead_of_panicking() {
    let width = 4u32;
    let height = 4u32;
    let data = vec![0u8; 4]; // far too small for any real plane

    let out = copy_nv12_from_planes(&data, width, height, width, 1000, width, 2000);
    let expected_len = (width * height + width * height / 2) as usize;
    assert_eq!(out.len(), expected_len);
    assert!(out.as_ref().iter().all(|&b| b == 0));
}
