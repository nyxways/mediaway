/*
 * device.h — mediaway-device-ffi: C ABI facade over Mediaway's device capture
 * layer (mediaway-device): Camera + Screen video capture; Microphone / Loopback /
 * ProcessLoopback audio capture.
 *
 * Hand-written (not cbindgen-generated) — see adr/0001-capture-c-abi.md §10.
 * Design rules: docs/spec/c-ffi.md (ADR-0004).
 *
 * Third mediaway-*-ffi header in the workspace, after <mediaway/container.h> and
 * <mediaway/pipeline.h>. This is a distinct, independently-numbered status enum and
 * independently-named frame structs from both — see adr/0001-capture-c-abi.md §3/§7
 * for why the three are not unified yet.
 *
 * SCOPE (v1): Camera (video, CPU-only), Screen (video, GPU-only, Windows — see
 * adr/0003-gpu-handle-c-abi.md), and Microphone/Loopback/ProcessLoopback (audio) can
 * all open a session. Window capture is still real in Rust but has NO C constructor
 * this pass — it additionally needs a native HWND input with no consumer-facing use
 * yet (adr/0001-capture-c-abi.md § Deferred); mediaway_video_capture_open() on a
 * Window-kind config ALWAYS returns MEDIAWAY_DEVICE_STATUS_UNSUPPORTED.
 *
 * Screen capture requires a live GPU device handle (mediaway_gpu_device_handle_t,
 * MEDIAWAY_GPU_DEVICE_DIRECTX11 on Windows) passed to
 * mediaway_video_capture_config_screen() — there is no CPU fallback
 * (adr/0003-gpu-handle-c-abi.md). Camera's gpu_device must stay
 * MEDIAWAY_GPU_DEVICE_NONE; mediaway_video_capture_open() rejects a mismatched
 * combination (non-NONE on Camera, or NONE/malformed on Screen) with
 * MEDIAWAY_DEVICE_STATUS_INVALID_INPUT rather than silently ignoring it.
 *
 * Ownership summary (see adr/0001-capture-c-abi.md §6, adr/0003-gpu-handle-c-abi.md §2/§3
 * for the full rationale):
 *   - mediaway_video_capture_config_t / mediaway_audio_capture_config_t are plain
 *     value structs: no heap allocation, no free function, passed/returned by value.
 *   - mediaway_gpu_device_handle_t (Screen capture's gpu_device INPUT) is caller-owned:
 *     the caller must keep the underlying device alive for at least the duration of
 *     the call that consumes it (mediaway_video_capture_open /
 *     mediaway_video_capture_capture_once). This library takes its own internal
 *     reference during that call and does not require the caller's reference to
 *     outlive it afterward.
 *   - mediaway_device_video_frame_t (poll_frame / poll_frame_blocking / capture_once
 *     OUTPUT) is tagged by storage_kind: MEDIAWAY_VIDEO_FRAME_STORAGE_CPU carries a
 *     library-owned buffer, released with mediaway_device_video_frame_free.
 *     MEDIAWAY_VIDEO_FRAME_STORAGE_GPU carries gpu_buffer, a BORROWED handle aliasing
 *     the capture session's own GPU texture — never free it, never call Release() on
 *     it; see the GPU HAZARDS section below. This is the OPPOSITE ownership direction
 *     from mediaway-pipeline-ffi's mediaway_video_frame_t (a borrowed INPUT there) —
 *     same struct name would have been a real field-shape collision, so these are
 *     distinctly-named types.
 *   - mediaway_device_audio_frame_t (poll_frame output) is a library-owned buffer;
 *     release it with mediaway_device_audio_frame_free.
 *   - No bare buffer-free function exists in this header: every owned output here
 *     already carries its own length field, so the two frame-specific frees above are
 *     sufficient (adr/0001-capture-c-abi.md §4).
 *
 * Blocking-close cost (NOT hidden — docs/spec/caveats-and-clarity.md):
 *   - mediaway_video_capture_close / mediaway_audio_capture_close JOIN the backend's
 *     background worker thread and can block for up to ONE FRAME/PERIOD INTERVAL —
 *     this is a real, non-instantaneous cost, not merely a pointer free
 *     (adr/0001-capture-c-abi.md §9).
 *   - mediaway_video_capture_release_frame is a documented no-op for the Camera
 *     backend today (CPU-owned frames hold no backend resource) but must still be
 *     called before the next frame-acquiring poll. For Screen it performs the real
 *     GPU-side release that flips the session's texture slot back to reusable.
 *   - mediaway_video_capture_capture_once is Camera-ONLY — it will refuse a
 *     Screen-kind config with MEDIAWAY_DEVICE_STATUS_UNSUPPORTED. Closing a solo/last
 *     Screen session synchronously frees the GPU texture the just-captured frame's
 *     gpu_buffer would point to, so this convenience cannot safely support Screen; use
 *     mediaway_video_capture_open + mediaway_video_capture_poll_frame_blocking instead
 *     (adr/0003-gpu-handle-c-abi.md § Context, § Decision 5).
 *
 * GPU HAZARDS (Screen capture only — adr/0003-gpu-handle-c-abi.md §8, NOT hidden):
 *   - gpu_buffer's native_a (an ID3D11Texture2D*) is a NON-OWNING, BORROWED pointer —
 *     the library retains the only owning COM reference for the whole session
 *     lifetime. Do NOT call Release() on it. It is the SAME pointer value across
 *     multiple poll calls on one session (refreshed via CopyResource each time), not a
 *     fresh allocation per frame.
 *   - Read window: the texture is valid to read between a successful poll (which
 *     transitions its slot to "held") and the matching mediaway_video_capture_release_frame
 *     (back to "empty"). Reading it after release_frame, or holding it without calling
 *     release_frame promptly while new frames keep arriving, races with the library's
 *     own background thread issuing the next CopyResource into that SAME texture.
 *   - ID3D11Device immediate-context concurrency: the library's background thread
 *     calls GetImmediateContext() + CopyResource on its own thread, using the SAME
 *     ID3D11Device the caller passed in. If the caller's own code issues immediate-
 *     context GPU commands on that same device concurrently, either enable
 *     ID3D11Multithread::SetMultithreadProtected(TRUE) on the device before passing it
 *     in, or confine the caller's own immediate-context use to when no Screen session
 *     is open on that device — otherwise this is the standard "ID3D11 immediate
 *     context is not safe for concurrent multi-thread submission" hazard.
 *
 * Thread safety: every handle (mediaway_video_capture_t*, mediaway_audio_capture_t*)
 * is thread-confined by convention, not internally synchronized. A handle may be
 * moved to another thread, but calling two functions on the SAME handle concurrently
 * from different threads without external synchronization is a data race (undefined
 * behavior), not merely wrong output.
 *
 * Panic safety: every function below (except the plain-value config constructors,
 * which cannot panic) is wrapped in Rust's catch_unwind at the FFI boundary. A caught
 * panic sets a per-handle "poisoned" flag; every subsequent call on that handle
 * short-circuits to MEDIAWAY_DEVICE_STATUS_HANDLE_POISONED, EXCEPT
 * mediaway_video_capture_close / mediaway_audio_capture_close, which are always safe
 * to call, including on a poisoned handle.
 *
 * HOTPLUG (mediaway_device_hotplug_t, adr/0002-callback-event-delivery.md):
 *   - Dual mode on one handle: mediaway_device_hotplug_poll_event (baseline, always
 *     available unless a callback is registered) and an opt-in push mode via
 *     mediaway_device_hotplug_register_callback / _unregister_callback. EXACTLY ONE
 *     mode is active per handle at a time (poll XOR callback), not merely
 *     recommended: poll_event while a callback is registered returns
 *     MEDIAWAY_DEVICE_STATUS_CALLBACK_MODE_ACTIVE and drains nothing;
 *     register_callback on an already-registered handle returns
 *     MEDIAWAY_DEVICE_STATUS_CALLBACK_ALREADY_REGISTERED.
 *   - The callback is invoked from a mediaway-device-ffi-OWNED bridging thread that
 *     polls the real backend every 50ms and invokes the caller's function pointer per
 *     drained event — genuine push from the caller's point of view (no polling loop in
 *     their own code), but with a bounded ~50ms added latency versus true OS-level
 *     delivery. NOT zero-latency.
 *   - Thread-safety contract for mediaway_device_hotplug_callback_fn: may run on an
 *     unspecified Mediaway-owned thread; MUST NOT block; MUST NOT call back into any
 *     mediaway_device_* function on the SAME handle (would deadlock); MUST NOT
 *     unwind/panic across the FFI boundary (this library's catch_unwind cannot catch a
 *     foreign exception/panic unwinding back into it — the caller must prevent that).
 *   - mediaway_device_hotplug_unregister_callback / a callback-mode
 *     mediaway_device_hotplug_close BLOCK for up to the ~50ms poll interval plus the
 *     time any in-flight callback invocation takes to return — the bridging thread
 *     cannot be safely killed mid-callback. Both are idempotent / always safe to call,
 *     including on a poisoned handle, mirroring mediaway_*_capture_close.
 *   - mediaway_device_hotplug_event_t delivered to the callback is BORROWED, valid
 *     only for the duration of that one call — do not free it, and copy device_id out
 *     if needed afterward. The poll_event output is OWNED — free it with
 *     mediaway_device_hotplug_event_free.
 *   - Construction is LAZY: mediaway_device_hotplug_open only validates `kinds` and
 *     never touches the real backend; the real backend is constructed on first
 *     poll_event/register_callback, on whichever thread makes that first call (see
 *     adr/0002-callback-event-delivery.md's lazy-construction revision). On Windows,
 *     that dispatch reaches the real WindowsDeviceHotplug (Microphone/Loopback only,
 *     ADR-0005 Hotplug scope); no Linux hotplug backend exists yet, so that platform's
 *     first touch still returns MEDIAWAY_DEVICE_STATUS_NO_BACKEND. See this crate's own
 *     ADR-0002 implementation addendum for a verification caveat around
 *     mediaway_device_hotplug_close on a real (non-mock) handle.
 */

