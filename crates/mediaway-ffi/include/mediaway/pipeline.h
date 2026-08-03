/*
 * pipeline.h — mediaway-pipeline-ffi: C ABI facade over Mediaway's auto video
 * encode -> fragmented MP4 convenience layer (mediaway-pipeline).
 *
 * Hand-written (not cbindgen-generated) — see adr/0001-auto-encode-c-abi.md §8.
 * Design rules: docs/spec/c-ffi.md (ADR-0004).
 *
 * Ownership summary (see adr/0001-auto-encode-c-abi.md §6, adr/0002-gpu-frame-input-c-abi.md
 * §2 for the full rationale):
 *   - mediaway_auto_video_encode_config_t is a plain value struct: no heap
 *     allocation, no free function, passed/returned by value.
 *   - mediaway_video_frame_t is tagged by storage_kind: MEDIAWAY_VIDEO_FRAME_STORAGE_CPU
 *     carries raw_bytes, a caller-owned borrow valid for the duration of the call
 *     only. MEDIAWAY_VIDEO_FRAME_STORAGE_GPU carries gpu_buffer, a BORROWED handle
 *     aliasing the caller's own GPU texture — this library never frees it and never
 *     calls Release() on it; see the GPU HAZARDS section below. This is the OPPOSITE
 *     ownership direction from mediaway-device-ffi's mediaway_device_video_frame_t
 *     (a borrowed OUTPUT there) — same struct name would have been a real
 *     field-shape collision, so these are distinctly-named types.
 *   - mediaway_encode_session_finish's out_data is a library-owned buffer; release
 *     it with mediaway_pipeline_ffi_buffer_free.
 *
 * Handle consumption (non-obvious, stated here and on each function below):
 *   - mediaway_encode_session_open takes ownership of `encoder` UNCONDITIONALLY —
 *     on success AND on failure — because EncodeSession::open takes its encoder by
 *     value in Rust. Do not call mediaway_auto_encoder_close on `encoder` after
 *     calling this function, regardless of the returned status (double-free).
 *   - mediaway_encode_session_finish takes ownership of `session` UNCONDITIONALLY
 *     for the same reason (EncodeSession::finish takes `self` by value). Do not
 *     call mediaway_encode_session_close on `session` afterward.
 *
 * v1 scope, revised: mediaway_auto_video_encode_config_t now also carries
 * gpu_device (adr/0002-gpu-frame-input-c-abi.md §1) — a real device opts the
 * session into the Zero-Copy/GPU-copy input path at open time.
 * backend/max_path_class stay deferred, always resolving to Auto/the Rust-level
 * default (CpuUpload, which already permits ZeroCopy/GpuCopy) underneath. See
 * adr/0001-auto-encode-c-abi.md §1.
 *
 * GPU HAZARDS (mediaway_video_frame_t with storage_kind == GPU only —
 * adr/0002-gpu-frame-input-c-abi.md §2, NOT hidden):
 *   - gpu_buffer's native_a (e.g. an ID3D11Texture2D*) is a NON-OWNING, BORROWED
 *     pointer — the CALLER retains the only owning reference. This library does
 *     NOT call Release() on it and does not extend its lifetime past the call.
 *   - Read window: the texture must remain valid and unmodified by the caller for
 *     the duration of the mediaway_encode_session_write_frame call only — the
 *     encoder backend reads (and may issue a GPU-side copy/upload from) it
 *     synchronously within that call. Once the call returns, the caller is free
 *     to reuse or release the texture.
 *   - ID3D11Device immediate-context concurrency: if the encoder backend issues
 *     immediate-context GPU commands against the SAME ID3D11Device passed as
 *     gpu_device, and the caller's own code also issues immediate-context
 *     commands on that device concurrently from another thread, either enable
 *     ID3D11Multithread::SetMultithreadProtected(TRUE) on the device before
 *     passing it in, or confine the caller's own immediate-context use to avoid
 *     overlapping write_frame calls — the standard "ID3D11 immediate context is
 *     not safe for concurrent multi-thread submission" hazard.
 *
 * Thread safety: every handle (mediaway_auto_encoder_t*, mediaway_encode_session_t*)
 * is thread-confined by convention, not internally synchronized. A handle may be
 * moved to another thread, but calling two functions on the SAME handle
 * concurrently from different threads without external synchronization is a data
 * race (undefined behavior), not merely wrong output.
 *
 * This is a distinct, independently-numbered status enum and independently-named
 * buffer-free function from mediaway-container-ffi's <mediaway/container.h> — see
 * adr/0001-auto-encode-c-abi.md §2 and § Buffer-free naming for why the two are
 * not unified yet.
 */

