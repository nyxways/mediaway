//! C ABI struct/enum types mirroring `mediaway_common`/`mediaway_encoder` types.
//!
//! Field layouts and ownership are decided in `adr/0001-auto-encode-c-abi.md` §5/§6.
//!
//! `MediawayRational`/`MediawayPipelineCodecKind` are re-exported from
//! `mediaway-common-ffi` rather than defined locally
//! (`docs/adr/0015-common-ffi-unification.md`) — confirmed field-identical to
//! `mediaway-container-ffi`'s independent copy before this migration. The C-facing
//! type name (`mediaway_rational_t`/`mediaway_pipeline_codec_kind_t`, this crate's
//! `include/mediaway/pipeline.h`) is unaffected by where the Rust definition lives.

use mediaway_common::PixelFormat;

/// Rational timebase (`num / den`, seconds) — see `mediaway-common-ffi::types::Rational`.
pub use crate::common::types::Rational as MediawayRational;

/// Codec kind — see `mediaway-common-ffi::types::CodecKind`.
///
/// Passing a non-video codec (`Aac`..`RawAudio`) to
/// [`crate::pipeline::mediaway_auto_video_encode_config_new`] is a runtime
/// [`crate::pipeline::MediawayPipelineStatus::InvalidArgument`], not a compile-time
/// restriction — [`mediaway_encoder::auto::AutoVideoEncodeConfig`]'s `codec`
/// field really is the full codec-kind range.
pub use crate::common::types::CodecKind as MediawayPipelineCodecKind;

/// GPU device kind — see `mediaway-common-ffi::gpu::GpuDeviceKind`.
pub use crate::common::gpu::GpuDeviceKind as MediawayGpuDeviceKind;

/// Native GPU device handle — see `mediaway-common-ffi::gpu::GpuDeviceHandle`.
///
/// Same input/borrow contract `mediaway-device-ffi` already documents for this type
/// (`adr/0003-gpu-handle-c-abi.md` §2): caller-owned, must outlive the call it's
/// passed to.
pub use crate::common::gpu::GpuDeviceHandle as MediawayGpuDeviceHandle;

/// GPU buffer/texture kind — see `mediaway-common-ffi::gpu::GpuBufferKind`.
pub use crate::common::gpu::GpuBufferKind as MediawayGpuBufferKind;

/// Native GPU buffer/texture handle — see `mediaway-common-ffi::gpu::GpuBufferHandle`.
///
/// Unlike `mediaway-device-ffi`'s use of this type (output-only, borrowed from a
/// capture session), here it is **caller-supplied input**
/// (`adr/0002-gpu-frame-input-c-abi.md` §2): the caller owns the underlying texture and
/// must keep it alive for at least the duration of
/// [`crate::pipeline::mediaway_encode_session_write_frame`].
pub use crate::common::gpu::GpuBufferHandle as MediawayGpuBufferHandle;

/// Pixel layout — mirrors [`PixelFormat`]'s 5 variants.
///
/// First definition of this enum in the workspace's C headers — no mirroring
/// precedent to reconcile against. Only `Nv12`/`Bgra8` are exercised by the
/// current Windows CPU-upload backend today; this is an existing Rust-level
/// limitation, not a new FFI one — passing another variant surfaces as
/// [`crate::pipeline::MediawayPipelineStatus::Unsupported`] from the wrapped encoder.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayPixelFormat {
    /// 8-bit NV12 (YUV 4:2:0 semi-planar) — common HW encode input.
    Nv12 = 0,
    /// 8-bit I420 / YUV420P.
    I420 = 1,
    /// 8-bit BGRA packed.
    Bgra8 = 2,
    /// 8-bit RGBA packed.
    Rgba8 = 3,
    /// 8-bit YUYV / YUY2 packed (YUV 4:2:2).
    Yuyv = 4,
}

impl From<MediawayPixelFormat> for PixelFormat {
    fn from(format: MediawayPixelFormat) -> Self {
        match format {
            MediawayPixelFormat::Nv12 => Self::Nv12,
            MediawayPixelFormat::I420 => Self::I420,
            MediawayPixelFormat::Bgra8 => Self::Bgra8,
            MediawayPixelFormat::Rgba8 => Self::Rgba8,
            MediawayPixelFormat::Yuyv => Self::Yuyv,
        }
    }
}