#ifndef MEDIAWAY_DEVICE_H
#define MEDIAWAY_DEVICE_H

#define MEDIAWAY_DEVICE_FFI_ABI_VERSION 1 /* bumped: Screen dispatch + gpu_device/storage_kind fields, adr/0003-gpu-handle-c-abi.md */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Opaque handles ──────────────────────────────────────────────────────────────── */

/* No member list: layout is private to the Rust implementation and may change without
 * notice pre-1.0. Always access through the functions below. */
typedef struct mediaway_video_capture mediaway_video_capture_t;
typedef struct mediaway_audio_capture mediaway_audio_capture_t;

/* ── Status codes ────────────────────────────────────────────────────────────────── */

typedef enum mediaway_device_status {
    MEDIAWAY_DEVICE_STATUS_OK               = 0,
    MEDIAWAY_DEVICE_STATUS_INVALID_ARGUMENT = 1,  /* null pointer, mismatched ptr/len */
    MEDIAWAY_DEVICE_STATUS_HANDLE_POISONED  = 2,  /* a previous call on this handle panicked */
    MEDIAWAY_DEVICE_STATUS_UNSUPPORTED      = 3,  /* Window this pass; also capture_once() on a Screen config — see file header */
    MEDIAWAY_DEVICE_STATUS_NO_BACKEND       = 4,  /* no capture backend compiled in — expected/graceful */
    MEDIAWAY_DEVICE_STATUS_INVALID_INPUT    = 5,  /* bad config (e.g. zero-denominator time base, mismatched gpu_device) */
    MEDIAWAY_DEVICE_STATUS_BACKEND_FAILURE  = 6,  /* OS/API failure inside the capture backend */
    MEDIAWAY_DEVICE_STATUS_CLOSED           = 7,  /* session already closed or not open */
    MEDIAWAY_DEVICE_STATUS_ACCESS_DENIED    = 8,  /* desktop duplication / device access denied */
    MEDIAWAY_DEVICE_STATUS_UNKNOWN_ERROR    = 9,  /* reserved for a future error variant */
    MEDIAWAY_DEVICE_STATUS_INTERNAL_PANIC   = 10, /* this call caught a Rust panic; the handle is now poisoned */
    MEDIAWAY_DEVICE_STATUS_CALLBACK_ALREADY_REGISTERED = 11, /* register_callback called twice without an intervening unregister_callback */
    MEDIAWAY_DEVICE_STATUS_CALLBACK_MODE_ACTIVE        = 12, /* poll_event called while a callback is registered; drains nothing */
    MEDIAWAY_DEVICE_STATUS_TIMEOUT                     = 13, /* poll_frame_blocking / capture_once's deadline elapsed with no frame (adr/0003-gpu-handle-c-abi.md §6) */
} mediaway_device_status_t;

