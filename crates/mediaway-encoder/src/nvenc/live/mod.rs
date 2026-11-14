//! NVENC **live** H.264 encode: low-latency screen/window streaming from D3D11 textures.
//!
//! Distinct from [`super::NvencVideoEncoder`] (CPU NV12 upload, fixed `HighQuality` tuning,
//! 30-frame GOP): [`NvencLiveEncoder`] is tuned for an interactive stream sent to many viewers
//! over lossy relays.
//!
//! - **Input: caller D3D11 textures, Zero-Copy.** BGRA (`DXGI_FORMAT_B8G8R8A8_*`, what Windows
//!   Graphics Capture delivers), RGBA or NV12 textures on the device given at
//!   [`NvencLiveConfig::device`] are registered with NVENC once (cached, see
//!   [`NvencLiveEncoder::encode_d3d11`]) and mapped per frame. No CPU readback and no copy:
//!   NVENC's own hardware colour conversion turns RGB into YUV 4:2:0 (BT.709 limited range,
//!   signalled in the SPS VUI).
//! - **Rate control:** CBR with a VBV buffer of one frame by default
//!   ([`NvencLiveConfig::vbv_buffer_bits`]), no B-frames, no lookahead, zero reorder delay, so
//!   every [`push_frame`](crate::VideoEncoder::push_frame) yields exactly that frame's packet.
//! - **Keyframes on demand:** the GOP is infinite; the stream starts with an IDR and the next
//!   one comes only from [`NvencLiveEncoder::request_keyframe`] (e.g. a viewer joined or lost
//!   packets) or [`NvencLiveEncoder::resize`]. SPS/PPS precede every IDR.
//! - **Runtime control:** [`NvencLiveEncoder::set_bitrate`] (`nvEncReconfigureEncoder`, no new
//!   IDR) and optional periodic intra refresh ([`NvencLiveConfig::intra_refresh`]).
//! - **Output:** H.264 High profile, Annex-B (`00 00 00 01` start codes), one access unit per
//!   [`Packet`](mediaway_common::Packet), `is_keyframe` set on IDR access units.
//!
//! Bindings: a hand-written subset of the NVENC SDK 13.0 API ([`ffi`]); requires an NVIDIA
//! driver whose `nvEncodeAPI64.dll` supports API 13.0 (R570 or newer). Policy and measured
//! latencies: `adr/nvenc/0002-live-h264-session.md`.

use crate::EncodeError;
use mediaway_common::{GpuDeviceHandle, Rational};

#[cfg(windows)]
mod ffi;
#[cfg(windows)]
mod session;

/// NVENC preset, `P1` (fastest, lowest quality) .. `P7` (slowest, best quality). Encode time
/// per frame grows with the preset; see the crate docs' measured numbers before raising it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NvencPreset {
    /// Fastest.
    P1,
    /// Faster.
    P2,
    /// Fast.
    P3,
    /// Medium (default).
    #[default]
    P4,
    /// Slow.
    P5,
    /// Slower.
    P6,
    /// Slowest.
    P7,
}

impl NvencPreset {
    #[cfg(windows)]
    const fn index(self) -> usize {
        match self {
            Self::P1 => 0,
            Self::P2 => 1,
            Self::P3 => 2,
            Self::P4 => 3,
            Self::P5 => 4,
            Self::P6 => 5,
            Self::P7 => 6,
        }
    }
}

/// NVENC tuning for live use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NvencLatency {
    /// `NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY` (default): the encoder's lowest-latency tuning.
    #[default]
    UltraLow,
    /// `NV_ENC_TUNING_INFO_LOW_LATENCY`: slightly better quality, still no reordering.
    Low,
}

/// Pixel format of the D3D11 textures passed to [`NvencLiveEncoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NvencLiveInput {
    /// `DXGI_FORMAT_B8G8R8A8_UNORM` / `_UNORM_SRGB` / `DXGI_FORMAT_B8G8R8X8_UNORM` (Windows
    /// Graphics Capture's format). Alpha is ignored.
    Bgra,
    /// `DXGI_FORMAT_R8G8B8A8_UNORM` / `_UNORM_SRGB`. Alpha is ignored.
    Rgba,
    /// `DXGI_FORMAT_NV12` (already 4:2:0; no conversion).
    Nv12,
}

/// Periodic intra refresh instead of large periodic IDRs.
///
/// A band of intra macroblocks sweeps the picture so that a decoder that lost references is
/// clean again after `duration_frames` frames, while the bitrate stays flat.
///
/// Recovery-point SEI is written so decoders can tell where a refresh starts. A decoder that
/// has *nothing* (a new viewer) still needs an IDR: call
/// [`NvencLiveEncoder::request_keyframe`] for that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntraRefresh {
    /// Frames between the starts of two refresh waves (`intraRefreshPeriod`).
    pub period_frames: u32,
    /// Frames one wave takes to cover the picture (`intraRefreshCnt`), `<= period_frames`.
    pub duration_frames: u32,
}