#ifndef MEDIAWAY_PIPELINE_H
#define MEDIAWAY_PIPELINE_H

#define MEDIAWAY_PIPELINE_FFI_ABI_VERSION 2 /* bump on any breaking change; pre-1.0, no stability promise */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Opaque handles ──────────────────────────────────────────────────────────────── */

/* No member list: layout is private to the Rust implementation and may change without
 * notice pre-1.0. Always access through the functions below. */
typedef struct mediaway_auto_encoder mediaway_auto_encoder_t;
typedef struct mediaway_encode_session mediaway_encode_session_t;
typedef struct mediaway_audio_encode_session mediaway_audio_encode_session_t; /* adr/0003 — the session IS the encoder; no intermediate handle */

/* ── Status codes ────────────────────────────────────────────────────────────────── */

typedef enum mediaway_pipeline_status {
    MEDIAWAY_PIPELINE_STATUS_OK                      = 0,
    MEDIAWAY_PIPELINE_STATUS_INVALID_ARGUMENT        = 1,  /* null pointer, mismatched ptr/len */
    MEDIAWAY_PIPELINE_STATUS_HANDLE_POISONED         = 2,  /* a previous call on this handle panicked */
    MEDIAWAY_PIPELINE_STATUS_NO_BACKEND              = 3,  /* no encode backend compiled in — expected/graceful */
    MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED             = 4,  /* bad codec/pixel-format/geometry — context-dependent, not always graceful */
    MEDIAWAY_PIPELINE_STATUS_INVALID_INPUT           = 5,  /* bad dimensions, rates, or frame metadata */
    MEDIAWAY_PIPELINE_STATUS_ENCODER_BACKEND_FAILURE = 6,  /* OS/API failure inside the encoder backend */
    MEDIAWAY_PIPELINE_STATUS_ENCODER_CLOSED          = 7,  /* session already finished or not open */
    MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_TRACK       = 8,  /* muxer rejected the encoder's stream info */
    MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_PACKET      = 9,  /* packet does not match the registered track */
    MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_DATA        = 10, /* malformed container data */
    MEDIAWAY_PIPELINE_STATUS_UNKNOWN_ERROR           = 11, /* reserved for a future error variant */
    MEDIAWAY_PIPELINE_STATUS_INTERNAL_PANIC          = 12, /* this call caught a Rust panic; the handle is now poisoned */
} mediaway_pipeline_status_t;

/* ── Shared value types ──────────────────────────────────────────────────────────── */

/* Identical shape to mediaway-container-ffi's mediaway_rational_t — reused, not
 * re-derived, but a distinct typedef name (no shared header exists yet). */
#ifndef MEDIAWAY_RATIONAL_T_DEFINED
#define MEDIAWAY_RATIONAL_T_DEFINED
typedef struct mediaway_rational {
    uint64_t num;
    uint32_t den; /* must be non-zero */
} mediaway_rational_t;
#endif

/* Distinct type name from mediaway_codec_kind_t (mediaway-container-ffi), but numeric
 * values are deliberately mirrored 1:1 — both wrap the same shared Rust type
 * (mediaway_common::CodecKind) end-to-end. Passing a non-video codec (AAC..RAW_AUDIO)
 * to mediaway_auto_video_encode_config_new is a runtime
 * MEDIAWAY_PIPELINE_STATUS_INVALID_ARGUMENT, not a compile-time restriction. */
