#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::cast_possible_wrap,
    clippy::significant_drop_tightening,
    reason = "test modules may unwrap / print"
)]

use super::*;

#[test]
fn interleave_pcm_f32_interleaves_two_channels() {
    let mut left = [1.0f32, 2.0, 3.0];
    let mut right = [10.0f32, 20.0, 30.0];
    let mut ptrs = [
        NonNull::new(left.as_mut_ptr()).unwrap(),
        NonNull::new(right.as_mut_ptr()).unwrap(),
    ];
    // SAFETY: `ptrs` holds 2 valid pointers, each readable for 3 elements.
    let out = unsafe { interleave_pcm_f32(ptrs.as_mut_ptr(), 2, 3) };
    assert_eq!(out, vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0]);
}

#[test]
fn interleave_pcm_f32_single_channel_is_identity() {
    let mut mono = [1.0f32, 2.0, 3.0, 4.0];
    let mut ptrs = [NonNull::new(mono.as_mut_ptr()).unwrap()];
    // SAFETY: `ptrs` holds 1 valid pointer, readable for 4 elements.
    let out = unsafe { interleave_pcm_f32(ptrs.as_mut_ptr(), 1, 4) };
    assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn bytes_from_f32_vec_round_trips_little_endian() {
    let samples = [1.0f32, -1.0];
    let bytes = bytes_from_f32_vec(&samples);
    assert_eq!(bytes.len(), 8);
    assert_eq!(&bytes[0..4], 1.0f32.to_le_bytes());
    assert_eq!(&bytes[4..8], (-1.0f32).to_le_bytes());
}

#[test]
fn push_frame_drops_oldest_past_capacity() {
    let queue = SharedQueue {
        frames: Mutex::new(VecDeque::new()),
    };
    for i in 0..(PCM_QUEUE_CAP + 3) {
        push_frame(&queue, 48_000, 2, i as i64, &[0.0, 0.0]);
    }
    let q = queue.frames.lock().unwrap();
    assert_eq!(q.len(), PCM_QUEUE_CAP);
    assert_eq!(q.front().unwrap().pts, 3);
}
