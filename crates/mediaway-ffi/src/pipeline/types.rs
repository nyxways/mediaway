//! C ABI struct/enum types mirroring `mediaway_common`/`mediaway_encoder` types.
//!
//! Field layouts and ownership are decided in `adr/0001-auto-encode-c-abi.md` §5/§6.
//!
//! `MediawayRational`/`MediawayPipelineCodecKind`/`MediawayPixelFormat`/
//! `MediawaySampleFormat`/`MediawayVideoFrameStorageKind` are all re-exported from
//! `common::types` rather than defined locally — `Rational`/`CodecKind` since
//! ADR-0015, the other three moved there later after confirming they were
//! field-identical to `device::types`'s independent copies
//! (`adr/common/0001-shared-header-consolidation.md`). The C-facing type names
//! (this crate's `include/mediaway/pipeline.h`) are unaffected by where the Rust
//! definitions live.

/// Rational timebase (`num / den`, seconds) — see `mediaway-common-ffi::types::Rational`.
pub type MediawayRational = crate::common::types::Rational;

/// Codec kind — see `mediaway-common-ffi::types::CodecKind`.
///
/// Passing a non-video codec (`Aac`..`RawAudio`) to
/// [`crate::pipeline::mediaway_auto_video_encode_config_new`] is a runtime
/// [`crate::pipeline::MediawayPipelineStatus::InvalidArgument`], not a compile-time
/// restriction — [`mediaway_encoder::auto::AutoVideoEncodeConfig`]'s `codec`
/// field really is the full codec-kind range.
pub type MediawayPipelineCodecKind = crate::common::types::CodecKind;

/// GPU device kind — see `mediaway-common-ffi::gpu::GpuDeviceKind`.
pub type MediawayGpuDeviceKind = crate::common::gpu::GpuDeviceKind;

/// Native GPU device handle — see `mediaway-common-ffi::gpu::GpuDeviceHandle`.
///
/// Same input/borrow contract `mediaway-device-ffi` already documents for this type
/// (`adr/0003-gpu-handle-c-abi.md` §2): caller-owned, must outlive the call it's
/// passed to.
pub type MediawayGpuDeviceHandle = crate::common::gpu::GpuDeviceHandle;

/// GPU buffer/texture kind — see `mediaway-common-ffi::gpu::GpuBufferKind`.
pub type MediawayGpuBufferKind = crate::common::gpu::GpuBufferKind;

/// Native GPU buffer/texture handle — see `mediaway-common-ffi::gpu::GpuBufferHandle`.
///
/// Unlike `mediaway-device-ffi`'s use of this type (output-only, borrowed from a
/// capture session), here it is **caller-supplied input**
/// (`adr/0002-gpu-frame-input-c-abi.md` §2): the caller owns the underlying texture and
/// must keep it alive for at least the duration of
/// [`crate::pipeline::mediaway_encode_session_write_frame`].
pub type MediawayGpuBufferHandle = crate::common::gpu::GpuBufferHandle;

/// Pixel layout — mirrors `mediaway_common::PixelFormat`'s 5 variants.
///
/// Re-exported from `common::types` (moved there after confirming this module's
/// former independent copy was field-identical to `device::types`'s —
/// `adr/common/0001-shared-header-consolidation.md`). Only `Nv12`/`Bgra8` are
/// exercised by the current Windows CPU-upload backend today; this is an existing
/// Rust-level limitation, not a new FFI one — passing another variant surfaces as
/// [`crate::pipeline::MediawayPipelineStatus::Unsupported`] from the wrapped encoder.
pub type MediawayPixelFormat = crate::common::types::PixelFormat;

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
    /// Frames between forced IDR refreshes — mirrors
    /// [`mediaway_encoder::auto::AutoVideoEncodeConfig::gop_size`]. `1` = IDR-only
    /// (default, byte-identical to every existing caller).
    ///
    /// **Not yet honored by the auto-selected backend on any platform this crate
    /// opens** (`adr/0001-auto-encode-c-abi.md`'s 2026-08-07 addendum) — only the
    /// standalone `mediaway-encoder::vulkan` H.264/HEVC encoders read this today, and
    /// they are not reachable through [`crate::pipeline::mediaway_auto_encoder_open`]
    /// yet. Setting this is a forward-compatible no-op for now.
    pub gop_size: u32,
    /// Whether `rate_control_target_bitrate_bps`/`rate_control_vbv_buffer_size_bytes`
    /// are meaningful. `false` (default) keeps fixed-QP encoding, mirroring
    /// [`mediaway_encoder::auto::AutoVideoEncodeConfig::rate_control`]'s `None`. Same
    /// **not yet honored** caveat as [`Self::gop_size`] applies.
    pub rate_control_enabled: bool,
    /// Target bitrate in bits per second for CBR-style rate control. Only read when
    /// `rate_control_enabled` is `true`.
    pub rate_control_target_bitrate_bps: u32,
    /// VBV buffer size in bytes for CBR-style rate control. `0` lets the backend pick
    /// a driver-suggested default rather than this crate guessing one — mirrors
    /// `RateControlConfig::vbv_buffer_size_bytes`'s `None`. Only read when
    /// `rate_control_enabled` is `true`.
    pub rate_control_vbv_buffer_size_bytes: u32,
}

