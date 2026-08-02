use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use mediaway_common::{
    Bytes, CodecKind, Rational, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};

use super::{
    CaptureError, CaptureOutputPreference, DesktopVideoCapture, capture_desktop_video_once,
};

struct MockDesktopVideo {
    frame: Option<VideoFrame>,
    stream_info: StreamInfo,
    closed: Rc<RefCell<bool>>,
}

impl DesktopVideoCapture for MockDesktopVideo {
    fn stream_info(&self) -> &StreamInfo {
        &self.stream_info
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, CaptureError> {
        Ok(self.frame.take())
    }

    fn release_frame(&mut self) -> Result<(), CaptureError> {
        Ok(())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        *self.closed.borrow_mut() = true;
        Ok(())
    }
}

fn stream_info() -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: CodecKind::RawVideo,
        time_base: Rational::new(1, 30),
        geometry: VideoGeometry {
            width: 1,
            height: 1,
        },
        extra_data: Bytes::new(),
    }
}

fn cpu_frame() -> VideoFrame {
    VideoFrame {
        pts: 0,
        duration: 1,
        width: 1,
        height: 1,
        format: mediaway_common::PixelFormat::Rgba8,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from_static(&[0u8; 4]),
        },
    }
}

#[test]
fn capture_once_returns_cpu_frame() {
    let closed = Rc::new(RefCell::new(false));
    let result = capture_desktop_video_once(
        || {
            Ok(MockDesktopVideo {
                frame: Some(cpu_frame()),
                stream_info: stream_info(),
                closed: Rc::clone(&closed),
            })
        },
        Duration::from_millis(50),
    );
    assert!(result.is_ok());
}

#[test]
fn capture_once_times_out_when_nothing_ready() {
    let closed = Rc::new(RefCell::new(false));
    let result = capture_desktop_video_once(
        || {
            Ok(MockDesktopVideo {
                frame: None,
                stream_info: stream_info(),
                closed: Rc::clone(&closed),
            })
        },
        Duration::from_millis(10),
    );
    assert_eq!(result, Err(CaptureError::Timeout));
}

#[test]
fn capture_once_always_closes_the_session() {
    let closed = Rc::new(RefCell::new(false));
    let _ = capture_desktop_video_once(
        || {
            Ok(MockDesktopVideo {
                frame: Some(cpu_frame()),
                stream_info: stream_info(),
                closed: Rc::clone(&closed),
            })
        },
        Duration::from_millis(50),
    );
    assert!(*closed.borrow());
}

#[test]
fn default_output_preference_is_zero_copy_gpu() {
    assert_eq!(
        CaptureOutputPreference::default(),
        CaptureOutputPreference::ZeroCopyGpu
    );
}
