//! Non-Windows stub.

use crate::camera::{CameraCapture, CameraCaptureConfig};
use crate::{CaptureError, DeviceInfo};
use mediaway_common::{Bytes, CodecKind, Rational, StreamInfo, VideoGeometry};

/// Windows camera capture stub.
pub struct WindowsCameraCapture {
    _priv: (),
}

impl WindowsCameraCapture {
    /// Unavailable off Windows.
    pub const fn open(_config: &CameraCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl CameraCapture for WindowsCameraCapture {
    fn stream_info(&self) -> &StreamInfo {
        closed_video_info()
    }

    fn poll_frame(&mut self) -> Result<Option<mediaway_common::VideoFrame>, CaptureError> {
        Err(CaptureError::Unsupported)
    }

    fn release_frame(&mut self) -> Result<(), CaptureError> {
        Err(CaptureError::Unsupported)
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

/// No Windows backend is compiled into this binary at all off Windows.
pub const fn enumerate_cameras() -> Result<Vec<DeviceInfo>, CaptureError> {
    Err(CaptureError::Unsupported)
}

fn closed_video_info() -> &'static StreamInfo {
    use std::sync::OnceLock;
    static INFO: OnceLock<StreamInfo> = OnceLock::new();
    INFO.get_or_init(|| StreamInfo::Video {
        id: 0,
        codec: CodecKind::RawVideo,
        time_base: Rational::new(1, 30),
        geometry: VideoGeometry {
            width: 0,
            height: 0,
        },
        extra_data: Bytes::new(),
    })
}
