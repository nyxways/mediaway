//! Platform dispatch — the **only** module in this crate allowed to contain
//! `#[cfg(target_os = …)]` / `#[cfg(windows)]`.
//!
//! All public functions return `Box<dyn Trait>` so callers stay
//! platform-agnostic. On an unsupported platform every open returns
//! `Err(…::NoBackend)`. Migrated from the former `examples/platform.rs`
//! (see [ADR-0014](../../../docs/adr/0014-pipeline-convenience-crate.md)).

use mediaway_common::CodecKind;
use mediaway_decoder::capability::{DecodeSupport, DecodeUnavailable};
use mediaway_decoder::{DecodeError, VideoDecoder, VideoDecoderConfig};
use mediaway_device::audio::{AudioCapture, AudioCaptureConfig};
use mediaway_device::desktop::{DesktopVideoCapture, DesktopVideoCaptureConfig};
use mediaway_device::{CaptureError, DeviceKind, PermissionState, Support};
use mediaway_encoder::auto::AutoVideoEncodeConfig;
use mediaway_encoder::capability::EncoderCapability;
use mediaway_encoder::{EncodeError, VideoEncoder};

// ── Video encoder ─────────────────────────────────────────────────────────────

/// Opens the best available video encoder on the current platform.
///
/// A zero-sized marker type — no state, just a home for [`Self::open`]; the real
/// per-platform dispatch lives inside it, `#[cfg]`-gated.
pub struct AutoEncoder;

impl AutoEncoder {
    /// Open the best available video encoder for `config` on the current platform.
    ///
    /// Returns `Err(EncodeError::NoBackend)` when no backend is compiled in.
    ///
    /// # Errors
    ///
    /// Propagates backend open errors.
    pub fn open(config: &AutoVideoEncodeConfig) -> Result<Box<dyn VideoEncoder>, EncodeError> {
        #[cfg(windows)]
        {
            use mediaway_encoder::windows::auto::AutoVideoEncoder;
            let enc = AutoVideoEncoder::open(config)?;
            Ok(Box::new(enc))
        }

        #[cfg(target_os = "linux")]
        {
            // mediaway-encoder-linux has no `auto` module (unlike Windows): its VA-API
            // backend only implements `VideoInputPreference::CpuUploadOk` today (see
            // that crate's ADR-0001), so there is no path selection to do here.
            use mediaway_encoder::VideoInputPreference;
            use mediaway_encoder::linux::LinuxVideoEncoder;
            let low_level =
                config.to_low_level(VideoInputPreference::CpuUploadOk, config.gpu_device);
            let enc = LinuxVideoEncoder::open(&low_level)?;
            Ok(Box::new(enc))
        }

        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = config;
            Err(EncodeError::NoBackend)
        }
    }
}

/// Probe every encode [`mediaway_encoder::auto::Backend`] for `codec` on the current
/// platform (see [`mediaway_encoder::capability`]).
///
/// **Windows:** a real, live (costly) probe — see
/// `mediaway_encoder::windows::auto::support`.
///
/// **Other platforms:** empty — `mediaway-encoder-linux`/other platform crates have no
/// per-backend selection surface yet (`AutoEncoder::open` always resolves to their one
/// VA-API-style path), so there is nothing to report without inventing rows that
/// correspond to nothing real.
#[must_use]
#[allow(
    clippy::missing_const_for_fn,
    reason = "the Windows branch calls a non-const backend probe; only the other-platforms \
              branch (compiled alone on non-Windows) is trivially const-eligible"
)]
pub fn encoder_support(codec: CodecKind) -> Vec<EncoderCapability> {
    #[cfg(windows)]
    {
        mediaway_encoder::windows::auto::support(codec)
    }

    #[cfg(not(windows))]
    {
        let _ = codec;
        Vec::new()
    }
}

// ── Video decoder ─────────────────────────────────────────────────────────────

/// Opens the best available video decoder on the current platform.
///
/// A zero-sized marker type — no state, just a home for [`Self::open`]; the real
/// per-platform dispatch lives inside it, `#[cfg]`-gated.
pub struct AutoDecoder;

impl AutoDecoder {
    /// Open the best available video decoder for `config` on the current platform.
    ///
    /// Returns `Err(DecodeError::NoBackend)` when no backend is compiled in.
    ///
    /// # Errors
    ///
    /// Propagates backend open errors.
    pub fn open(config: &VideoDecoderConfig) -> Result<Box<dyn VideoDecoder>, DecodeError> {
        #[cfg(windows)]
        {
            use mediaway_decoder_windows::WindowsVideoDecoder;
            let dec = WindowsVideoDecoder::open(config)?;
            Ok(Box::new(dec))
        }

        #[cfg(target_os = "linux")]
        {
            use mediaway_decoder_linux::LinuxVideoDecoder;
            let dec = LinuxVideoDecoder::open(config)?;
            Ok(Box::new(dec))
        }

        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = config;
            Err(DecodeError::NoBackend)
        }
    }
}