/// Which of [`MediawayVideoFrame`]'s two storage fields is valid.
///
/// Re-exported from `common::types` (`adr/0002-gpu-frame-input-c-abi.md` §2 —
/// mirrors `device::types`'s identical idiom: kind field decides which fields
/// matter, no C union, no second write function).
pub type MediawayVideoFrameStorageKind = crate::common::types::VideoFrameStorageKind;

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
/// Re-exported from `common::types` (moved there after confirming this module's
/// former independent copy was field-identical to `device::types`'s). Only `F32` is
/// accepted by the real Windows backend today.
pub type MediawaySampleFormat = crate::common::types::SampleFormat;

/// Config for [`crate::pipeline::audio::mediaway_audio_encoder_open`] — plain value struct,
/// no handle, no heap allocation, no free function.
///
/// `codec` is `Aac` or `Opus` today (`adr/pipeline/0006-audio-decode-c-abi.md` §
/// Encode side); passing any other kind is a runtime
/// [`crate::pipeline::MediawayPipelineStatus::Unsupported`]. `sample_format` is `F32`
/// today (the format the real WASAPI capture backends deliver, and the only format
/// `mediaway-sw`'s Opus backend accepts).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediawayAudioEncodeConfig {
    /// Output codec (AAC or Opus).
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

// ── Video decode (`../../adr/pipeline/0004-auto-decode-c-abi.md`) ────────────────

/// Config for [`crate::pipeline::mediaway_decode_session_open`] — no handle, no free.
///
/// Not fully POD: `extra_data` is a **borrowed** input, valid only for the duration
/// of the `mediaway_decode_session_open` call that reads it (same ownership shape
/// as `mediaway_video_track_info_t`'s `extra_data`).
///
/// GPU output stays deferred (always opens `CpuFramesOk` internally) — see
/// `adr/0004-auto-decode-c-abi.md` §1. `extra_data` (AVCC/SPS-PPS codec config) IS
/// required at open time — confirmed against the real Rust contract
/// (`VideoDecoderConfig.extra_data` is consumed inside `open()`, before any packet is
/// pushed), corrected from this ADR's own first draft which assumed the muxer-track
/// analogy (supply via the first packet) held here. It does not.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayAutoVideoDecodeConfig {
    /// Input codec.
    pub codec: MediawayPipelineCodecKind,
    /// Expected width (may be refined from the bitstream).
    pub width: u32,
    /// Expected height (may be refined from the bitstream).
    pub height: u32,
    /// Timestamp timebase for pushed packets / polled frames.
    pub time_base: MediawayRational,
    /// Preferred output pixel format when the backend converts.
    pub pixel_format: MediawayPixelFormat,
    /// Borrowed codec config (AVCC / SPS-PPS); `NULL` iff `extra_data_len == 0` (no
    /// codec config supplied). Valid for the `mediaway_decode_session_open` call only.
    pub extra_data: *const u8,
    /// Length of `extra_data` in bytes.
    pub extra_data_len: usize,
}