impl From<PixelFormat> for MediawayPixelFormat {
    // `PixelFormat` is `#[non_exhaustive]`; all variants that exist today are matched
    // by name below. No "unknown" C variant exists to fall back to (unlike
    // `MediawayPipelineStatus::UnknownError`), so a future variant maps to the safest
    // default (NV12, the same default `AutoVideoEncodeConfig::new` uses) — which
    // happens to equal the `Nv12` arm's own body; that overlap is intentional, not a
    // copy-paste bug.
    #[allow(clippy::match_same_arms)]
    fn from(format: PixelFormat) -> Self {
        match format {
            PixelFormat::Nv12 => Self::Nv12,
            PixelFormat::I420 => Self::I420,
            PixelFormat::Bgra8 => Self::Bgra8,
            PixelFormat::Rgba8 => Self::Rgba8,
            PixelFormat::Yuyv => Self::Yuyv,
            _ => Self::Nv12,
        }
    }
}

/// Config for [`crate::pipeline::mediaway_auto_encoder_open`] — plain value struct, no
/// handle, no heap allocation, no free function.
///
/// `max_path_class`/`backend` stay deferred (always `CpuUpload`/`Auto`
/// underneath) — `AutoVideoEncodeConfig::max_path_class`'s own default
/// (`CpuUpload`) already permits `ZeroCopy`/`GpuCopy`, so `gpu_device` alone is
/// enough to opt into the GPU path. See `adr/0001-auto-encode-c-abi.md` §1 and
/// `adr/0002-gpu-frame-input-c-abi.md` §1.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediawayAutoVideoEncodeConfig {
    /// Output codec.
    pub codec: MediawayPipelineCodecKind,
    /// Encoded width (caller supplies; no baked-in resolution preset).
    pub width: u32,
    /// Encoded height.
    pub height: u32,
    /// Timestamp timebase.
    pub time_base: MediawayRational,
    /// Target bitrate in bits per second (`0` = backend default).
    pub bitrate_bps: u32,
    /// Hint when a CPU upload path is selected.
    pub pixel_format: MediawayPixelFormat,
    /// GPU device to open the encoder against. `MediawayGpuDeviceKind::None`
    /// (the zero value) keeps the existing CPU-only behavior; a real device
    /// opts into the Zero-Copy/GPU-copy input path (`adr/0002-gpu-frame-input-c-abi.md`
    /// §1).
    pub gpu_device: MediawayGpuDeviceHandle,
}

/// Which of [`MediawayVideoFrame`]'s two storage fields is valid.
///
/// `adr/0002-gpu-frame-input-c-abi.md` §2 — mirrors `mediaway-device-ffi`'s
/// `MediawayVideoFrameStorageKind` idiom exactly (kind field decides which fields
/// matter, no C union, no second write function).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayVideoFrameStorageKind {
    /// `raw_bytes`/`raw_bytes_len` are valid; `gpu_buffer` is unused/zeroed.
    Cpu = 0,
    /// `gpu_buffer` is valid; `raw_bytes` is `NULL`, `raw_bytes_len` is `0`.
    Gpu = 1,
}

/// Input to [`crate::pipeline::mediaway_encode_session_write_frame`] — borrowed view,
/// valid for the call only.
///
/// Models both `VideoFrameStorage::Cpu { data }` and `VideoFrameStorage::Gpu(handle)`
/// (`adr/0002-gpu-frame-input-c-abi.md` §2) — `storage_kind` decides which of
/// `raw_bytes`/`gpu_buffer` the call reads. `gpu_buffer` is **borrowed**: it aliases
/// the caller's own GPU resource and must stay valid until the call returns; this
/// struct never frees it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayVideoFrame {
    /// Presentation timestamp in the stream timebase.
    pub pts: i64,
    /// Duration in timebase units (`0` if unknown).
    pub duration: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel layout.
    pub pixel_format: MediawayPixelFormat,
    /// Which of `raw_bytes`/`gpu_buffer` is valid.
    pub storage_kind: MediawayVideoFrameStorageKind,
    /// Borrowed plane bytes (CPU only), valid for the call only. Null iff
    /// `raw_bytes_len == 0` or `storage_kind == Gpu`.
    pub raw_bytes: *const u8,
    /// Length of `raw_bytes` in bytes (CPU only); `0` whenever `storage_kind == Gpu`.
    pub raw_bytes_len: usize,
    /// Borrowed GPU texture handle (GPU only); zeroed whenever `storage_kind == Cpu`.
    pub gpu_buffer: MediawayGpuBufferHandle,
}

// ── Audio encode (../../adr/pipeline/0003-auto-audio-encode-c-abi.md) ─────────────────────────