/* ── Shared value types ──────────────────────────────────────────────────────────── */

/* Identical shape to mediaway-container-ffi's / mediaway-pipeline-ffi's
 * mediaway_rational_t — reused, not re-derived, but a distinct typedef name (no
 * shared header exists yet). */
typedef struct mediaway_rational {
    uint64_t num;
    uint32_t den; /* must be non-zero */
} mediaway_rational_t;

/* Reused verbatim from mediaway-pipeline-ffi — both wrap mediaway_common::PixelFormat
 * identically. Only NV12/BGRA8 are exercised by the current Windows Camera backend
 * today (an existing Rust-level limitation, not a new FFI one). */
typedef enum mediaway_pixel_format {
    MEDIAWAY_PIXEL_FORMAT_NV12  = 0,
    MEDIAWAY_PIXEL_FORMAT_I420  = 1,
    MEDIAWAY_PIXEL_FORMAT_BGRA8 = 2,
    MEDIAWAY_PIXEL_FORMAT_RGBA8 = 3,
    MEDIAWAY_PIXEL_FORMAT_YUYV  = 4,
} mediaway_pixel_format_t;

/* First definition of this enum in the workspace's C headers — mediaway_common::SampleFormat
 * has no prior FFI precedent. Only F32 is accepted by the real Windows WASAPI backend
 * today. */
