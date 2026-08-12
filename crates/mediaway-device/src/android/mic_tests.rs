use super::*;

#[test]
fn bytemuck_f32_to_bytes_round_trips_little_endian() {
    let samples = [1.0f32, -1.0, 0.5];
    let bytes = bytemuck_f32_to_bytes(&samples);
    assert_eq!(bytes.len(), 12);
    assert_eq!(&bytes[0..4], 1.0f32.to_le_bytes());
    assert_eq!(&bytes[4..8], (-1.0f32).to_le_bytes());
}

#[test]
fn push_frame_drops_oldest_past_capacity() {
    let queue = SharedQueue {
        frames: Mutex::new(VecDeque::new()),
    };
    for i in 0..(PCM_QUEUE_CAP + 4) {
        push_frame(&queue, 48_000, 2, i as i64, &[0.0, 0.0]);
    }
    let q = queue.frames.lock().unwrap();
    assert_eq!(q.len(), PCM_QUEUE_CAP);
    assert_eq!(q.front().unwrap().pts, 4);
}

#[test]
fn push_frame_sets_expected_metadata() {
    let queue = SharedQueue {
        frames: Mutex::new(VecDeque::new()),
    };
    push_frame(&queue, 44_100, 2, 7, &[0.1, 0.2, 0.3, 0.4]);
    let q = queue.frames.lock().unwrap();
    let frame = q.front().unwrap();
    assert_eq!(frame.pts, 7);
    assert_eq!(frame.duration, 2);
    assert_eq!(frame.sample_rate, 44_100);
    assert_eq!(frame.channels, 2);
    assert_eq!(frame.format, SampleFormat::F32);
    assert_eq!(frame.data.len(), 16);
}