/// Probe whether video decode for `codec` is usable on the current machine right now
/// (see [`mediaway_decoder::capability`]).
///
/// Unlike [`encoder_support`], decode has exactly one implementation per platform
/// today — no competing `Backend`s to enumerate (`mediaway-decoder-vulkan` exists but
/// is not wired into [`AutoDecoder::open`]) — so this reports a single
/// [`DecodeSupport`] per codec, not a `Vec` of rows. A tiny throwaway 64×64 open is
/// costly (real MFT/VA-API session), same trade-off as `encoder_support`.
#[must_use]
pub fn decoder_support(codec: CodecKind) -> DecodeSupport {
    #[cfg(windows)]
    {
        use mediaway_decoder_windows::WindowsVideoDecoder;
        let cfg = VideoDecoderConfig {
            codec,
            output: mediaway_decoder::VideoOutputPreference::CpuFramesOk,
            ..VideoDecoderConfig::h264(64, 64, mediaway_common::Rational::new(1, 30))
        };
        match WindowsVideoDecoder::open(&cfg) {
            Ok(_) => DecodeSupport::Supported,
            Err(DecodeError::Unsupported) => {
                DecodeSupport::Unavailable(DecodeUnavailable::NotImplemented)
            }
            Err(_) => DecodeSupport::Unavailable(DecodeUnavailable::NoDevice),
        }
    }

    #[cfg(target_os = "linux")]
    {
        use mediaway_decoder_linux::LinuxVideoDecoder;
        let cfg = VideoDecoderConfig {
            codec,
            output: mediaway_decoder::VideoOutputPreference::CpuFramesOk,
            ..VideoDecoderConfig::h264(64, 64, mediaway_common::Rational::new(1, 30))
        };
        match LinuxVideoDecoder::open(&cfg) {
            Ok(_) => DecodeSupport::Supported,
            Err(DecodeError::Unsupported) => {
                DecodeSupport::Unavailable(DecodeUnavailable::NotImplemented)
            }
            Err(_) => DecodeSupport::Unavailable(DecodeUnavailable::NoDevice),
        }
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = codec;
        DecodeSupport::Unavailable(DecodeUnavailable::NotImplemented)
    }
}

// ── Screen / display capture ──────────────────────────────────────────────────

/// Opens a screen capture session on the current platform.
///
/// A zero-sized marker type — no state, just a home for [`Self::open`]; the real
/// per-platform dispatch lives inside it, `#[cfg]`-gated. Use
/// [`mediaway_device::desktop::DesktopVideoCaptureConfig::screen`] to build the
/// primary-display config this takes.
pub struct ScreenCapture;

impl ScreenCapture {
    /// Open a screen capture session for the given config on the current platform.
    ///
    /// Returns `Err(CaptureError::NoBackend)` when no backend is compiled in.
    ///
    /// # Errors
    ///
    /// Propagates backend open errors.
    pub fn open(
        config: &DesktopVideoCaptureConfig,
    ) -> Result<Box<dyn DesktopVideoCapture>, CaptureError> {
        #[cfg(windows)]
        {
            use mediaway_device::windows_desktop::WindowsScreenCapture;
            let cap = WindowsScreenCapture::open(config)?;
            Ok(Box::new(cap))
        }

        #[cfg(target_os = "linux")]
        {
            use mediaway_device::linux::LinuxScreenCapture;
            let cap = LinuxScreenCapture::open(config)?;
            Ok(Box::new(cap))
        }

        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = config;
            Err(CaptureError::NoBackend)
        }
    }
}

// ── Microphone capture ────────────────────────────────────────────────────────

/// Opens the default microphone on the current platform.
///
/// A zero-sized marker type — no state, just a home for [`Self::open`]; the real
/// per-platform dispatch lives inside it, `#[cfg]`-gated.
pub struct Microphone;

impl Microphone {
    /// Open the default microphone on the current platform.
    ///
    /// Returns `Err(CaptureError::NoBackend)` when no backend is compiled in.
    ///
    /// # Errors
    ///
    /// Propagates backend open errors.
    pub fn open(config: &AudioCaptureConfig) -> Result<Box<dyn AudioCapture>, CaptureError> {
        #[cfg(windows)]
        {
            use mediaway_device::windows_audio::WindowsWasapiCapture;
            let cap = WindowsWasapiCapture::open_microphone(config)?;
            Ok(Box::new(cap))
        }

        #[cfg(not(windows))]
        {
            let _ = config;
            Err(CaptureError::NoBackend)
        }
    }
}

// ── Device capability / permission ────────────────────────────────────────────

/// Whether a capture backend exists for `kind` on the current platform (see
/// [`mediaway_device` ADR-0003](../../mediaway-device/adr/0003-capability-and-permission-probe.md)).
#[must_use]
pub fn device_support(kind: DeviceKind) -> Support {
    #[cfg(windows)]
    {
        mediaway_device::windows::support(kind)
    }

    #[cfg(target_os = "linux")]
    {
        mediaway_device::linux::support(kind)
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = kind;
        Support::Unavailable(mediaway_device::Unavailable::NotImplemented)
    }
}

/// Best-effort OS permission probe for `kind` on the current platform.
///
/// See backend docs (`mediaway-device-windows`/`mediaway-device-linux`
/// `capabilities.rs`) for per-kind cost — this can open a real device session.
///
/// # Errors
///
/// Propagates backend probe failures.
pub fn request_device_permission(kind: DeviceKind) -> Result<PermissionState, CaptureError> {
    #[cfg(windows)]
    {
        mediaway_device::windows::request_permission(kind)
    }

    #[cfg(target_os = "linux")]
    {
        mediaway_device::linux::request_permission(kind)
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = kind;
        Ok(PermissionState::NotSupported)
    }
}