typedef enum mediaway_sample_format {
    MEDIAWAY_SAMPLE_FORMAT_S16 = 0,
    MEDIAWAY_SAMPLE_FORMAT_S32 = 1,
    MEDIAWAY_SAMPLE_FORMAT_F32 = 2,
} mediaway_sample_format_t;

/* General-purpose mediaway_device::DeviceKind mirror — this crate's FIRST full
 * DeviceKind mirror (mediaway_device_video_source_kind_t / _audio_source_kind_t below
 * are capability-narrowed subsets, not the general enum). Needed by
 * mediaway_device_hotplug_open's kinds parameter and mediaway_device_event_t's
 * device_kind field, both of which need the general kind. UNKNOWN is a decode-side
 * catch-all for a future Rust-side DeviceKind variant this header does not know about
 * yet (DeviceKind is #[non_exhaustive]) — not a value a caller should construct;
 * passing it to mediaway_device_hotplug_open returns MEDIAWAY_DEVICE_STATUS_INVALID_INPUT.
 * See adr/0002-callback-event-delivery.md §6. */
typedef enum mediaway_device_kind {
    MEDIAWAY_DEVICE_KIND_SCREEN           = 0,
    MEDIAWAY_DEVICE_KIND_WINDOW           = 1,
    MEDIAWAY_DEVICE_KIND_CAMERA           = 2,
    MEDIAWAY_DEVICE_KIND_MICROPHONE       = 3,
    MEDIAWAY_DEVICE_KIND_LOOPBACK         = 4,
    MEDIAWAY_DEVICE_KIND_PROCESS_LOOPBACK = 5,
    MEDIAWAY_DEVICE_KIND_UNKNOWN          = 255,
} mediaway_device_kind_t;

/* ── GPU device/buffer handles (adr/0003-gpu-handle-c-abi.md) ─────────────────────── */

/* Both mediaway_gpu_device_handle_t/mediaway_gpu_buffer_handle_t wrap Rust data-carrying
 * enums (mediaway_common::GpuDeviceHandle/GpuBufferHandle) with no existing discriminant
 * sequence to mirror — this crate's own FFI-layer invention, defined once in
 * mediaway-common-ffi and declared here textually (same known duplicate-typedef
 * acceptance mediaway_rational_t/mediaway_pixel_format_t already carry). Flat struct +
 * discriminant, not a C union, matching this header's existing
 * mediaway_device_event_t/mediaway_video_capture_config_t convention. */

typedef enum mediaway_gpu_device_kind {
    MEDIAWAY_GPU_DEVICE_NONE      = 0, /* no device supplied — the safe zero-init default */
    MEDIAWAY_GPU_DEVICE_DIRECTX11 = 1,
    MEDIAWAY_GPU_DEVICE_DIRECTX12 = 2,
    MEDIAWAY_GPU_DEVICE_VULKAN    = 3,
    MEDIAWAY_GPU_DEVICE_METAL     = 4,
    MEDIAWAY_GPU_DEVICE_WEBGPU    = 5,
} mediaway_gpu_device_kind_t;

/* Caller-supplied GPU device handle (e.g. Screen capture's gpu_device). The caller
 * owns the underlying device and must keep it alive for at least the duration of the
 * call that consumes it (mediaway_video_capture_open / mediaway_video_capture_capture_once)
 * — see the file header's ownership summary. Plain value; no free function. */
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

/* Polled GPU frame storage — BORROWED, not owned. See the file header's GPU HAZARDS
 * section for the full COM-refcount / read-window / immediate-context contract. */
