/*
 * common.h — mediaway-ffi: shared C value types used by container.h/device.h/pipeline.h.
 *
 * Single source of truth for the types that used to be textually duplicated (behind
 * matching #ifndef guards) in each of the three headers above. Mirrors this crate's
 * Rust-side `common::types`/`common::gpu` modules (`src/common/types.rs`, `src/common/gpu.rs`)
 * — see docs/adr/0015-common-ffi-unification.md for why these specific types are shared
 * while each header's own status enum and buffer-free function name stay independent.
 *
 * Does NOT include mediaway_codec_kind_t / mediaway_pipeline_codec_kind_t: those stay two
 * distinct (if numerically mirrored) types by deliberate ADR decision — see
 * adr/pipeline/0001-auto-encode-c-abi.md §2.
 */

#ifndef MEDIAWAY_COMMON_H
#define MEDIAWAY_COMMON_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Rational timebase ────────────────────────────────────────────────────────────── */

typedef struct mediaway_rational {
    uint64_t num;
    uint32_t den; /* must be non-zero */
} mediaway_rational_t;

/* ── Pixel / sample formats ───────────────────────────────────────────────────────── */

/* Only NV12/BGRA8 (video) and F32 (audio) are exercised by the current Windows
 * backends today — an existing Rust-level limitation, not an FFI one. */
typedef enum mediaway_pixel_format {
    MEDIAWAY_PIXEL_FORMAT_NV12  = 0,
    MEDIAWAY_PIXEL_FORMAT_I420  = 1,
    MEDIAWAY_PIXEL_FORMAT_BGRA8 = 2,
    MEDIAWAY_PIXEL_FORMAT_RGBA8 = 3,
    MEDIAWAY_PIXEL_FORMAT_YUYV  = 4,
} mediaway_pixel_format_t;

typedef enum mediaway_sample_format {
    MEDIAWAY_SAMPLE_FORMAT_S16 = 0, /* signed 16-bit LE interleaved PCM */
    MEDIAWAY_SAMPLE_FORMAT_S32 = 1, /* signed 32-bit LE interleaved PCM */
    MEDIAWAY_SAMPLE_FORMAT_F32 = 2, /* IEEE float32 interleaved PCM */
} mediaway_sample_format_t;

/* ── GPU device/buffer handles (mediaway-ffi adr/device/0003-gpu-handle-c-abi.md,
 *    adr/pipeline/0002-gpu-frame-input-c-abi.md) ─────────────────────────────────── */

typedef enum mediaway_gpu_device_kind {
    MEDIAWAY_GPU_DEVICE_NONE      = 0, /* no device supplied — the safe zero-init default */
    MEDIAWAY_GPU_DEVICE_DIRECTX11 = 1,
    MEDIAWAY_GPU_DEVICE_DIRECTX12 = 2,
    MEDIAWAY_GPU_DEVICE_VULKAN    = 3,
    MEDIAWAY_GPU_DEVICE_METAL     = 4,
    MEDIAWAY_GPU_DEVICE_WEBGPU    = 5,
} mediaway_gpu_device_kind_t;

/* Caller-supplied GPU device handle (e.g. a Screen capture config's gpu_device field, or
 * mediaway_auto_video_encode_config_t.gpu_device). Plain value; no free function. The
 * caller owns the underlying device and must keep it alive for at least the duration of
 * the call that consumes it — the exact contract is documented per call site, since
 * lifetime obligations differ by consumer. */
typedef struct mediaway_gpu_device_handle {
    mediaway_gpu_device_kind_t kind;
    uintptr_t native;          /* ID3D11Device* / ID3D12Device* / VkDevice / MTLDevice bits; 0 for NONE/WebGpu */
    uint64_t webgpu_device_id; /* WebGpu only; 0 otherwise */
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

/* GPU frame storage. Direction (borrowed input vs. borrowed output) and ownership are
 * decided by each call site's own header comment — this struct only carries the bits. */
typedef struct mediaway_gpu_buffer_handle {
    mediaway_gpu_buffer_kind_t kind;
    uintptr_t native_a;         /* texture / resource / handle / buffer / image, per kind */
    uintptr_t native_b;         /* Vulkan memory cookie only; 0 otherwise */
    uint32_t subresource;       /* DirectX11 only; 0 otherwise */
    uint64_t webgpu_texture_id; /* WebGpu only; 0 otherwise */
} mediaway_gpu_buffer_handle_t;

/* ── Video frame storage discriminant ─────────────────────────────────────────────── */

typedef enum mediaway_video_frame_storage_kind {
    MEDIAWAY_VIDEO_FRAME_STORAGE_CPU = 0, /* raw byte buffer valid */
    MEDIAWAY_VIDEO_FRAME_STORAGE_GPU = 1, /* gpu_buffer valid */
} mediaway_video_frame_storage_kind_t;

#ifdef __cplusplus
}
#endif

#endif /* MEDIAWAY_COMMON_H */
