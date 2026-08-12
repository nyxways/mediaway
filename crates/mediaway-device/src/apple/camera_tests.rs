use super::*;

#[test]
fn push_frame_drops_oldest_past_capacity() {
    let queue = FrameQueue {
        frames: Mutex::new(VecDeque::new()),
    };
    for i in 0..(FRAME_QUEUE_CAP + 2) {
        push_frame(&queue, (Bytes::new(), 1280, 720), i as i64);
    }
    let q = queue.frames.lock().unwrap();
    assert_eq!(q.len(), FRAME_QUEUE_CAP);
    assert_eq!(q.front().unwrap().pts, 2);
}

#[test]
fn push_frame_sets_nv12_metadata() {
    let queue = FrameQueue {
        frames: Mutex::new(VecDeque::new()),
    };
    push_frame(&queue, (Bytes::new(), 640, 480), 3);
    let q = queue.frames.lock().unwrap();
    let frame = q.front().unwrap();
    assert_eq!(frame.width, 640);
    assert_eq!(frame.height, 480);
    assert_eq!(frame.format, PixelFormat::Nv12);
    assert_eq!(frame.pts, 3);
}