typedef struct mediaway_gpu_buffer_handle {
    mediaway_gpu_buffer_kind_t kind;
    uintptr_t native_a;         /* texture / resource / handle / buffer / image, per kind */
    uintptr_t native_b;         /* Vulkan memory cookie only; 0 otherwise */
    uint32_t subresource;       /* DirectX11 only; 0 otherwise */
    uint64_t webgpu_texture_id; /* WebGpu only; 0 otherwise */
} mediaway_gpu_buffer_handle_t;

/* ── Video capture config ─────────────────────────────────────────────────────────── */

typedef enum mediaway_device_video_source_kind {
    MEDIAWAY_DEVICE_VIDEO_SOURCE_SCREEN = 0, /* supported this pass, Windows only — see file header */
    MEDIAWAY_DEVICE_VIDEO_SOURCE_WINDOW = 1, /* no C constructor exposed this pass either */
    MEDIAWAY_DEVICE_VIDEO_SOURCE_CAMERA = 2, /* supported this pass */
} mediaway_device_video_source_kind_t;

/* Plain value type; no free function. No output-preference field: no real backend
 * branches on it today (Camera always CPU-frames, Screen always ZeroCopyGpu
 * internally). gpu_device is meaningful for Screen only — see the file header's SCOPE
 * section for the enforcement rule. */
typedef struct mediaway_video_capture_config {
    mediaway_device_video_source_kind_t source_kind;
    uint32_t source_index; /* output_index (Screen) / device ordinal (Camera) */
    mediaway_rational_t time_base;
    mediaway_gpu_device_handle_t gpu_device; /* Screen only; MEDIAWAY_GPU_DEVICE_NONE for Camera */
} mediaway_video_capture_config_t;

/* ── Audio capture config ─────────────────────────────────────────────────────────── */

typedef enum mediaway_device_audio_source_kind {
    MEDIAWAY_DEVICE_AUDIO_SOURCE_MICROPHONE       = 0,
    MEDIAWAY_DEVICE_AUDIO_SOURCE_LOOPBACK         = 1,
    MEDIAWAY_DEVICE_AUDIO_SOURCE_PROCESS_LOOPBACK = 2,
} mediaway_device_audio_source_kind_t;

/* Plain value type; no free function. */
typedef struct mediaway_audio_capture_config {
    mediaway_device_audio_source_kind_t source_kind;
    uint32_t device_index;          /* Microphone / Loopback endpoint index; ignored for ProcessLoopback */
    uint32_t process_id;            /* ProcessLoopback only */
    bool include_child_processes;   /* ProcessLoopback tree_scope; ignored otherwise */
    mediaway_rational_t time_base;
    mediaway_sample_format_t sample_format; /* only F32 accepted by the real Windows backend today */
} mediaway_audio_capture_config_t;

/* ── Frame output types ───────────────────────────────────────────────────────────── */

typedef enum mediaway_video_frame_storage_kind {
    MEDIAWAY_VIDEO_FRAME_STORAGE_CPU = 0, /* data/data_len valid; gpu_buffer unused */
    MEDIAWAY_VIDEO_FRAME_STORAGE_GPU = 1, /* gpu_buffer valid; data == NULL, data_len == 0 */
} mediaway_video_frame_storage_kind_t;

/* Output of mediaway_video_capture_poll_frame / _poll_frame_blocking / _capture_once.
 * Distinct name AND opposite ownership direction from mediaway-pipeline-ffi's
 * mediaway_video_frame_t (borrowed input there, owned output here — see the file
 * header). storage_kind decides which of data/gpu_buffer is valid
 * (adr/0003-gpu-handle-c-abi.md §3) — a CPU frame's bytes are library-owned, release
 * with mediaway_device_video_frame_free; a GPU frame's gpu_buffer is BORROWED, see the
 * file header's GPU HAZARDS section — never free it. */
typedef struct mediaway_device_video_frame {
    int64_t pts;
    uint64_t duration; /* 0 if unknown */
    uint32_t width;
    uint32_t height;
    mediaway_pixel_format_t pixel_format;
    mediaway_video_frame_storage_kind_t storage_kind;
    uint8_t *data;     /* CPU only; owned, NULL after mediaway_device_video_frame_free */
    size_t data_len;   /* CPU only; 0 whenever storage_kind == GPU */
    mediaway_gpu_buffer_handle_t gpu_buffer; /* GPU only; BORROWED, zeroed whenever storage_kind == CPU */
} mediaway_device_video_frame_t;

