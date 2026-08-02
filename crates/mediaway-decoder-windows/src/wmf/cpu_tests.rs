#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests may unwrap"
)]

use super::*;
use windows::Win32::Media::MediaFoundation::{
    MFCreate2DMediaBuffer, MFCreateMemoryBuffer, MFCreateSample, MFVideoFormat_NV12,
};

fn ensure_runtime() {
    super::super::runtime::ensure_mf().expect("MF runtime init");
}

#[test]
fn nv12_bytes_from_contiguous_buffer_matches_input() {
    ensure_runtime();
    let width = 4u32;
    let height = 4u32;
    let len = (width * height + width * height / 2) as usize;
    let pattern: Vec<u8> = (0..len)
        .map(|i| u8::try_from(i % 256).expect("i % 256 always fits in u8"))
        .collect();

    let sample = unsafe { MFCreateSample() }.expect("create sample");
    let buffer = unsafe { MFCreateMemoryBuffer(u32::try_from(len).expect("len fits u32")) }
        .expect("create memory buffer");
    unsafe {
        let mut ptr: *mut u8 = std::ptr::null_mut();
        buffer.Lock(&raw mut ptr, None, None).expect("lock");
        std::ptr::copy_nonoverlapping(pattern.as_ptr(), ptr, len);
        buffer
            .SetCurrentLength(u32::try_from(len).expect("len fits u32"))
            .expect("set current length");
        buffer.Unlock().expect("unlock");
        sample.AddBuffer(&buffer).expect("add buffer");
    }

    let out = nv12_bytes_from_output_sample(&sample, width, height).expect("extract nv12 bytes");
    assert_eq!(out.as_ref(), pattern.as_slice());
}

#[test]
fn nv12_bytes_from_2d_buffer_matches_input_regardless_of_stride() {
    ensure_runtime();
    let width = 16u32;
    let height = 16u32;
    let width_usize = width as usize;
    let height_usize = height as usize;

    let sample = unsafe { MFCreateSample() }.expect("create sample");
    let buffer =
        match unsafe { MFCreate2DMediaBuffer(width, height, MFVideoFormat_NV12.data1, false) } {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skip: MFCreate2DMediaBuffer failed ({e:?}) — no 2D buffer allocator?");
                return;
            }
        };
    let buf2d: IMF2DBuffer = buffer.cast().expect("cast to IMF2DBuffer");

    let mut scanline0: *mut u8 = std::ptr::null_mut();
    let mut pitch = 0i32;
    unsafe {
        buf2d
            .Lock2D(&raw mut scanline0, &raw mut pitch)
            .expect("lock2d");
    }
    let pitch_usize = pitch.unsigned_abs() as usize;
    let mut expected = vec![0u8; width_usize * height_usize + width_usize * (height_usize / 2)];
    let mut next = 0u8;
    unsafe {
        for row in 0..height_usize {
            let dst = scanline0.add(row * pitch_usize);
            for col in 0..width_usize {
                *dst.add(col) = next;
                expected[row * width_usize + col] = next;
                next = next.wrapping_add(1);
            }
        }
        let uv_dst_base = scanline0.add(height_usize * pitch_usize);
        let uv_expected_base = width_usize * height_usize;
        for row in 0..height_usize / 2 {
            let dst = uv_dst_base.add(row * pitch_usize);
            for col in 0..width_usize {
                *dst.add(col) = next;
                expected[uv_expected_base + row * width_usize + col] = next;
                next = next.wrapping_add(1);
            }
        }
        buf2d.Unlock2D().expect("unlock2d");
        sample.AddBuffer(&buffer).expect("add buffer");
    }

    let out = nv12_bytes_from_output_sample(&sample, width, height).expect("extract nv12 bytes");
    assert_eq!(out.as_ref(), expected.as_slice());
}