/// Configuration for [`NvencLiveEncoder::open`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NvencLiveConfig {
    /// The D3D11 device that owns every texture passed to the encoder
    /// ([`GpuDeviceHandle::DirectX11`]; anything else is [`EncodeError::Unsupported`]). The
    /// encoder takes its own reference. If other threads use the same device (capture,
    /// rendering), make it multithread-protected (`ID3D10Multithread::SetMultithreadProtected`;
    /// `mediaway-device`'s Windows Graphics Capture already does).
    pub device: GpuDeviceHandle,
    /// Encoded width in pixels (even).
    pub width: u32,
    /// Encoded height in pixels (even).
    pub height: u32,
    /// Largest width [`NvencLiveEncoder::resize`] may switch to; `0` = [`Self::width`].
    pub max_width: u32,
    /// Largest height [`NvencLiveEncoder::resize`] may switch to; `0` = [`Self::height`].
    pub max_height: u32,
    /// Frame rate numerator (frames per second = `fps_num / fps_den`). Rate control budgets
    /// bits per frame from it, so it should match the real submit rate.
    pub fps_num: u32,
    /// Frame rate denominator.
    pub fps_den: u32,
    /// Timebase of the `pts` / `duration` carried from input frames to packets (not
    /// interpreted by the encoder).
    pub time_base: Rational,
    /// CBR target in bits per second.
    pub bitrate_bps: u32,
    /// VBV (HRD) buffer size in bits; `0` = one frame (`bitrate_bps * fps_den / fps_num`), the
    /// usual choice for lowest latency.
    pub vbv_buffer_bits: u32,
    /// Texture pixel format.
    pub input: NvencLiveInput,
    /// Speed / quality trade-off.
    pub preset: NvencPreset,
    /// Latency tuning.
    pub latency: NvencLatency,
    /// Optional periodic intra refresh; `None` = only on-demand IDRs.
    pub intra_refresh: Option<IntraRefresh>,
}

impl NvencLiveConfig {
    /// A live config for `width` x `height` BGRA textures on `device` at `fps_num / fps_den`
    /// frames per second and `bitrate_bps`: `P4`, ultra-low latency, one-frame VBV, no intra
    /// refresh, microsecond timestamps. Adjust fields as needed.
    #[must_use]
    pub const fn new(
        device: GpuDeviceHandle,
        width: u32,
        height: u32,
        fps_num: u32,
        fps_den: u32,
        bitrate_bps: u32,
    ) -> Self {
        Self {
            device,
            width,
            height,
            max_width: 0,
            max_height: 0,
            fps_num,
            fps_den,
            time_base: Rational::new(1, 1_000_000),
            bitrate_bps,
            vbv_buffer_bits: 0,
            input: NvencLiveInput::Bgra,
            preset: NvencPreset::P4,
            latency: NvencLatency::UltraLow,
            intra_refresh: None,
        }
    }
}

/// Live NVENC H.264 encoder on a caller D3D11 device (see the module docs).
///
/// Synchronous: [`push_frame`](crate::VideoEncoder::push_frame) /
/// [`Self::encode_d3d11`] return once the hardware finished the frame, and the packet is then
/// ready in [`poll_packet`](crate::VideoEncoder::poll_packet). Run it on its own thread if the
/// capture thread must not wait for the encode. `Send`, not `Sync`.
pub struct NvencLiveEncoder {
    #[cfg(windows)]
    inner: session::LiveSession,
    #[cfg(not(windows))]
    _priv: (),
}

impl NvencLiveEncoder {
    /// Open a live H.264 session.
    ///
    /// # Errors
    ///
    /// - [`EncodeError::InvalidInput`]: zero / odd dimensions, `max_*` below the size, zero
    ///   frame rate or bitrate, bad [`IntraRefresh`] values.
    /// - [`EncodeError::Unsupported`]: not Windows, a non-D3D11 [`NvencLiveConfig::device`],
    ///   or the GPU lacks H.264 encode / intra refresh / the requested size.
    /// - [`EncodeError::Backend`]: no NVIDIA driver (`nvEncodeAPI64.dll` missing or older than
    ///   API 13.0), the device is not an NVIDIA GPU, or an NVENC call failed.
    #[cfg(windows)]
    pub fn open(config: &NvencLiveConfig) -> Result<Self, EncodeError> {
        Ok(Self {
            inner: session::LiveSession::open(config)?,
        })
    }

    /// Not Windows: always [`EncodeError::Unsupported`].
    ///
    /// # Errors
    ///
    /// Always.
    #[cfg(not(windows))]
    pub const fn open(_config: &NvencLiveConfig) -> Result<Self, EncodeError> {
        Err(EncodeError::Unsupported)
    }

