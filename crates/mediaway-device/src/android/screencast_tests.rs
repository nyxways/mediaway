#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::cast_possible_wrap,
    clippy::significant_drop_tightening,
    reason = "test modules may unwrap / print"
)]

use super::*;
use mediaway_common::NativeHandle;

#[test]
fn push_frame_drops_oldest_past_capacity() {
    let queue = FrameQueue {
        frames: Mutex::new(VecDeque::new()),
    };
    for i in 0..(FRAME_QUEUE_CAP + 2) {
        push_frame(&queue, 1920, 1080, Bytes::new(), i as i64);
    }
    let q = queue.frames.lock().unwrap();
    assert_eq!(q.len(), FRAME_QUEUE_CAP);
    assert_eq!(q.front().unwrap().pts, 2);
}

#[test]
fn push_frame_sets_rgba_metadata() {
    let queue = FrameQueue {
        frames: Mutex::new(VecDeque::new()),
    };
    push_frame(&queue, 640, 480, Bytes::new(), 3);
    let q = queue.frames.lock().unwrap();
    let frame = q.front().unwrap();
    assert_eq!(frame.width, 640);
    assert_eq!(frame.height, 480);
    assert_eq!(frame.format, PixelFormat::Rgba8);
    assert_eq!(frame.pts, 3);
}

#[test]
fn config_is_copy_and_equatable() {
    let a = AndroidScreenCaptureConfig {
        media_projection: NativeHandle::new(1).unwrap(),
        java_vm: NativeHandle::new(2).unwrap(),
        width: 1920,
        height: 1080,
        density_dpi: 320,
        flags: 0,
        time_base: Rational::new(1, 30),
    };
    let b = a;
    assert_eq!(a, b);
}
