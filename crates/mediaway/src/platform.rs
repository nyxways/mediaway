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
            // mediaway-encoder-linux has no `auto` module (unlike Windows), but its VA-API
            // backend does have a real Zero-Copy path (`GpuBufferHandle::DmaBuf`, adr/linux/
            // 0006) alongside CpuUpload — try it first when the caller signals GPU-resident
            // frames (`gpu_device: Some(_)`), matching the Windows chain's ZC-before-CPU
            // ordering. See ADR-0004.
            use mediaway_encoder::VideoInputPreference;
            use mediaway_encoder::linux::LinuxVideoEncoder;
            if config.gpu_device.is_some() {
                let low = config.to_low_level(VideoInputPreference::ZeroCopyGpu, config.gpu_device);
                if let Ok(enc) = LinuxVideoEncoder::open(&low) {
                    return Ok(Box::new(enc));
                }
            }
            let low = config.to_low_level(VideoInputPreference::CpuUploadOk, config.gpu_device);
            let enc = LinuxVideoEncoder::open(&low)?;
            Ok(Box::new(enc))
        }

        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            // mediaway-encoder::apple has no `auto` module either, but `VTCompressionSession`
            // does have a real Zero-Copy path (`GpuBufferHandle::Metal`, apple/adr/0003)
            // alongside CpuUpload — same ZC-before-CPU ordering as Linux/Windows. See ADR-0004.
            use mediaway_encoder::VideoInputPreference;
            use mediaway_encoder::apple::AppleVideoEncoder;
            if config.gpu_device.is_some() {
                let low = config.to_low_level(VideoInputPreference::ZeroCopyGpu, config.gpu_device);
                if let Ok(enc) = AppleVideoEncoder::open(&low) {
                    return Ok(Box::new(enc));
                }
            }
            let low = config.to_low_level(VideoInputPreference::CpuUploadOk, config.gpu_device);
            let enc = AppleVideoEncoder::open(&low)?;
            Ok(Box::new(enc))
        }

        #[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "ios")))]
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
        if codec == CodecKind::Opus {
            // No inbox WMF Opus encoder MFT exists — the facade's Opus encode
            // path is `mediaway-sw` (Software). Probe it live.
            use mediaway_common::{Rational, SampleFormat};
            use mediaway_encoder::auto::{Backend, EncodePathClass};
            use mediaway_encoder::capability::{EncodeSupport, EncodeUnavailable};
            use mediaway_encoder::{AudioEncoderConfig, EncodeError, SwOpusAudioEncoder};
            let cfg = AudioEncoderConfig {
                codec,
                sample_rate: 48_000,
                channels: 2,
                sample_format: SampleFormat::F32,
                time_base: Rational::new(1, 50),
                bitrate_bps: 0,
            };
            let support = match SwOpusAudioEncoder::open(&cfg) {
                Ok(_) => EncodeSupport::Supported(EncodePathClass::Software),
                Err(EncodeError::Backend) => {
                    EncodeSupport::Unavailable(EncodeUnavailable::NoDevice)
                }
                Err(_) => EncodeSupport::Unavailable(EncodeUnavailable::NotImplemented),
            };
            return vec![EncoderCapability::new(Backend::Software, support)];
        }
        mediaway_encoder::windows::auto::support(codec)
    }

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        if codec == CodecKind::Opus {
            // Native AudioConverter Opus encoder (ADR-0005) — probe it live, same cost
            // trade-off as every other probe here.
            use mediaway_common::{Rational, SampleFormat};
            use mediaway_encoder::apple::AppleAudioEncoder;
            use mediaway_encoder::auto::{Backend, EncodePathClass};
            use mediaway_encoder::capability::{EncodeSupport, EncodeUnavailable};
            use mediaway_encoder::{AudioEncoderConfig, EncodeError};
            let cfg = AudioEncoderConfig {
                codec,
                sample_rate: 48_000,
                channels: 2,
                sample_format: SampleFormat::F32,
                time_base: Rational::new(1, 50),
                bitrate_bps: 0,
            };
            let support = match AppleAudioEncoder::open(&cfg) {
                // Native `AudioConverter` session, no GPU handle involved — closest fit among
                // `EncodePathClass`'s video-shaped variants is `CpuUpload` (CPU-resident data
                // fed to an OS encoder), same category the Windows WMF AAC path would report.
                Ok(_) => EncodeSupport::Supported(EncodePathClass::CpuUpload),
                Err(EncodeError::Backend) => {
                    EncodeSupport::Unavailable(EncodeUnavailable::NoDevice)
                }
                Err(_) => EncodeSupport::Unavailable(EncodeUnavailable::NotImplemented),
            };
            return vec![EncoderCapability::new(Backend::Os, support)];
        }
        let _ = codec;
        Vec::new()
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "ios")))]
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
            use mediaway_decoder::windows::WindowsVideoDecoder;
            let dec = WindowsVideoDecoder::open(config)?;
            Ok(Box::new(dec))
        }

        #[cfg(target_os = "linux")]
        {
            use mediaway_decoder::linux::LinuxVideoDecoder;
            let dec = LinuxVideoDecoder::open(config)?;
            Ok(Box::new(dec))
        }

        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            use mediaway_decoder::apple::AppleVideoDecoder;
            let dec = AppleVideoDecoder::open(config)?;
            Ok(Box::new(dec))
        }

        #[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "ios")))]
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
        use mediaway_decoder::windows::{OpusDecoderConfig, WindowsVideoDecoder, WmfOpusDecoder};
        if codec == CodecKind::Opus {
            // Inbox WMF Opus decoder MFT (CMSOpusDecMFT) — probe with a real
            // session, same cost trade-off as the video probes.
            return match WmfOpusDecoder::open(&OpusDecoderConfig::new(48_000, 2)) {
                Ok(_) => DecodeSupport::Supported,
                Err(_) => DecodeSupport::Unavailable(DecodeUnavailable::NoDevice),
            };
        }
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
        use mediaway_decoder::linux::LinuxVideoDecoder;
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

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use mediaway_decoder::apple::AppleVideoDecoder;
        if codec == CodecKind::Opus {
            // Native AudioConverter Opus decoder (ADR-0005) — probe it live, same cost
            // trade-off as every other probe here.
            use mediaway_decoder::apple::{OpusDecoder, OpusDecoderConfig};
            let cfg = OpusDecoderConfig::new(48_000, 2, mediaway_common::Rational::new(1, 50));
            return match OpusDecoder::open(&cfg) {
                Ok(_) => DecodeSupport::Supported,
                Err(_) => DecodeSupport::Unavailable(DecodeUnavailable::NoDevice),
            };
        }
        // VP9/AV1 require a container-supplied `vpcC`/`av1C` config record at `open()` (see
        // `mediaway-decoder` apple/adr/0002) — a throwaway 64x64 probe with no `extra_data`
        // cannot exercise either, so this probe only ever reports H.264/HEVC support live; VP9/
        // AV1 report `NotImplemented` rather than a misleading `NoDevice`/`Supported` guess.
        if matches!(
            codec,
            mediaway_common::CodecKind::Vp9 | mediaway_common::CodecKind::Av1
        ) {
            return DecodeSupport::Unavailable(DecodeUnavailable::NotImplemented);
        }
        let cfg = VideoDecoderConfig {
            codec,
            output: mediaway_decoder::VideoOutputPreference::CpuFramesOk,
            ..VideoDecoderConfig::h264(64, 64, mediaway_common::Rational::new(1, 30))
        };
        match AppleVideoDecoder::open(&cfg) {
            Ok(_) => DecodeSupport::Supported,
            Err(DecodeError::Unsupported) => {
                DecodeSupport::Unavailable(DecodeUnavailable::NotImplemented)
            }
            Err(_) => DecodeSupport::Unavailable(DecodeUnavailable::NoDevice),
        }
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "ios")))]
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

        #[cfg(target_os = "macos")]
        {
            use mediaway_device::apple::AppleScreenCapture;
            let cap = AppleScreenCapture::open(config)?;
            Ok(Box::new(cap))
        }

        #[cfg(target_os = "ios")]
        {
            // ReplayKit's in-app screen capture (`AppleScreenCapture`) takes no per-request
            // config — it always captures the whole app (video + app audio + mic audio); see
            // `mediaway-device` adr/apple/0004.
            use mediaway_device::apple::AppleScreenCapture;
            let _ = config;
            let cap = AppleScreenCapture::open()?;
            Ok(Box::new(cap))
        }

        #[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "ios")))]
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

        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            use mediaway_device::apple::AppleMicrophoneCapture;
            let cap = AppleMicrophoneCapture::open(config)?;
            Ok(Box::new(cap))
        }

        #[cfg(not(any(windows, target_os = "macos", target_os = "ios")))]
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

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        mediaway_device::apple::support(kind)
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "ios")))]
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

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        mediaway_device::apple::request_permission(kind)
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "ios")))]
    {
        let _ = kind;
        Ok(PermissionState::NotSupported)
    }
}