/* Output of mediaway_audio_capture_poll_frame — owned; release with
 * mediaway_device_audio_frame_free. First audio-frame struct in any workspace C
 * header. */
typedef struct mediaway_device_audio_frame {
    int64_t pts;
    uint64_t duration;
    uint32_t sample_rate; /* negotiated by the backend (e.g. WASAPI GetMixFormat) */
    uint16_t channels;    /* negotiated by the backend */
    mediaway_sample_format_t sample_format;
    uint8_t *data;         /* owned; NULL after mediaway_device_audio_frame_free */
    size_t data_len;
} mediaway_device_audio_frame_t;

/* ── Hotplug event types (adr/0002-callback-event-delivery.md §6) ────────────────── */

typedef enum mediaway_device_event_kind {
    MEDIAWAY_DEVICE_EVENT_ADDED           = 0,
    MEDIAWAY_DEVICE_EVENT_REMOVED         = 1,
    MEDIAWAY_DEVICE_EVENT_DEFAULT_CHANGED = 2,
    MEDIAWAY_DEVICE_EVENT_STATE_CHANGED   = 3,
} mediaway_device_event_kind_t;

/* A device-change notification. Flat struct + discriminant, not a C union — same
 * "kind field + flat struct" convention as mediaway_video_capture_config_t /
 * mediaway_audio_capture_config_t above.
 *
 * OWNERSHIP DEPENDS ON HOW IT WAS OBTAINED:
 *   - From mediaway_device_hotplug_poll_event: OWNED. Release with
 *     mediaway_device_hotplug_event_free.
 *   - Delivered to a registered mediaway_device_hotplug_callback_fn: BORROWED, valid
 *     only for the duration of that one call. Do NOT free it; copy device_id out
 *     yourself before returning if you need it afterward. */
typedef struct mediaway_device_event {
    mediaway_device_event_kind_t event_kind;
    mediaway_device_kind_t device_kind;
    /* Owned, NUL-terminated UTF-8 device identity string (e.g. "wasapi:<endpoint-id>").
     * NULL only for DEFAULT_CHANGED when the kind now has no default, or (defensively,
     * practically unreachable) if the identity ever contained an embedded NUL —
     * event_kind/device_kind still carry real information even without an id. */
    char *device_id;
} mediaway_device_event_t;

/* ── Hotplug (adr/0002-callback-event-delivery.md) ────────────────────────────────── */

/* mediaway_device_hotplug_open only validates `kinds`; the real backend is constructed
 * lazily on first poll_event/register_callback (see the file header). On Windows that
 * reaches the real WindowsDeviceHotplug; no Linux backend exists yet. See this crate's
 * own ADR-0002 implementation addendum for a verification caveat around
 * mediaway_device_hotplug_close on a real (non-mock) handle. */

typedef struct mediaway_device_hotplug mediaway_device_hotplug_t;

/* The callback function pointer type registered via
 * mediaway_device_hotplug_register_callback. `event` is BORROWED, valid only for the
 * duration of this one call — see mediaway_device_event_t's ownership note above. See
 * the file header's HOTPLUG section for the full thread-safety contract (must not
 * block, must not call back into any mediaway_device_* function on the same handle,
 * must not unwind/panic across the FFI boundary). */
typedef void (*mediaway_device_hotplug_callback_fn)(
    void *user_data, const mediaway_device_event_t *event);

/* Open a hotplug watcher for `kinds` (borrowed, no free function). Mirrors
 * WindowsDeviceHotplug::open(kinds: &[DeviceKind]) directly: an empty `kinds`, or every
 * element mapping to a real but out-of-v1-scope DeviceKind, surfaces whatever error the
 * backend itself returns; a kinds[i] value this header does not recognize
 * (MEDIAWAY_DEVICE_KIND_UNKNOWN) returns MEDIAWAY_DEVICE_STATUS_INVALID_INPUT before any
 * backend call. *out_hotplug is NULL on any non-OK status. */
mediaway_device_status_t mediaway_device_hotplug_open(
    const mediaway_device_kind_t *kinds, size_t kinds_len,
    mediaway_device_hotplug_t **out_hotplug);

/* Close a hotplug watcher, freeing its handle. If a callback is registered, performs
 * the same join as mediaway_device_hotplug_unregister_callback first (implicit
 * unregister — see the file header's HOTPLUG blocking-cost note), then closes the
 * underlying watcher. Always safe to call, including on a poisoned handle or with
 * hotplug == NULL (a no-op, reported as OK). */
mediaway_device_status_t mediaway_device_hotplug_close(mediaway_device_hotplug_t *hotplug);

