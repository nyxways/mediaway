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
pub use mediaway_common_ffi::types::Rational as MediawayRational;

/// Codec kind — see `mediaway-common-ffi::types::CodecKind`.
///
/// Passing a non-video codec (`Aac`..`RawAudio`) to
/// [`crate::mediaway_auto_video_encode_config_new`] is a runtime
/// [`crate::MediawayPipelineStatus::InvalidArgument`], not a compile-time
/// restriction — [`mediaway_encoder::auto::AutoVideoEncodeConfig`]'s `codec`
/// field really is the full codec-kind range.
pub use mediaway_common_ffi::types::CodecKind as MediawayPipelineCodecKind;

/// GPU device kind — see `mediaway-common-ffi::gpu::GpuDeviceKind`.
pub use mediaway_common_ffi::gpu::GpuDeviceKind as MediawayGpuDeviceKind;

/// Native GPU device handle — see `mediaway-common-ffi::gpu::GpuDeviceHandle`.
///
/// Same input/borrow contract `mediaway-device-ffi` already documents for this type
/// (`adr/0003-gpu-handle-c-abi.md` §2): caller-owned, must outlive the call it's
/// passed to.
pub use mediaway_common_ffi::gpu::GpuDeviceHandle as MediawayGpuDeviceHandle;

/// GPU buffer/texture kind — see `mediaway-common-ffi::gpu::GpuBufferKind`.
pub use mediaway_common_ffi::gpu::GpuBufferKind as MediawayGpuBufferKind;

/// Native GPU buffer/texture handle — see `mediaway-common-ffi::gpu::GpuBufferHandle`.
///
/// Unlike `mediaway-device-ffi`'s use of this type (output-only, borrowed from a
/// capture session), here it is **caller-supplied input**
/// (`adr/0002-gpu-frame-input-c-abi.md` §2): the caller owns the underlying texture and
/// must keep it alive for at least the duration of
/// [`crate::mediaway_encode_session_write_frame`].
pub use mediaway_common_ffi::gpu::GpuBufferHandle as MediawayGpuBufferHandle;

/// Pixel layout — mirrors [`PixelFormat`]'s 5 variants.
///
/// First definition of this enum in the workspace's C headers — no mirroring
/// precedent to reconcile against. Only `Nv12`/`Bgra8` are exercised by the
/// current Windows CPU-upload backend today; this is an existing Rust-level
/// limitation, not a new FFI one — passing another variant surfaces as
/// [`crate::MediawayPipelineStatus::Unsupported`] from the wrapped encoder.
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

/// Config for [`crate::mediaway_auto_encoder_open`] — plain value struct, no
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

/// Input to [`crate::mediaway_encode_session_write_frame`] — borrowed view,
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