typedef enum mediaway_pipeline_codec_kind {
    MEDIAWAY_PIPELINE_CODEC_H264      = 0,
    MEDIAWAY_PIPELINE_CODEC_HEVC      = 1,
    MEDIAWAY_PIPELINE_CODEC_AV1       = 2,
    MEDIAWAY_PIPELINE_CODEC_VP9       = 3,
    MEDIAWAY_PIPELINE_CODEC_AAC       = 4,
    MEDIAWAY_PIPELINE_CODEC_OPUS      = 5,
    MEDIAWAY_PIPELINE_CODEC_MP3       = 6,
    MEDIAWAY_PIPELINE_CODEC_VORBIS    = 7,
    MEDIAWAY_PIPELINE_CODEC_WEBVTT    = 8,
    MEDIAWAY_PIPELINE_CODEC_TX3G      = 9,
    MEDIAWAY_PIPELINE_CODEC_RAW_VIDEO = 10,
    MEDIAWAY_PIPELINE_CODEC_RAW_AUDIO = 11,
} mediaway_pipeline_codec_kind_t; /* pre-1.0 — values may be renumbered */

/* First definition of this enum in the workspace's C headers — no mirroring precedent
 * to reconcile against. Only NV12/BGRA8 are exercised by the current Windows
 * CPU-upload backend today (an existing Rust-level limitation, not a new FFI one);
 * passing another variant surfaces as MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED. */
#ifndef MEDIAWAY_PIXEL_FORMAT_T_DEFINED
#define MEDIAWAY_PIXEL_FORMAT_T_DEFINED
typedef enum mediaway_pixel_format {
    MEDIAWAY_PIXEL_FORMAT_NV12  = 0,
    MEDIAWAY_PIXEL_FORMAT_I420  = 1,
    MEDIAWAY_PIXEL_FORMAT_BGRA8 = 2,
    MEDIAWAY_PIXEL_FORMAT_RGBA8 = 3,
    MEDIAWAY_PIXEL_FORMAT_YUYV  = 4,
} mediaway_pixel_format_t;
#endif

/* Both mediaway_gpu_device_handle_t/mediaway_gpu_buffer_handle_t wrap Rust
 * data-carrying enums (mediaway_common::GpuDeviceHandle/GpuBufferHandle) — first
 * defined textually in mediaway-device-ffi's <mediaway/device.h>; declared again
 * here for the same known duplicate-typedef acceptance mediaway_rational_t/
 * mediaway_pixel_format_t already carry (both crates' Rust sides share one
 * definition in mediaway-common-ffi::gpu). Flat struct + discriminant, not a C
 * union — matches this header's existing mediaway_video_frame_t convention. */

#ifndef MEDIAWAY_GPU_DEVICE_KIND_T_DEFINED
#define MEDIAWAY_GPU_DEVICE_KIND_T_DEFINED
typedef enum mediaway_gpu_device_kind {
    MEDIAWAY_GPU_DEVICE_NONE      = 0, /* no device supplied — the safe zero-init default */
    MEDIAWAY_GPU_DEVICE_DIRECTX11 = 1,
    MEDIAWAY_GPU_DEVICE_DIRECTX12 = 2,
    MEDIAWAY_GPU_DEVICE_VULKAN    = 3,
    MEDIAWAY_GPU_DEVICE_METAL     = 4,
    MEDIAWAY_GPU_DEVICE_WEBGPU    = 5,
} mediaway_gpu_device_kind_t;
#endif

#ifndef MEDIAWAY_GPU_DEVICE_HANDLE_T_DEFINED
#define MEDIAWAY_GPU_DEVICE_HANDLE_T_DEFINED
/* Caller-supplied GPU device handle (mediaway_auto_video_encode_config_t.gpu_device).
 * The caller owns the underlying device and must keep it alive for at least the
 * duration of mediaway_auto_encoder_open. Plain value; no free function. */
typedef struct mediaway_gpu_device_handle {
    mediaway_gpu_device_kind_t kind;
    uintptr_t native;           /* ID3D11Device* / ID3D12Device* / VkDevice / MTLDevice bits; 0 for NONE/WebGpu */
    uint64_t webgpu_device_id;  /* WebGpu only; 0 otherwise */
} mediaway_gpu_device_handle_t;
#endif

