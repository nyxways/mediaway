use super::*;

#[test]
fn copy_rows_tight_packs_strided_rows() {
    // 2 rows of 4 bytes, stride 6 (2 bytes padding per row).
    let src = [
        1, 2, 3, 4, 0, 0, //
        5, 6, 7, 8, 0, 0,
    ];
    let mut out = Vec::new();
    copy_rows(&src, &mut out, 4, 2, 6).unwrap();
    assert_eq!(out, vec![1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn copy_rows_none_when_src_too_short() {
    let src = [1, 2, 3];
    let mut out = Vec::new();
    assert!(copy_rows(&src, &mut out, 4, 2, 4).is_none());
}

#[test]
fn map_camera_status_known_codes() {
    assert_eq!(
        map_camera_status(ndk_sys::camera_status_t(-10013)),
        CaptureError::AccessDenied
    );
    assert_eq!(
        map_camera_status(ndk_sys::camera_status_t(-10002)),
        CaptureError::DeviceLost
    );
    assert_eq!(
        map_camera_status(ndk_sys::camera_status_t(-10001)),
        CaptureError::InvalidInput
    );
    assert_eq!(
        map_camera_status(ndk_sys::camera_status_t(-10005)),
        CaptureError::Backend
    );
}

#[test]
fn push_frame_drops_oldest_past_capacity() {
    let queue = FrameQueue {
        frames: Mutex::new(VecDeque::new()),
    };
    for i in 0..(FRAME_QUEUE_CAP + 3) {
        push_frame(&queue, PixelFormat::I420, Bytes::new(), i as i64);
    }
    let q = queue.frames.lock().unwrap();
    assert_eq!(q.len(), FRAME_QUEUE_CAP);
    assert_eq!(q.front().unwrap().pts, 3);
}
