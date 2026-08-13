//! Non-Android host stubs.

use crate::audio::{AudioCapture, AudioCaptureConfig};
use crate::camera::{CameraCapture, CameraCaptureConfig};
use crate::desktop::DesktopVideoCapture;
use crate::{CaptureError, DeviceKind, PermissionState, Support, Unavailable};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Rational, StreamInfo, VideoGeometry};

use super::AndroidScreenCaptureConfig;

/// Android camera capture stub.
pub struct AndroidCameraCapture {
    _priv: (),
}

impl AndroidCameraCapture {
    /// Unavailable off Android.
    pub const fn open(_config: &CameraCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl CameraCapture for AndroidCameraCapture {
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

/// Android microphone capture stub.
pub struct AndroidMicrophoneCapture {
    _priv: (),
}

impl AndroidMicrophoneCapture {
    /// Unavailable off Android.
    pub const fn open(_config: &AudioCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl AudioCapture for AndroidMicrophoneCapture {
    fn stream_info(&self) -> &StreamInfo {
        closed_audio_info()
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, CaptureError> {
        Err(CaptureError::Unsupported)
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

/// Android screen capture stub.
pub struct AndroidScreenCapture {
    _priv: (),
}

impl AndroidScreenCapture {
    /// Unavailable off Android.
    pub const fn open(_config: &AndroidScreenCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl DesktopVideoCapture for AndroidScreenCapture {
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

/// No Android backend is compiled into this binary at all off Android.
#[must_use]
pub const fn support(_kind: DeviceKind) -> Support {
    Support::Unavailable(Unavailable::NotImplemented)
}

/// No Android backend is compiled into this binary at all off Android.
pub const fn request_permission(_kind: DeviceKind) -> Result<PermissionState, CaptureError> {
    Ok(PermissionState::NotSupported)
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