/* Register a push-mode callback (see the file header's HOTPLUG section for the full
 * thread-safety contract and mode-exclusivity rules). Returns
 * MEDIAWAY_DEVICE_STATUS_CALLBACK_ALREADY_REGISTERED if a callback is already
 * registered on this handle — call unregister_callback first to replace it. */
mediaway_device_status_t mediaway_device_hotplug_register_callback(
    mediaway_device_hotplug_t *hotplug,
    mediaway_device_hotplug_callback_fn callback, void *user_data);

/* Unregister a previously registered callback and return the handle to poll mode.
 * BLOCKS — see the file header's HOTPLUG blocking-cost note. Idempotent: a no-op
 * returning OK when no callback is registered. Always safe to call, including on a
 * poisoned handle (needed to reclaim the bridging thread even after it poisons the
 * handle on a fatal backend error). */
mediaway_device_status_t mediaway_device_hotplug_unregister_callback(
    mediaway_device_hotplug_t *hotplug);

/* Pull the next hotplug event if ready. Only valid in poll mode: returns
 * MEDIAWAY_DEVICE_STATUS_CALLBACK_MODE_ACTIVE and drains nothing while a callback is
 * registered. *out_has_event == false is a valid "no event yet" result, not an error;
 * *out_event is only meaningful when *out_has_event == true, and must then be released
 * with mediaway_device_hotplug_event_free. */
mediaway_device_status_t mediaway_device_hotplug_poll_event(
    mediaway_device_hotplug_t *hotplug, mediaway_device_event_t *out_event,
    bool *out_has_event);

/* Free an event returned by mediaway_device_hotplug_poll_event. Must NOT be called on
 * the borrowed event a registered callback receives (see mediaway_device_event_t's
 * ownership note above). Nulls device_id afterward, making a double-free a visible
 * no-op instead of undefined behavior. */
void mediaway_device_hotplug_event_free(mediaway_device_event_t *event);

/* ── ABI version ─────────────────────────────────────────────────────────────────── */

/* Runtime counterpart to MEDIAWAY_DEVICE_FFI_ABI_VERSION, for consumers that load this
 * library dynamically and never compile against this header. */
uint32_t mediaway_device_ffi_abi_version(void);

/* ── Video capture config constructors (plain value structs, no handle, no free) ─── */

/* gpu_device is always MEDIAWAY_GPU_DEVICE_NONE — Camera never uses it. */
mediaway_video_capture_config_t mediaway_video_capture_config_camera(
    uint32_t device_index, mediaway_rational_t time_base);

/* gpu_device must be a live MEDIAWAY_GPU_DEVICE_DIRECTX11 — see the file header's SCOPE
 * and GPU HAZARDS sections for the caller's device-lifetime obligation. */
mediaway_video_capture_config_t mediaway_video_capture_config_screen(
    uint32_t output_index, mediaway_rational_t time_base,
    mediaway_gpu_device_handle_t gpu_device);

/* ── Video capture ─────────────────────────────────────────────────────────────────── */

/* Open a video capture session for `config`. Camera and Screen configs can both
 * succeed; Window always returns MEDIAWAY_DEVICE_STATUS_UNSUPPORTED (see the file
 * header). A mismatched gpu_device (non-NONE on Camera, NONE/malformed on Screen)
 * returns MEDIAWAY_DEVICE_STATUS_INVALID_INPUT. *out_capture is NULL on any non-OK
 * status (a normal Err, or a caught panic). */
mediaway_device_status_t mediaway_video_capture_open(
    const mediaway_video_capture_config_t *config, mediaway_video_capture_t **out_capture);

/* Query the negotiated frame width/height (only known after the backend has
 * negotiated with the OS — do not assume a resolution). */
mediaway_device_status_t mediaway_video_capture_geometry(
    const mediaway_video_capture_t *capture, uint32_t *out_width, uint32_t *out_height);

/* Pull the next video frame if ready. *out_has_frame == false is a valid "no frame
 * yet" result, not an error; *out_frame is only meaningful when *out_has_frame ==
 * true, and must then be released with mediaway_device_video_frame_free. Check
 * out_frame->storage_kind before reading data vs gpu_buffer — see the file header. */
mediaway_device_status_t mediaway_video_capture_poll_frame(
    mediaway_video_capture_t *capture, mediaway_device_video_frame_t *out_frame,
    bool *out_has_frame);