#ifndef MEDIAWAY_GPU_BUFFER_KIND_T_DEFINED
#define MEDIAWAY_GPU_BUFFER_KIND_T_DEFINED
typedef enum mediaway_gpu_buffer_kind {
    MEDIAWAY_GPU_BUFFER_DIRECTX11       = 0, /* native_a = texture, subresource meaningful */
    MEDIAWAY_GPU_BUFFER_DIRECTX12       = 1, /* native_a = resource */
    MEDIAWAY_GPU_BUFFER_DIRECTX_SHARED  = 2, /* native_a = HANDLE */
    MEDIAWAY_GPU_BUFFER_METAL           = 3, /* native_a = buffer/IOSurface token */
    MEDIAWAY_GPU_BUFFER_ANDROID_SURFACE = 4, /* native_a = AHardwareBuffer* */
    MEDIAWAY_GPU_BUFFER_VULKAN          = 5, /* native_a = VkImage, native_b = memory cookie */
    MEDIAWAY_GPU_BUFFER_WEBGPU          = 6, /* webgpu_texture_id meaningful */
    MEDIAWAY_GPU_BUFFER_UNKNOWN         = 255, /* GpuBufferHandle is #[non_exhaustive]; decode-side catch-all only */
} mediaway_gpu_buffer_kind_t;
#endif

#ifndef MEDIAWAY_GPU_BUFFER_HANDLE_T_DEFINED
#define MEDIAWAY_GPU_BUFFER_HANDLE_T_DEFINED
/* mediaway_video_frame_t's GPU-storage input — BORROWED, not owned. See the file
 * header's GPU HAZARDS section for the full read-window / immediate-context
 * contract. Opposite ownership direction from mediaway-device-ffi's identically-shaped
 * type (that one is a borrowed OUTPUT; this one is a borrowed INPUT). */
typedef struct mediaway_gpu_buffer_handle {
    mediaway_gpu_buffer_kind_t kind;
    uintptr_t native_a;         /* texture / resource / handle / buffer / image, per kind */
    uintptr_t native_b;         /* Vulkan memory cookie only; 0 otherwise */
    uint32_t subresource;       /* DirectX11 only; 0 otherwise */
    uint64_t webgpu_texture_id; /* WebGpu only; 0 otherwise */
} mediaway_gpu_buffer_handle_t;
#endif

/* Plain value type; no free function. gpu_device (adr/0002-gpu-frame-input-c-abi.md
 * §1) opts the session into the Zero-Copy/GPU-copy input path at open time;
 * MEDIAWAY_GPU_DEVICE_NONE (the zero value) keeps the existing CPU-only behavior.
 * backend/max_path_class stay deferred (see the file header comment above). */
typedef struct mediaway_auto_video_encode_config {
    mediaway_pipeline_codec_kind_t codec;
    uint32_t width;
    uint32_t height;
    mediaway_rational_t time_base;
    uint32_t bitrate_bps;                 /* 0 = backend default */
    mediaway_pixel_format_t pixel_format;
    mediaway_gpu_device_handle_t gpu_device; /* MEDIAWAY_GPU_DEVICE_NONE for CPU-only */
} mediaway_auto_video_encode_config_t;

#ifndef MEDIAWAY_VIDEO_FRAME_STORAGE_KIND_T_DEFINED
#define MEDIAWAY_VIDEO_FRAME_STORAGE_KIND_T_DEFINED
typedef enum mediaway_video_frame_storage_kind {
    MEDIAWAY_VIDEO_FRAME_STORAGE_CPU = 0, /* raw_bytes/raw_bytes_len valid; gpu_buffer unused */
    MEDIAWAY_VIDEO_FRAME_STORAGE_GPU = 1, /* gpu_buffer valid; raw_bytes == NULL, raw_bytes_len == 0 */
} mediaway_video_frame_storage_kind_t;
#endif

/* Input to mediaway_encode_session_write_frame — borrowed view, valid for the call
 * only. storage_kind decides which of raw_bytes/gpu_buffer is read (see the file
 * header's ownership summary and GPU HAZARDS section). */
