#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::cast_possible_wrap,
    clippy::significant_drop_tightening,
    clippy::panic,
    reason = "test modules may unwrap / print / panic on assertion failure"
)]

use super::*;

#[test]
fn push_bounded_drops_oldest_past_capacity() {
    let queue = Mutex::new(VecDeque::new());
    for i in 0..(QUEUE_CAP + 3) {
        push_bounded(&queue, i);
    }
    let q = queue.lock().unwrap();
    assert_eq!(q.len(), QUEUE_CAP);
    assert_eq!(*q.front().unwrap(), 3);
}

#[test]
fn audio_stream_info_reflects_frame_format() {
    let frame = AudioFrame {
        pts: 0,
        duration: 100,
        sample_rate: 44_100,
        channels: 2,
        format: SampleFormat::F32,
        data: Bytes::new(),
    };
    let info = audio_stream_info(&frame);
    let StreamInfo::Audio {
        sample_rate,
        channels,
        ..
    } = info
    else {
        panic!("expected StreamInfo::Audio");
    };
    assert_eq!(sample_rate, 44_100);
    assert_eq!(channels, 2);
}

#[test]
fn unknown_video_info_is_zeroed_placeholder() {
    let info = unknown_video_info();
    let StreamInfo::Video { geometry, .. } = info else {
        panic!("expected StreamInfo::Video");
    };
    assert_eq!(geometry.width, 0);
    assert_eq!(geometry.height, 0);
}

#[test]
fn broadcast_extension_capture_new_has_no_frames_until_pushed() {
    let mut cap = AppleBroadcastExtensionCapture::new();
    assert_eq!(DesktopVideoCapture::poll_frame(&mut cap).unwrap(), None);
    assert_eq!(DesktopAudioCapture::poll_frame(&mut cap).unwrap(), None);
    assert_eq!(AudioCapture::poll_frame(&mut cap).unwrap(), None);
}

#[test]
fn broadcast_extension_capture_close_then_poll_is_closed() {
    let mut cap = AppleBroadcastExtensionCapture::new();
    DesktopVideoCapture::close(&mut cap).unwrap();
    assert_eq!(
        DesktopVideoCapture::poll_frame(&mut cap),
        Err(CaptureError::Closed)
    );
}
