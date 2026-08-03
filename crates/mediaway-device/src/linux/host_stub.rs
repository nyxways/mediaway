//! Non-Linux host stubs.

use crate::audio::{AudioCapture, AudioCaptureConfig};
use crate::desktop::{DesktopVideoCapture, DesktopVideoCaptureConfig};
use crate::{CaptureError, DeviceKind, PermissionState, Support, Unavailable};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Rational, StreamInfo, VideoGeometry};

/// Linux screen capture stub.
pub struct LinuxScreenCapture {
    _priv: (),
}

impl LinuxScreenCapture {
    /// Unavailable off Linux.
    pub const fn open(_config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl DesktopVideoCapture for LinuxScreenCapture {
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

/// Linux camera capture stub.
pub struct LinuxCameraCapture {
    _priv: (),
}

impl LinuxCameraCapture {
    /// Unavailable off Linux.
    pub const fn open(_config: &crate::camera::CameraCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl crate::camera::CameraCapture for LinuxCameraCapture {
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

/// Linux window capture stub.
pub struct LinuxWindowCapture {
    _priv: (),
}

impl LinuxWindowCapture {
    /// Unavailable off Linux.
    pub const fn open(_config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl DesktopVideoCapture for LinuxWindowCapture {
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

/// Linux microphone capture stub.
pub struct LinuxMicrophoneCapture {
    _priv: (),
}

impl LinuxMicrophoneCapture {
    /// Unavailable off Linux.
    pub const fn open(_config: &AudioCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl AudioCapture for LinuxMicrophoneCapture {
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

/// No Linux backend is compiled into this binary at all off Linux.
#[must_use]
pub const fn support(_kind: DeviceKind) -> Support {
    Support::Unavailable(Unavailable::NotImplemented)
}

/// No Linux backend is compiled into this binary at all off Linux.
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