/// Audio PCM sample layout — mirrors `mediaway_common::SampleFormat`'s 3 variants.
///
/// First definition in this header; `mediaway-device-ffi` carries its own
/// independent copy (no shared header exists yet). Only `F32` is accepted by
/// the real Windows backend today.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawaySampleFormat {
    /// Signed 16-bit little-endian interleaved PCM.
    S16 = 0,
    /// Signed 32-bit little-endian interleaved PCM.
    S32 = 1,
    /// IEEE float32 interleaved PCM.
    F32 = 2,
}

/// Config for [`crate::pipeline::audio::mediaway_audio_encoder_open`] — plain value struct,
/// no handle, no heap allocation, no free function.
///
/// `codec` is `Aac` today; passing any other kind is a runtime
/// [`crate::pipeline::MediawayPipelineStatus::Unsupported`]. `sample_format` is `F32`
/// today (the format the real WASAPI capture backends deliver).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediawayAudioEncodeConfig {
    /// Output codec (AAC today).
    pub codec: MediawayPipelineCodecKind,
    /// Input sample rate (Hz). Must be non-zero.
    pub sample_rate: u32,
    /// Input channel count. Must be non-zero.
    pub channels: u16,
    /// Input PCM format (F32 today).
    pub sample_format: MediawaySampleFormat,
    /// Timestamp timebase for pushed PCM frames / polled packets.
    pub time_base: MediawayRational,
    /// Target bitrate in bits per second (`0` = backend default).
    pub bitrate_bps: u32,
}

/// Input to [`crate::pipeline::audio::mediaway_audio_encode_session_push_pcm`].
///
/// Borrowed view, valid for the call only (same ownership direction as
/// `MediawayVideoFrame`'s `raw_bytes`). The encoder copies synchronously.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayAudioFrameView {
    /// Presentation timestamp in the stream timebase.
    pub pts: i64,
    /// Duration in timebase units (`0` if unknown).
    pub duration: u64,
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// PCM sample format.
    pub sample_format: MediawaySampleFormat,
    /// Borrowed interleaved PCM bytes; null iff `data_len == 0`.
    pub data: *const u8,
    /// Length of `data` in bytes.
    pub data_len: usize,
}

/// Output of [`crate::pipeline::audio::mediaway_audio_encode_session_poll_packet`] — OWNED;
/// release with [`crate::pipeline::audio::mediaway_pipeline_ffi_packet_free`].
///
/// No `stream_id` field: the caller assigns the muxer track id when pushing
/// this packet into their own container muxer (the audio session does not mux).
#[repr(C)]
#[derive(Debug)]
pub struct MediawayAudioPacket {
    /// Presentation timestamp in the stream timebase.
    pub pts: i64,
    /// Decode timestamp in the stream timebase.
    pub dts: i64,
    /// Duration in timebase units.
    pub duration: u64,
    /// Whether this packet is a keyframe / random access point.
    pub is_keyframe: bool,
    /// Outside the active edit window; decoders may skip.
    pub is_discard: bool,
    /// Owned AAC bitstream bytes; `NULL` after
    /// [`crate::pipeline::audio::mediaway_pipeline_ffi_packet_free`].
    pub payload: *mut u8,
    /// Length of `payload` in bytes.
    pub payload_len: usize,
}

/// Output of [`crate::pipeline::audio::mediaway_audio_encode_session_stream_info`] — OWNED;
/// release with [`crate::pipeline::audio::mediaway_pipeline_ffi_stream_info_free`].
///
/// The `extra_data` (`AudioSpecificConfig` for AAC) is what a muxer's audio
/// track needs to be playable — the caller copies it into
/// `mediaway_audio_track_info_t` (container.h) when registering the track.
#[repr(C)]
#[derive(Debug)]
pub struct MediawayAudioStreamInfo {
    /// Output codec (AAC today).
    pub codec: MediawayPipelineCodecKind,
    /// Stream timebase.
    pub time_base: MediawayRational,
    /// Sample rate (Hz) — `0` when not yet known.
    pub sample_rate: u32,
    /// Channel count — `0` when not yet known.
    pub channels: u16,
    /// Owned codec config (e.g. `AudioSpecificConfig`); `NULL` after
    /// [`crate::pipeline::audio::mediaway_pipeline_ffi_stream_info_free`], and whenever
    /// `extra_data_len == 0`.
    pub extra_data: *mut u8,
    /// Length of `extra_data` in bytes.
    pub extra_data_len: usize,
}
