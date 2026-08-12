use super::*;

#[test]
fn push_frame_drops_oldest_past_capacity() {
    let queue = FrameQueue {
        frames: Mutex::new(VecDeque::new()),
    };
    for _ in 0..(FRAME_QUEUE_CAP + 2) {
        push_frame(&queue, (Bytes::new(), 1920, 1080));
    }
    let q = queue.frames.lock().unwrap();
    assert_eq!(q.len(), FRAME_QUEUE_CAP);
}

#[test]
fn push_frame_sets_nv12_metadata() {
    let queue = FrameQueue {
        frames: Mutex::new(VecDeque::new()),
    };
    push_frame(&queue, (Bytes::new(), 640, 480));
    let q = queue.frames.lock().unwrap();
    let frame = q.front().unwrap();
    assert_eq!(frame.width, 640);
    assert_eq!(frame.height, 480);
    assert_eq!(frame.format, PixelFormat::Nv12);
}

#[test]
fn take_sender_returns_once() {
    let (tx, _rx) = sync_channel::<()>(1);
    let state = Mutex::new(Some(tx));
    assert!(take_sender(&state).is_some());
    assert!(take_sender(&state).is_none());
}