/* Block until the next video frame is ready or timeout_ms elapses. Unlike
 * poll_frame, MEDIAWAY_DEVICE_STATUS_OK unconditionally means *out_frame was written —
 * no separate has-frame flag. Does NOT close the session (see the file header's
 * blocking-close-cost note) — this is the recommended way to capture a single Screen
 * frame; release_frame/close afterward once done reading it. Returns
 * MEDIAWAY_DEVICE_STATUS_TIMEOUT if timeout_ms elapses with no frame. */
mediaway_device_status_t mediaway_video_capture_poll_frame_blocking(
    mediaway_video_capture_t *capture, uint32_t timeout_ms,
    mediaway_device_video_frame_t *out_frame);

/* Open a Camera capture session, block for one frame (up to timeout_ms), then release
 * and close — a convenience for callers who don't want to manage a session (e.g. a
 * hotkey Camera snapshot). CAMERA ONLY: refuses a Screen-kind config with
 * MEDIAWAY_DEVICE_STATUS_UNSUPPORTED — see the file header's blocking-close-cost note
 * for why; use mediaway_video_capture_open + mediaway_video_capture_poll_frame_blocking
 * for Screen instead. Returns MEDIAWAY_DEVICE_STATUS_TIMEOUT if timeout_ms elapses with
 * no frame. */
mediaway_device_status_t mediaway_video_capture_capture_once(
    const mediaway_video_capture_config_t *config, uint32_t timeout_ms,
    mediaway_device_video_frame_t *out_frame);

/* Release backend resources held by the last polled frame. Documented no-op for the
 * Camera backend today; still must be called before the next frame-acquiring poll. For
 * Screen this is a real GPU-side release — see the file header. */
mediaway_device_status_t mediaway_video_capture_release_frame(mediaway_video_capture_t *capture);

/* Close a video capture session, freeing its handle. BLOCKS for up to one frame
 * interval (joins the backend's worker thread) — see the file header. Always safe to
 * call, including on a poisoned handle or with capture == NULL (a no-op, reported as
 * OK). */
mediaway_device_status_t mediaway_video_capture_close(mediaway_video_capture_t *capture);

/* ── Audio capture config constructors (plain value structs, no handle, no free) ─── */

mediaway_audio_capture_config_t mediaway_audio_capture_config_microphone(
    mediaway_rational_t time_base);
mediaway_audio_capture_config_t mediaway_audio_capture_config_loopback(
    mediaway_rational_t time_base);
mediaway_audio_capture_config_t mediaway_audio_capture_config_process_loopback(
    uint32_t process_id, bool include_child_processes, mediaway_rational_t time_base);

/* ── Audio capture ─────────────────────────────────────────────────────────────────── */

/* Open an audio capture session for `config`. *out_capture is NULL on any non-OK
 * status (a normal Err, or a caught panic). */
mediaway_device_status_t mediaway_audio_capture_open(
    const mediaway_audio_capture_config_t *config, mediaway_audio_capture_t **out_capture);

/* Query the negotiated sample rate/channel count (only known after the backend has
 * negotiated with the OS, e.g. WASAPI IAudioClient::GetMixFormat — do not assume a
 * format). */
mediaway_device_status_t mediaway_audio_capture_format(
    const mediaway_audio_capture_t *capture, uint32_t *out_sample_rate, uint16_t *out_channels);

/* Pull the next PCM chunk if ready. *out_has_frame == false is a valid "no samples
 * yet" result, not an error; *out_frame is only meaningful when *out_has_frame ==
 * true, and must then be released with mediaway_device_audio_frame_free. */
mediaway_device_status_t mediaway_audio_capture_poll_frame(
    mediaway_audio_capture_t *capture, mediaway_device_audio_frame_t *out_frame,
    bool *out_has_frame);

/* Close an audio capture session, freeing its handle. BLOCKS for up to one period
 * interval (joins the backend's worker thread) — see the file header. Always safe to
 * call, including on a poisoned handle or with capture == NULL (a no-op, reported as
 * OK). */
mediaway_device_status_t mediaway_audio_capture_close(mediaway_audio_capture_t *capture);

/* ── Owned output frees ───────────────────────────────────────────────────────────── */

/* Each reads its own length off the struct — no bare generic buffer-free function
 * exists in this header (see the file header). */
void mediaway_device_video_frame_free(mediaway_device_video_frame_t *frame);
void mediaway_device_audio_frame_free(mediaway_device_audio_frame_t *frame);

#ifdef __cplusplus
}
#endif

#endif /* MEDIAWAY_DEVICE_H */