typedef struct mediaway_video_frame {
    int64_t pts;
    uint64_t duration;             /* 0 if unknown */
    uint32_t width;
    uint32_t height;
    mediaway_pixel_format_t pixel_format;
    mediaway_video_frame_storage_kind_t storage_kind;
    const uint8_t *raw_bytes;      /* CPU only; borrowed; NULL iff raw_bytes_len == 0 or storage_kind == GPU */
    size_t raw_bytes_len;          /* CPU only; 0 whenever storage_kind == GPU */
    mediaway_gpu_buffer_handle_t gpu_buffer; /* GPU only; BORROWED, zeroed whenever storage_kind == CPU */
} mediaway_video_frame_t;

/* ── ABI version ─────────────────────────────────────────────────────────────────── */

/* Runtime counterpart to MEDIAWAY_PIPELINE_FFI_ABI_VERSION, for consumers that load
 * this library dynamically and never compile against this header. */
uint32_t mediaway_pipeline_ffi_abi_version(void);

/* ── Config (plain value struct, no handle, no free) ─────────────────────────────── */

/* Explicit size and codec — resolution comes from the app, not a named preset.
 * Defaults bitrate_bps to 0 (backend default), pixel_format to NV12, and gpu_device
 * to MEDIAWAY_GPU_DEVICE_NONE (CPU-only). Callers wanting the GPU input path set
 * .gpu_device on the returned value afterward. */
mediaway_auto_video_encode_config_t mediaway_auto_video_encode_config_new(
    mediaway_pipeline_codec_kind_t codec, uint32_t width, uint32_t height,
    mediaway_rational_t time_base);

/* Sugar over mediaway_auto_video_encode_config_new(MEDIAWAY_PIPELINE_CODEC_H264, ...).
 * Kept for the aspirational example's ergonomics; the general form above also exists
 * since H.264 is not the only real codec the Windows auto backend resolves today. */
mediaway_auto_video_encode_config_t mediaway_auto_video_encode_config_h264(
    uint32_t width, uint32_t height, mediaway_rational_t time_base);

/* ── Auto encoder (intermediate handle) ──────────────────────────────────────────── */

/* Open the best available video encoder for `config` on the current platform.
 * MEDIAWAY_PIPELINE_STATUS_NO_BACKEND ("no backend compiled in here") is an expected,
 * graceful outcome, not a bug — check for it and exit cleanly rather than treating it
 * like a fatal error. *out_encoder is NULL on any non-OK status (a normal Err, or a
 * caught panic). */
mediaway_pipeline_status_t mediaway_auto_encoder_open(
    const mediaway_auto_video_encode_config_t *config,
    mediaway_auto_encoder_t **out_encoder);

/* Close and free an auto-encoder handle WITHOUT ever calling
 * mediaway_encode_session_open on it. Only for abandoning an opened encoder on an
 * early-abort path — do not call this after mediaway_encode_session_open has already
 * consumed `encoder` (double-free). Always safe to call, including with encoder == NULL. */
void mediaway_auto_encoder_close(mediaway_auto_encoder_t *encoder);

/* ── Encode session ───────────────────────────────────────────────────────────────── */

/* Register `encoder`'s stream as an MP4 track and begin streaming. Consumes
 * `encoder` UNCONDITIONALLY — success or failure — do not call
 * mediaway_auto_encoder_close on it afterward regardless of the returned status. */
mediaway_pipeline_status_t mediaway_encode_session_open(
    mediaway_auto_encoder_t *encoder, mediaway_encode_session_t **out_session);

/* Push one frame and drain any packets it produces into the muxer. `frame`'s
 * storage_kind decides which of raw_bytes/gpu_buffer is read; both are
 * caller-owned borrows, valid for the duration of this call only — see the file
 * header's ownership summary and GPU HAZARDS section for the GPU case. */
mediaway_pipeline_status_t mediaway_encode_session_write_frame(
    mediaway_encode_session_t *session, const mediaway_video_frame_t *frame);

/* Flush the encoder and muxer, returning the complete fMP4 byte stream. Consumes
 * `session` UNCONDITIONALLY — do not call mediaway_encode_session_close on it
 * afterward regardless of the returned status. Release *out_data with
 * mediaway_pipeline_ffi_buffer_free. */
mediaway_pipeline_status_t mediaway_encode_session_finish(
    mediaway_encode_session_t *session, uint8_t **out_data, size_t *out_len);

