#![allow(clippy::expect_used, reason = "unit tests")]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use mediaway_common::{
    Bytes, CodecKind, Rational, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};

use super::{
    CaptureError, CaptureOutputPreference, CaptureRegion, CursorCapture, DesktopVideoCapture,
    DesktopVideoCaptureConfig, capture_desktop_video_once,
};
use crate::Select;
use mediaway_common::NativeHandle;

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

#[test]
fn the_cursor_is_left_out_unless_asked_for() {
    // The default every backend now shares. Before the field existed it was only a
    // convention, and macOS followed the other one.
    assert_eq!(CursorCapture::default(), CursorCapture::Excluded);
    let time_base = Rational::new(1, 30);
    let screen = DesktopVideoCaptureConfig::screen(Select::Default, time_base);
    let window = DesktopVideoCaptureConfig::window(
        NativeHandle::new(1).expect("non-zero placeholder"),
        time_base,
    );
    assert_eq!(screen.cursor, CursorCapture::Excluded);
    assert_eq!(window.cursor, CursorCapture::Excluded);
}

const fn region(x: u32, y: u32, width: u32, height: u32) -> CaptureRegion {
    CaptureRegion {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn no_region_is_asked_for_by_default() {
    let time_base = Rational::new(1, 30);
    assert_eq!(
        DesktopVideoCaptureConfig::screen(Select::Default, time_base).region,
        None
    );
    let window = NativeHandle::new(1).expect("non-zero placeholder");
    assert_eq!(
        DesktopVideoCaptureConfig::window(window, time_base).region,
        None
    );
}

#[test]
fn a_region_fits_only_when_it_lies_entirely_inside() {
    assert!(
        region(0, 0, 640, 480).fits_within(640, 480),
        "exactly the surface"
    );
    assert!(
        region(100, 50, 540, 430).fits_within(640, 480),
        "touching the far corner"
    );
    assert!(
        !region(101, 50, 540, 430).fits_within(640, 480),
        "one column past the edge"
    );
    assert!(
        !region(100, 51, 540, 430).fits_within(640, 480),
        "one row past the edge"
    );
    assert!(
        !region(0, 0, 641, 480).fits_within(640, 480),
        "wider than the surface"
    );
}

#[test]
fn an_empty_region_never_fits() {
    assert!(!region(0, 0, 0, 480).fits_within(640, 480));
    assert!(!region(0, 0, 640, 0).fits_within(640, 480));
}

#[test]
fn a_region_whose_edge_overflows_u32_does_not_fit_rather_than_wrapping() {
    // `x + width` wraps to 99 in u32 arithmetic, which would "fit" a 640-wide surface.
    assert!(!region(u32::MAX - 100, 0, 200, 10).fits_within(640, 480));
    assert!(!region(0, u32::MAX, 10, 2).fits_within(640, 480));
}

#[test]
fn only_a_region_at_the_top_left_is_at_the_origin() {
    assert!(region(0, 0, 10, 10).is_at_origin());
    assert!(!region(1, 0, 10, 10).is_at_origin());
    assert!(!region(0, 1, 10, 10).is_at_origin());
}
