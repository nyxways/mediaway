//! Non-Windows stub.

use crate::desktop::{
    DesktopAudioCapture, DesktopAudioCaptureConfig, DesktopVideoCapture, DesktopVideoCaptureConfig,
};
use crate::{CaptureError, DeviceInfo};
use mediaway_common::{Bytes, CodecKind, Rational, StreamInfo, VideoGeometry};

/// Windows screen capture stub.
pub struct WindowsScreenCapture {
    _priv: (),
}

impl WindowsScreenCapture {
    /// Unavailable off Windows.
    pub const fn open(_config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl DesktopVideoCapture for WindowsScreenCapture {
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

/// Windows window capture stub (WGC).
pub struct WindowsWindowCapture {
    _priv: (),
}

impl WindowsWindowCapture {
    /// Unavailable off Windows.
    pub const fn open(_config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl DesktopVideoCapture for WindowsWindowCapture {
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

/// Windows desktop audio capture stub.
pub struct WindowsDesktopAudioCapture {
    _priv: (),
}

impl WindowsDesktopAudioCapture {
    /// Unavailable off Windows.
    pub const fn open(_config: &DesktopAudioCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl DesktopAudioCapture for WindowsDesktopAudioCapture {
    fn stream_info(&self) -> &StreamInfo {
        closed_audio_info()
    }

    fn poll_frame(&mut self) -> Result<Option<mediaway_common::AudioFrame>, CaptureError> {
        Err(CaptureError::Unsupported)
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

/// Unavailable off Windows.
pub const fn exclude_window_from_capture(_hwnd: usize) -> Result<(), CaptureError> {
    Err(CaptureError::Unsupported)
}

/// No Windows backend is compiled into this binary at all off Windows.
pub const fn enumerate_outputs() -> Result<Vec<DeviceInfo>, CaptureError> {
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

fn closed_audio_info() -> &'static StreamInfo {
    use std::sync::OnceLock;
    static INFO: OnceLock<StreamInfo> = OnceLock::new();
    INFO.get_or_init(|| StreamInfo::Audio {
        id: 0,
        codec: CodecKind::RawAudio,
        time_base: Rational::new(1, 48_000),
        sample_rate: 0,
        channels: 0,
        extra_data: Bytes::new(),
    })
}