/* Abandon a session WITHOUT finishing it (no flush, no valid MP4 output) — added for
 * the same resource-cleanup symmetry as mediaway_auto_encoder_close. Always safe to
 * call, including on a poisoned handle or with session == NULL. Do not call this
 * after mediaway_encode_session_finish has already consumed `session`. */
void mediaway_encode_session_close(mediaway_encode_session_t *session);

/* ── Shared free ─────────────────────────────────────────────────────────────────── */

/* Free a buffer returned by mediaway_encode_session_finish. Distinctly named from
 * mediaway-container-ffi's mediaway_buffer_free — see the file header comment above. */
void mediaway_pipeline_ffi_buffer_free(uint8_t *data, size_t len);

/* ── Audio encode (adr/0003-auto-audio-encode-c-abi.md) ─────────────────────────── */

/* Identical shape/values to mediaway-device-ffi's mediaway_sample_format_t —
 * reused, not re-derived, but a distinct header (no shared header exists yet).
 * First definition in this header is not guaranteed — include order decides.
 * Only F32 is accepted by the real Windows backend today; the other variants
 * exist so the enum can be extended without a version bump. */
#ifndef MEDIAWAY_SAMPLE_FORMAT_T_DEFINED
#define MEDIAWAY_SAMPLE_FORMAT_T_DEFINED
typedef enum mediaway_sample_format {
    MEDIAWAY_SAMPLE_FORMAT_S16 = 0, /* signed 16-bit LE interleaved PCM */
    MEDIAWAY_SAMPLE_FORMAT_S32 = 1, /* signed 32-bit LE interleaved PCM */
    MEDIAWAY_SAMPLE_FORMAT_F32 = 2, /* IEEE float32 interleaved PCM */
} mediaway_sample_format_t;
#endif

/* Config for mediaway_audio_encoder_open — plain value struct, no handle, no heap
 * allocation, no free function. codec is AAC today (any other kind is a runtime
 * MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED); sample_format is F32 today. */
typedef struct mediaway_audio_encode_config {
    mediaway_pipeline_codec_kind_t codec;  /* output codec (AAC today) */
    uint32_t sample_rate;                  /* input sample rate in Hz, non-zero */
    uint16_t channels;                     /* input channel count, non-zero */
    mediaway_sample_format_t sample_format; /* input PCM format (F32 today) */
    mediaway_rational_t time_base;         /* timestamp timebase for pushed frames / polled packets */
    uint32_t bitrate_bps;                  /* target bitrate; 0 = backend default (128 kbps) */
} mediaway_audio_encode_config_t;

/* Build a stereo AAC config (F32 input, backend-default bitrate) — the only
 * combination the real backend accepts today, kept as the ergonomic sugar. The
 * general form exists as the struct itself; no general constructor is exported. */
mediaway_audio_encode_config_t mediaway_audio_encode_config_aac(
    uint32_t sample_rate, mediaway_rational_t time_base);

/* Input to mediaway_audio_encode_session_push_pcm — BORROWED view, valid for the
 * call only (same ownership direction as mediaway_video_frame_t's raw_bytes). */
typedef struct mediaway_audio_frame_view {
    int64_t pts;               /* presentation timestamp in the stream timebase */
    uint64_t duration;         /* duration in timebase units (0 if unknown) */
    uint32_t sample_rate;      /* sample rate in Hz */
    uint16_t channels;         /* channel count */
    mediaway_sample_format_t sample_format; /* PCM sample format (F32 today) */
    const uint8_t *data;       /* BORROWED interleaved PCM bytes; NULL iff data_len == 0 */
    size_t data_len;           /* length of data in bytes */
} mediaway_audio_frame_view_t;

/* Output of mediaway_audio_encode_session_poll_packet — OWNED; release with
 * mediaway_pipeline_ffi_packet_free. No stream_id: the caller assigns the muxer
 * track id when pushing into their own container muxer (the session does not mux). */
typedef struct mediaway_audio_packet {
    int64_t pts;           /* presentation timestamp in the stream timebase */
    int64_t dts;           /* decode timestamp in the stream timebase */
    uint64_t duration;     /* duration in timebase units */
    bool is_keyframe;      /* random access point */
    bool is_discard;       /* outside the active edit window; decoders may skip */
    uint8_t *payload;      /* OWNED AAC bitstream bytes; NULL after packet_free */
    size_t payload_len;    /* length of payload in bytes */
} mediaway_audio_packet_t;