    /// Encode one D3D11 texture (`ID3D11Texture2D*` on the session's device) as the next
    /// frame. The low-level form of [`push_frame`](crate::VideoEncoder::push_frame) with a
    /// [`GpuBufferHandle::DirectX11`](mediaway_common::GpuBufferHandle) frame.
    ///
    /// The texture must be at least the current encode size and in the configured
    /// [`NvencLiveInput`] format; only its top-left `width` x `height` region is encoded.
    /// `subresource` must be `0` (NVENC registers whole D3D11 textures).
    ///
    /// **Registration cache:** the first time a texture is seen it is registered with NVENC
    /// (`nvEncRegisterResource`, costly) and the encoder keeps a COM reference to it, so the
    /// address cannot be reused by another texture. Up to 8 textures are kept (a capture frame
    /// pool cycles through 2-3); the oldest is unregistered and released when a ninth
    /// arrives, and all are released on [`Self::resize`] and drop. Per frame the texture is
    /// only mapped / unmapped.
    ///
    /// The texture must not be written until this returns (the encode has finished by then).
    ///
    /// # Errors
    ///
    /// [`EncodeError::Closed`] after [`flush`](crate::VideoEncoder::flush);
    /// [`EncodeError::InvalidInput`] for a null / too small / wrong-format texture;
    /// [`EncodeError::Unsupported`] for `subresource != 0`; [`EncodeError::Backend`] when
    /// NVENC fails (e.g. the texture is on another device).
    #[cfg(windows)]
    pub fn encode_d3d11(
        &mut self,
        texture: mediaway_common::NativeHandle,
        subresource: u32,
        pts: i64,
        duration: u64,
    ) -> Result<(), EncodeError> {
        self.inner.encode_d3d11(texture, subresource, pts, duration)
    }

    /// Make the next encoded frame an IDR (with SPS/PPS in front), e.g. when a viewer joins
    /// or reports loss. Cheap; several requests before the next frame give one IDR.
    pub const fn request_keyframe(&mut self) {
        #[cfg(windows)]
        self.inner.request_keyframe();
    }

    /// Change the CBR target from the next frame on, without an IDR
    /// (`nvEncReconfigureEncoder`). The VBV buffer keeps its size in frames (one frame unless
    /// [`NvencLiveConfig::vbv_buffer_bits`] said otherwise).
    ///
    /// # Errors
    ///
    /// [`EncodeError::InvalidInput`] for `0`; [`EncodeError::Unsupported`] if the GPU cannot
    /// change bitrate dynamically; [`EncodeError::Backend`] if NVENC rejects it.
    pub fn set_bitrate(&mut self, bitrate_bps: u32) -> Result<(), EncodeError> {
        #[cfg(windows)]
        return self.inner.set_bitrate(bitrate_bps);
        #[cfg(not(windows))]
        {
            let _ = bitrate_bps;
            Err(EncodeError::Unsupported)
        }
    }

    /// Current CBR target in bits per second.
    #[must_use]
    pub const fn bitrate(&self) -> u32 {
        #[cfg(windows)]
        return self.inner.bitrate();
        #[cfg(not(windows))]
        0
    }

    /// Switch the encoded size (e.g. the shared window was resized) without reopening:
    /// `nvEncReconfigureEncoder` with an encoder reset; the next frame is an IDR with the new
    /// SPS. Releases the registration cache.
    ///
    /// # Errors
    ///
    /// [`EncodeError::InvalidInput`] for zero / odd sizes or sizes above
    /// [`NvencLiveConfig::max_width`] / [`NvencLiveConfig::max_height`];
    /// [`EncodeError::Backend`] if NVENC rejects it.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), EncodeError> {
        #[cfg(windows)]
        return self.inner.resize(width, height);
        #[cfg(not(windows))]
        {
            let _ = (width, height);
            Err(EncodeError::Unsupported)
        }
    }
}

#[cfg(windows)]
impl crate::VideoEncoder for NvencLiveEncoder {
    fn stream_info(&self) -> &mediaway_common::StreamInfo {
        self.inner.stream_info()
    }

    /// Accepts [`VideoFrameStorage::Gpu`](mediaway_common::VideoFrameStorage::Gpu) frames with
    /// a [`GpuBufferHandle::DirectX11`](mediaway_common::GpuBufferHandle) texture (see
    /// [`NvencLiveEncoder::encode_d3d11`]); CPU frames are [`EncodeError::Unsupported`] (use
    /// [`super::NvencVideoEncoder`] for CPU NV12).
    fn push_frame(&mut self, frame: &mediaway_common::VideoFrame) -> Result<(), EncodeError> {
        use mediaway_common::{GpuBufferHandle, VideoFrameStorage};
        let VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
            texture,
            subresource,
        }) = &frame.storage
        else {
            return Err(EncodeError::Unsupported);
        };
        self.inner
            .encode_d3d11(*texture, *subresource, frame.pts, frame.duration)
    }

    fn poll_packet(&mut self) -> Result<Option<mediaway_common::Packet>, EncodeError> {
        Ok(self.inner.poll_packet())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        self.inner.flush()
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
