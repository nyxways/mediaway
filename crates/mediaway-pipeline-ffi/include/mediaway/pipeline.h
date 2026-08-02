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

#define MEDIAWAY_PIPELINE_FFI_ABI_VERSION 1 /* bump on any breaking change; pre-1.0, no stability promise */

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
typedef struct mediaway_rational {
    uint64_t num;
    uint32_t den; /* must be non-zero */
} mediaway_rational_t;

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
typedef enum mediaway_pixel_format {
    MEDIAWAY_PIXEL_FORMAT_NV12  = 0,
    MEDIAWAY_PIXEL_FORMAT_I420  = 1,
    MEDIAWAY_PIXEL_FORMAT_BGRA8 = 2,
    MEDIAWAY_PIXEL_FORMAT_RGBA8 = 3,
    MEDIAWAY_PIXEL_FORMAT_YUYV  = 4,
} mediaway_pixel_format_t;

/* Both mediaway_gpu_device_handle_t/mediaway_gpu_buffer_handle_t wrap Rust
 * data-carrying enums (mediaway_common::GpuDeviceHandle/GpuBufferHandle) — first
 * defined textually in mediaway-device-ffi's <mediaway/device.h>; declared again
 * here for the same known duplicate-typedef acceptance mediaway_rational_t/
 * mediaway_pixel_format_t already carry (both crates' Rust sides share one
 * definition in mediaway-common-ffi::gpu). Flat struct + discriminant, not a C
 * union — matches this header's existing mediaway_video_frame_t convention. */

typedef enum mediaway_gpu_device_kind {
    MEDIAWAY_GPU_DEVICE_NONE      = 0, /* no device supplied — the safe zero-init default */
    MEDIAWAY_GPU_DEVICE_DIRECTX11 = 1,
    MEDIAWAY_GPU_DEVICE_DIRECTX12 = 2,
    MEDIAWAY_GPU_DEVICE_VULKAN    = 3,
    MEDIAWAY_GPU_DEVICE_METAL     = 4,
    MEDIAWAY_GPU_DEVICE_WEBGPU    = 5,
} mediaway_gpu_device_kind_t;

/* Caller-supplied GPU device handle (mediaway_auto_video_encode_config_t.gpu_device).
 * The caller owns the underlying device and must keep it alive for at least the
 * duration of mediaway_auto_encoder_open. Plain value; no free function. */
typedef struct mediaway_gpu_device_handle {
    mediaway_gpu_device_kind_t kind;
    uintptr_t native;           /* ID3D11Device* / ID3D12Device* / VkDevice / MTLDevice bits; 0 for NONE/WebGpu */
    uint64_t webgpu_device_id;  /* WebGpu only; 0 otherwise */
} mediaway_gpu_device_handle_t;

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

typedef enum mediaway_video_frame_storage_kind {
    MEDIAWAY_VIDEO_FRAME_STORAGE_CPU = 0, /* raw_bytes/raw_bytes_len valid; gpu_buffer unused */
    MEDIAWAY_VIDEO_FRAME_STORAGE_GPU = 1, /* gpu_buffer valid; raw_bytes == NULL, raw_bytes_len == 0 */
} mediaway_video_frame_storage_kind_t;

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

#ifdef __cplusplus
}
#endif

#endif /* MEDIAWAY_PIPELINE_H */
