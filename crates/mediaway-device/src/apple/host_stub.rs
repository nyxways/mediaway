//! Non-Apple host stubs.
//!
//! `AppleBroadcastExtensionCapture` has no stub here — see `mod.rs`'s doc comment on why.
//! `AppleScreenCapture`'s stub uses the macOS-shaped `open(&DesktopVideoCaptureConfig)` — the
//! real macOS/iOS `open()` signatures genuinely differ (see `mod.rs` module docs); this stub
//! picks one arbitrarily since off-Apple callers cannot reach either real implementation anyway.

use crate::audio::{AudioCapture, AudioCaptureConfig};
use crate::camera::{CameraCapture, CameraCaptureConfig};
use crate::desktop::{DesktopVideoCapture, DesktopVideoCaptureConfig};
use crate::{CaptureError, DeviceKind, PermissionState, Support, Unavailable};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Rational, StreamInfo, VideoGeometry};

/// Apple camera capture stub.
pub struct AppleCameraCapture {
    _priv: (),
}

impl AppleCameraCapture {
    /// Unavailable off Apple platforms.
    pub const fn open(_config: &CameraCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl CameraCapture for AppleCameraCapture {
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

/// Apple microphone capture stub.
pub struct AppleMicrophoneCapture {
    _priv: (),
}

impl AppleMicrophoneCapture {
    /// Unavailable off Apple platforms.
    pub const fn open(_config: &AudioCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl AudioCapture for AppleMicrophoneCapture {
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

/// Apple screen capture stub.
pub struct AppleScreenCapture {
    _priv: (),
}

impl AppleScreenCapture {
    /// Unavailable off Apple platforms.
    pub const fn open(_config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl DesktopVideoCapture for AppleScreenCapture {
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

/// Apple window capture stub (macOS-only real backend — see `mod.rs` module docs).
pub struct AppleWindowCapture {
    _priv: (),
}

impl AppleWindowCapture {
    /// Unavailable off macOS.
    pub const fn open(_config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl DesktopVideoCapture for AppleWindowCapture {
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

/// No Apple backend is compiled into this binary at all off Apple platforms.
#[must_use]
pub const fn support(_kind: DeviceKind) -> Support {
    Support::Unavailable(Unavailable::NotImplemented)
}

/// No Apple backend is compiled into this binary at all off Apple platforms.
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
