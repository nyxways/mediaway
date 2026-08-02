//! Non-Windows stub.

use mediaway_common::{Bytes, CodecKind, Rational, StreamInfo};
use mediaway_device::CaptureError;
use mediaway_device_audio::{
    AudioCapture, AudioCaptureConfig, AudioPlayback, AudioPlaybackConfig, PlaybackError,
};

/// Windows WASAPI capture stub.
pub struct WindowsWasapiCapture {
    _priv: (),
}

impl WindowsWasapiCapture {
    /// Unavailable off Windows.
    pub const fn open_microphone(_config: &AudioCaptureConfig) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl AudioCapture for WindowsWasapiCapture {
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

/// Windows WASAPI playback stub.
pub struct WindowsWasapiPlayback {
    _priv: (),
}

impl WindowsWasapiPlayback {
    /// Unavailable off Windows.
    pub const fn open(_config: &AudioPlaybackConfig) -> Result<Self, PlaybackError> {
        Err(PlaybackError::Unsupported)
    }
}

impl AudioPlayback for WindowsWasapiPlayback {
    fn stream_info(&self) -> &StreamInfo {
        closed_audio_info()
    }

    fn write_frame(&mut self, _frame: mediaway_common::AudioFrame) -> Result<(), PlaybackError> {
        Err(PlaybackError::Unsupported)
    }

    fn underrun_count(&self) -> u64 {
        0
    }

    fn close(&mut self) -> Result<(), PlaybackError> {
        Err(PlaybackError::Unsupported)
    }
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