/* Output of mediaway_audio_encode_session_stream_info — OWNED; release with
 * mediaway_pipeline_ffi_stream_info_free. extra_data is the AudioSpecificConfig
 * (raw, MP4 esds-ready) — copy it into mediaway_audio_track_info_t (container.h)
 * when registering the muxer track. */
typedef struct mediaway_audio_stream_info {
    mediaway_pipeline_codec_kind_t codec; /* output codec (AAC today) */
    mediaway_rational_t time_base;        /* stream timebase */
    uint32_t sample_rate;                 /* Hz; 0 when not yet known */
    uint16_t channels;                    /* channel count; 0 when not yet known */
    uint8_t *extra_data;                  /* OWNED codec config (AudioSpecificConfig); NULL after stream_info_free */
    size_t extra_data_len;                /* length of extra_data in bytes */
} mediaway_audio_stream_info_t;

/* Open the best available audio encoder for `config`. The returned handle IS the
 * encode session — no intermediate handle, so no consumption trap (adr/0003 §
 * Decision: audio has no internal muxer). MEDIAWAY_PIPELINE_STATUS_NO_BACKEND is
 * an expected graceful outcome on platforms without an audio backend; check for it
 * and exit cleanly. *out_session is NULL on any non-OK status (a normal Err, or a
 * caught panic). */
mediaway_pipeline_status_t mediaway_audio_encoder_open(
    const mediaway_audio_encode_config_t *config,
    mediaway_audio_encode_session_t **out_session);

/* Query the session's stream metadata: codec, timebase, sample rate, channel count,
 * and the codec config (extra_data — the AudioSpecificConfig an MP4 audio track
 * needs to be playable). The AudioSpecificConfig materializes only after the first
 * PCM frame is pushed, so call this after pushing, before muxing. OWNED output:
 * release *out_info with mediaway_pipeline_ffi_stream_info_free. */
mediaway_pipeline_status_t mediaway_audio_encode_session_stream_info(
    mediaway_audio_encode_session_t *session,
    mediaway_audio_stream_info_t *out_info);

/* Push one PCM buffer. `frame` is a BORROWED view valid for the duration of this
 * call only — the encoder copies synchronously (same cost class as the video
 * CPU-upload path). */
mediaway_pipeline_status_t mediaway_audio_encode_session_push_pcm(
    mediaway_audio_encode_session_t *session,
    const mediaway_audio_frame_view_t *frame);

/* Pull the next encoded packet, if one is ready. *out_has_packet == true: *out_packet
 * is an OWNED output — release it with mediaway_pipeline_ffi_packet_free. false is
 * a valid "nothing ready" result, not an error. */
mediaway_pipeline_status_t mediaway_audio_encode_session_poll_packet(
    mediaway_audio_encode_session_t *session,
    mediaway_audio_packet_t *out_packet, bool *out_has_packet);

/* Signal end-of-input; drain the remaining packets with
 * mediaway_audio_encode_session_poll_packet afterwards. */
mediaway_pipeline_status_t mediaway_audio_encode_session_flush(
    mediaway_audio_encode_session_t *session);

/* Close and free an audio encode session. Always safe to call, including with
 * session == NULL — this surface has no handle-consumption trap (adr/0003). */
void mediaway_audio_encode_session_close(mediaway_audio_encode_session_t *session);

/* Free a packet returned by mediaway_audio_encode_session_poll_packet. Nulls the
 * payload fields afterward, making a double-free a visible no-op. Always safe to
 * call, including with packet == NULL. */
void mediaway_pipeline_ffi_packet_free(mediaway_audio_packet_t *packet);

/* Free stream info returned by mediaway_audio_encode_session_stream_info. Nulls the
 * extra_data fields afterward, making a double-free a visible no-op. Always safe to
 * call, including with info == NULL. */
void mediaway_pipeline_ffi_stream_info_free(mediaway_audio_stream_info_t *info);

#ifdef __cplusplus
}
#endif

#endif /* MEDIAWAY_PIPELINE_H */