/// Input to [`crate::pipeline::mediaway_decode_session_push_packet`] — borrowed view,
/// valid for the call only.
///
/// New, pipeline-scoped type, not reused from `container.h`'s
/// `mediaway_packet_view_t` — see `adr/0004-auto-decode-c-abi.md` §4 for why.
/// `stream_id` is accepted but unused by decode (kept for call-site symmetry with a
/// container-demuxed packet the caller likely already has).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayDecodePacketView {
    /// Unused by decode; kept for call-site symmetry with a demuxed packet.
    pub stream_id: u32,
    /// Presentation timestamp in the stream timebase.
    pub pts: i64,
    /// Decode timestamp in the stream timebase.
    pub dts: i64,
    /// Duration in timebase units (`0` if unknown).
    pub duration: u64,
    /// Random access point.
    pub is_keyframe: bool,
    /// Outside the active edit window; decoders may skip.
    pub is_discard: bool,
    /// Borrowed compressed bitstream bytes; valid for the call only. `NULL` iff
    /// `payload_len == 0`.
    pub payload: *const u8,
    /// Length of `payload` in bytes.
    pub payload_len: usize,
}

/// Output of [`crate::pipeline::mediaway_decode_session_poll_frame`] — OWNED; release
/// with [`crate::pipeline::mediaway_decoded_video_frame_free`].
///
/// CPU-only (no `storage_kind`/`gpu_buffer` — GPU decode output is deferred, see
/// `adr/0004-auto-decode-c-abi.md` §1/§5). New, pipeline-scoped name: distinct
/// ownership direction from [`MediawayVideoFrame`] (borrowed encode *input* there vs.
/// owned decode *output* here) and distinct module from `device.h`'s frame types (no
/// codec/bitstream involved there).
#[repr(C)]
#[derive(Debug)]
pub struct MediawayDecodedVideoFrame {
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
    /// Owned plane bytes. `NULL` after
    /// [`crate::pipeline::mediaway_decoded_video_frame_free`].
    pub data: *mut u8,
    /// Length of `data` in bytes.
    pub data_len: usize,
}

// ── Audio decode (`../../adr/pipeline/0006-audio-decode-c-abi.md`) ───────────────

/// Config for [`crate::pipeline::mediaway_audio_decode_session_open`] — plain value
/// struct, no handle, no free function.
///
/// `codec` is `Opus` only today; passing any other kind is a runtime
/// [`crate::pipeline::MediawayPipelineStatus::Unsupported`]. Output PCM is always
/// `F32` (`unsafe-libopus`'s `opus_decode_float`) — no `sample_format` field to
/// mismatch, unlike [`MediawayAudioEncodeConfig`].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediawayAudioDecodeConfig {
    /// Input codec (Opus today).
    pub codec: MediawayPipelineCodecKind,
    /// Sample rate (Hz). Must be non-zero.
    pub sample_rate: u32,
    /// Channel count. Must be non-zero.
    pub channels: u16,
    /// Frame duration timebase — also the upper bound on one decoded frame's PCM
    /// sample count (`crate::opus::OpusDecoder::push_packet`'s costly-path doc).
    pub time_base: MediawayRational,
}

/// Output of [`crate::pipeline::mediaway_audio_decode_session_poll_frame`] — OWNED;
/// release with [`crate::pipeline::mediaway_decoded_audio_frame_free`].
///
/// New, pipeline-scoped name, distinct ownership direction from
/// [`MediawayAudioFrameView`] (borrowed encode *input* there vs. owned decode
/// *output* here) — same naming precedent as [`MediawayDecodedVideoFrame`] vs.
/// [`MediawayVideoFrame`].
#[repr(C)]
#[derive(Debug)]
pub struct MediawayDecodedAudioFrame {
    /// Presentation timestamp in the stream timebase.
    pub pts: i64,
    /// Duration in timebase units (`0` if unknown).
    pub duration: u64,
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// PCM sample format (always `F32` for Opus).
    pub sample_format: MediawaySampleFormat,
    /// Owned interleaved PCM bytes. `NULL` after
    /// [`crate::pipeline::mediaway_decoded_audio_frame_free`].
    pub data: *mut u8,
    /// Length of `data` in bytes.
    pub data_len: usize,
}
