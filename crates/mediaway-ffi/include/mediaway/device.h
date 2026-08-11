/*
 * device.h — mediaway-device-ffi: C ABI facade over Mediaway's device capture
 * layer. Post-`adr/0004-domain-feature-split.md` surface: the pre-split
 * `mediaway_video_capture_*` / `mediaway_audio_capture_config_t` names no
 * longer exist — video capture is split by domain into Camera
 * (`mediaway_camera_capture_*`) and Desktop/Screen (`mediaway_desktop_capture_*`),
 * and audio into Microphone (`mediaway_audio_capture_*`) and Desktop audio /
 * loopback (`mediaway_desktop_audio_capture_*`).
 *
 * Hand-written (not cbindgen-generated) — see adr/0001-capture-c-abi.md §10 and
 * adr/0004-domain-feature-split.md. Design rules: docs/spec/c-ffi.md (ADR-0004).
 *
 * SCOPE: Camera (video, CPU-only), Screen (video, GPU-only, Windows), and
 * Microphone / Loopback / ProcessLoopback (audio) can all open a session.
 * Window capture is still real in Rust but has NO C constructor this pass
 * (needs a native HWND input with no consumer-facing use yet — adr/0001 §
 * Deferred): `mediaway_desktop_capture_open()` on a Window-kind config ALWAYS
 * returns MEDIAWAY_DEVICE_STATUS_UNSUPPORTED.
 *
 * Screen capture requires a live GPU device handle
 * (`mediaway_gpu_device_handle_t`, MEDIAWAY_GPU_DEVICE_DIRECTX11 on Windows)
 * passed to `mediaway_desktop_capture_config_screen()` — there is no CPU
 * fallback (adr/0003-gpu-handle-c-abi.md). Camera's config carries no
 * gpu_device field at all (every shipped Camera backend is CPU-only).
 *
 * A caller with no pre-existing GPU device to bring (every non-Rust binding,
 * and any Rust caller without an existing renderer device) creates one with
 * `mediaway_gpu_device_create()` (adapter auto-select or explicit index,
 * `mediaway-device` ADR-0007) and reads its `mediaway_gpu_device_handle_t`
 * with `mediaway_gpu_device_handle()` to pass into Screen capture or GPU-input
 * encode. `mediaway_gpu_adapter_list()` enumerates every adapter DXGI reports
 * (name, VRAM, hardware-vs-software) for a caller that wants to pick
 * explicitly rather than accept the default.
 *
 * Ownership summary (adr/0001-capture-c-abi.md §6, adr/0003 §2/§3):
 *   - All capture config structs are plain value structs: no heap allocation,
 *     no free function, passed/returned by value.
 *   - `mediaway_gpu_device_handle_t` (Screen's gpu_device INPUT) is
 *     caller-owned: the caller must keep the underlying device alive for at
 *     least the duration of the call that consumes it. The library takes its
 *     own internal reference during that call and does not require the
 *     caller's reference to outlive it afterward.
 *   - Polled frames are OWNED outputs, released with the matching per-domain
 *     `_frame_free` (`mediaway_camera_frame_free` / `mediaway_desktop_frame_free`
 *     / `mediaway_audio_frame_free` / `mediaway_desktop_audio_frame_free`).
 *     `mediaway_desktop_frame_t`'s `gpu_buffer` (storage_kind == GPU) is a
 *     BORROWED handle aliasing the session's own texture — never free it; see
 *     the GPU HAZARDS section below.
 *   - No bare buffer-free function exists in this header: every owned output
 *     carries its own length field, so the frame-specific frees suffice
 *     (adr/0001 §4).
 *
 * Blocking-close cost (NOT hidden — docs/spec/caveats-and-clarity.md):
 *   - Every `*_capture_close` JOINS the backend's background worker thread and
 *     can block for up to ONE FRAME/PERIOD INTERVAL — a real, non-instantaneous
 *     cost, not merely a pointer free (adr/0001 §9).
 *   - `mediaway_camera_capture_release_frame` is a documented no-op for the
 *     Camera backend today but must still be called before the next
 *     frame-acquiring poll; for Desktop/Screen it performs the real GPU-side
 *     release that flips the session's texture slot back to reusable.
 *   - `mediaway_camera_capture_capture_once` is Camera-ONLY — it refuses a
 *     Desktop-kind config with MEDIAWAY_DEVICE_STATUS_UNSUPPORTED
 *     (adr/0003 § Context, § Decision 5).
 *
 * GPU HAZARDS (Screen capture only — adr/0003 §8, NOT hidden):
 *   - `mediaway_desktop_frame_t.gpu_buffer.native_a` (an ID3D11Texture2D*) is
 *     a NON-OWNING, BORROWED pointer — the library retains the only owning COM
 *     reference for the whole session lifetime. Do NOT call Release() on it.
 *     It is the SAME pointer value across multiple poll calls on one session
 *     (refreshed via CopyResource each time), not a fresh allocation.
 *   - Read window: the texture is valid to read between a successful poll
 *     (slot "held") and the matching `mediaway_desktop_capture_release_frame`
 *     (back to "empty"). Reading it after release, or holding it without
 *     releasing while new frames keep arriving, races with the library's own
 *     background thread issuing the next CopyResource into that SAME texture.
 *   - ID3D11Device immediate-context concurrency: the library's background
 *     thread calls GetImmediateContext() + CopyResource on its own thread
 *     using the SAME ID3D11Device the caller passed in. If the caller's own
 *     code issues immediate-context GPU commands on that same device
 *     concurrently, either enable ID3D11Multithread::SetMultithreadProtected(TRUE)
 *     on the device before passing it in, or confine the caller's own
 *     immediate-context use to when no Screen session is open on that device.
 *
 * Thread safety: every handle is thread-confined by convention, not internally
 * synchronized. A handle may be moved to another thread, but calling two
 * functions on the SAME handle concurrently from different threads without
 * external synchronization is a data race (undefined behavior).
 *
 * Panic safety: every function except the plain-value config constructors is
 * wrapped in Rust's catch_unwind at the FFI boundary. A caught panic sets a
 * per-handle "poisoned" flag; every subsequent call on that handle
 * short-circuits to MEDIAWAY_DEVICE_STATUS_HANDLE_POISONED, EXCEPT the
 * `*_capture_close` functions, which are always safe to call.
 *
 * Feature flags: `camera`, `desktop`, `audio`, `hotplug` (all default).
 * Calling a symbol from a feature the library was built without is a LINK
 * error; opening a session for a capability with no compiled-in backend
 * returns MEDIAWAY_DEVICE_STATUS_NO_BACKEND.
 */

#ifndef MEDIAWAY_DEVICE_H
#define MEDIAWAY_DEVICE_H

#define MEDIAWAY_DEVICE_FFI_ABI_VERSION 1 /* post domain-feature-split (adr/0004) */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "common.h"

#ifdef __cplusplus
extern "C" {
#endif

/* ── Opaque handles ──────────────────────────────────────────────────────────────── */

/* Guarded (not just declared bare): pipeline.h forward-declares these same two types
 * for its capture-to-encode bridge (adr/pipeline/0005-capture-encode-bridge-c-abi.md)
 * so it compiles standalone too — matching macro names here prevent a redefinition
 * error when both headers are included in one translation unit, regardless of order. */
#ifndef MEDIAWAY_CAMERA_CAPTURE_T_DEFINED
#define MEDIAWAY_CAMERA_CAPTURE_T_DEFINED
typedef struct mediaway_camera_capture mediaway_camera_capture_t;
#endif
#ifndef MEDIAWAY_DESKTOP_CAPTURE_T_DEFINED
#define MEDIAWAY_DESKTOP_CAPTURE_T_DEFINED
typedef struct mediaway_desktop_capture mediaway_desktop_capture_t;
#endif
typedef struct mediaway_audio_capture mediaway_audio_capture_t;
typedef struct mediaway_desktop_audio_capture mediaway_desktop_audio_capture_t;
typedef struct mediaway_device_hotplug mediaway_device_hotplug_t;
typedef struct mediaway_gpu_device mediaway_gpu_device_t;

/* ── Status codes (feature-independent, stable numbering) ────────────────────────── */

typedef enum mediaway_device_status {
    MEDIAWAY_DEVICE_STATUS_OK               = 0,
    MEDIAWAY_DEVICE_STATUS_INVALID_ARGUMENT = 1,  /* null pointer, mismatched ptr/len */
    MEDIAWAY_DEVICE_STATUS_HANDLE_POISONED  = 2,  /* a previous call on this handle panicked */
    MEDIAWAY_DEVICE_STATUS_UNSUPPORTED      = 3,  /* Window this pass; capture_once on a Desktop config */
    MEDIAWAY_DEVICE_STATUS_NO_BACKEND       = 4,  /* no capture backend compiled in — expected/graceful */
    MEDIAWAY_DEVICE_STATUS_INVALID_INPUT    = 5,  /* bad config (e.g. zero-denominator time base) */
    MEDIAWAY_DEVICE_STATUS_BACKEND_FAILURE  = 6,  /* OS/API failure inside the capture backend */
    MEDIAWAY_DEVICE_STATUS_CLOSED           = 7,  /* session already closed or not open */
    MEDIAWAY_DEVICE_STATUS_ACCESS_DENIED    = 8,  /* desktop duplication / device access denied */
    MEDIAWAY_DEVICE_STATUS_UNKNOWN_ERROR    = 9,  /* reserved for a future error variant */
    MEDIAWAY_DEVICE_STATUS_INTERNAL_PANIC   = 10, /* this call caught a Rust panic; the handle is now poisoned */
    MEDIAWAY_DEVICE_STATUS_CALLBACK_ALREADY_REGISTERED = 11,
    MEDIAWAY_DEVICE_STATUS_CALLBACK_MODE_ACTIVE        = 12,
    MEDIAWAY_DEVICE_STATUS_TIMEOUT                     = 13, /* poll_frame_blocking / capture_once deadline */
} mediaway_device_status_t;

/* ── Shared value types ──────────────────────────────────────────────────────────── */

/* mediaway_rational_t, mediaway_pixel_format_t, mediaway_sample_format_t, and the GPU
 * device/buffer handle types (adr/device/0003-gpu-handle-c-abi.md) all come from
 * common.h. Polled GPU frame storage here is BORROWED, never freed by the caller — see
 * the file header's GPU HAZARDS section. */

/* ── GPU device factory (mediaway-device ADR-0007) ───────────────────────────────── */

/* Enumeration entry — mediaway_gpu_adapter_list() output; free the whole array
 * (including each entry's `name`) with mediaway_gpu_adapter_list_free(). */
typedef struct mediaway_gpu_adapter_info {
    uint32_t index;      /* pass to mediaway_gpu_adapter_select_t's index field */
    char *name;          /* owned NUL-terminated UTF-8; NULL after free */
    uint32_t vendor_id;
    uint32_t device_id;
    uint64_t dedicated_video_memory; /* bytes */
    bool is_hardware;    /* false for WARP/software adapters */
} mediaway_gpu_adapter_info_t;

typedef enum mediaway_gpu_adapter_select_kind {
    MEDIAWAY_GPU_ADAPTER_SELECT_DEFAULT = 0, /* first hardware adapter DXGI reports */
    MEDIAWAY_GPU_ADAPTER_SELECT_INDEX   = 1, /* mediaway_gpu_adapter_info_t.index */
} mediaway_gpu_adapter_select_kind_t;

typedef struct mediaway_gpu_adapter_select {
    mediaway_gpu_adapter_select_kind_t kind;
    uint32_t index; /* meaningful only when kind == INDEX */
} mediaway_gpu_adapter_select_t;

/* Plain value; no free. */
typedef struct mediaway_gpu_device_options {
    mediaway_gpu_adapter_select_t adapter;
    bool video_support; /* D3D11_CREATE_DEVICE_VIDEO_SUPPORT */
    bool debug_layer;   /* D3D11_CREATE_DEVICE_DEBUG */
} mediaway_gpu_device_options_t;

/* List every GPU adapter this machine's DXGI factory reports. Free with
 * mediaway_gpu_adapter_list_free(), not per-entry. */
mediaway_device_status_t mediaway_gpu_adapter_list(
    mediaway_gpu_adapter_info_t **out_adapters, size_t *out_count);
void mediaway_gpu_adapter_list_free(mediaway_gpu_adapter_info_t *adapters, size_t count);

/* Create a real GPU device per `options`. Close with mediaway_gpu_device_close();
 * every mediaway_gpu_device_handle_t obtained via mediaway_gpu_device_handle()
 * becomes invalid the moment that call returns. */
mediaway_device_status_t mediaway_gpu_device_create(
    const mediaway_gpu_device_options_t *options, mediaway_gpu_device_t **out_device);
mediaway_device_status_t mediaway_gpu_device_handle(
    const mediaway_gpu_device_t *device, mediaway_gpu_device_handle_t *out_handle);
void mediaway_gpu_device_close(mediaway_gpu_device_t *device);

/* ── Camera (video, CPU-only) ────────────────────────────────────────────────────── */

/* Plain value; no free. No gpu_device field: every shipped Camera backend is
 * CPU-only (adr/0004). */
typedef struct mediaway_camera_capture_config {
    uint32_t device_index; /* 0 = default */
    mediaway_rational_t time_base;
} mediaway_camera_capture_config_t;

/* Output of mediaway_camera_capture_poll_frame — OWNED; release with
 * mediaway_camera_frame_free. CPU-only (no storage_kind/gpu_buffer). */
typedef struct mediaway_camera_frame {
    int64_t pts;    /* stream timebase ticks */
    uint64_t duration; /* 0 if unknown */
    uint32_t width;
    uint32_t height;
    mediaway_pixel_format_t pixel_format;
    uint8_t *data;    /* owned; NULL after free */
    size_t data_len;
} mediaway_camera_frame_t;

mediaway_camera_capture_config_t mediaway_camera_capture_config_default(
    uint32_t device_index, mediaway_rational_t time_base);

mediaway_device_status_t mediaway_camera_capture_open(
    const mediaway_camera_capture_config_t *config, mediaway_camera_capture_t **out_capture);
mediaway_device_status_t mediaway_camera_capture_geometry(
    const mediaway_camera_capture_t *capture, uint32_t *out_width, uint32_t *out_height);
mediaway_device_status_t mediaway_camera_capture_poll_frame(
    mediaway_camera_capture_t *capture, mediaway_camera_frame_t *out_frame, bool *out_has_frame);
mediaway_device_status_t mediaway_camera_capture_poll_frame_blocking(
    mediaway_camera_capture_t *capture, uint32_t timeout_ms, mediaway_camera_frame_t *out_frame);
mediaway_device_status_t mediaway_camera_capture_capture_once(
    const mediaway_camera_capture_config_t *config, uint32_t timeout_ms, mediaway_camera_frame_t *out_frame);
mediaway_device_status_t mediaway_camera_capture_release_frame(mediaway_camera_capture_t *capture);
mediaway_device_status_t mediaway_camera_capture_close(mediaway_camera_capture_t *capture);
void mediaway_camera_frame_free(mediaway_camera_frame_t *frame);

/* ── Desktop (Screen video, GPU-only, Windows) ───────────────────────────────────── */

typedef enum mediaway_desktop_capture_source_kind {
    MEDIAWAY_DESKTOP_CAPTURE_SOURCE_SCREEN = 0, /* supported this pass */
    MEDIAWAY_DESKTOP_CAPTURE_SOURCE_WINDOW = 1, /* no C constructor; open() returns UNSUPPORTED */
} mediaway_desktop_capture_source_kind_t;

/* Plain value; no free. gpu_device is mandatory for Screen (adr/0003 §4). */
typedef struct mediaway_desktop_capture_config {
    mediaway_desktop_capture_source_kind_t source_kind;
    uint32_t source_index; /* display output ordinal (0 = primary) */
    mediaway_rational_t time_base;
    mediaway_gpu_device_handle_t gpu_device;
} mediaway_desktop_capture_config_t;

/* mediaway_video_frame_storage_kind_t comes from common.h (CPU: data/data_len valid;
 * GPU: gpu_buffer valid, BORROWED here). */

/* Output of mediaway_desktop_capture_poll_frame — release with
 * mediaway_desktop_frame_free. GPU storage is BORROWED (never freed — see the
 * file header's GPU HAZARDS). */
typedef struct mediaway_desktop_frame {
    int64_t pts;
    uint64_t duration; /* 0 if unknown */
    uint32_t width;
    uint32_t height;
    mediaway_pixel_format_t pixel_format;
    mediaway_video_frame_storage_kind_t storage_kind;
    uint8_t *data;    /* CPU only; owned; NULL after free / when storage_kind == GPU */
    size_t data_len;  /* CPU only; 0 when storage_kind == GPU */
    mediaway_gpu_buffer_handle_t gpu_buffer; /* GPU only; BORROWED */
} mediaway_desktop_frame_t;

mediaway_desktop_capture_config_t mediaway_desktop_capture_config_screen(
    uint32_t output_index, mediaway_rational_t time_base, mediaway_gpu_device_handle_t gpu_device);

mediaway_device_status_t mediaway_desktop_capture_open(
    const mediaway_desktop_capture_config_t *config, mediaway_desktop_capture_t **out_capture);
mediaway_device_status_t mediaway_desktop_capture_geometry(
    const mediaway_desktop_capture_t *capture, uint32_t *out_width, uint32_t *out_height);
mediaway_device_status_t mediaway_desktop_capture_poll_frame(
    mediaway_desktop_capture_t *capture, mediaway_desktop_frame_t *out_frame, bool *out_has_frame);
mediaway_device_status_t mediaway_desktop_capture_poll_frame_blocking(
    mediaway_desktop_capture_t *capture, uint32_t timeout_ms, mediaway_desktop_frame_t *out_frame);
mediaway_device_status_t mediaway_desktop_capture_release_frame(mediaway_desktop_capture_t *capture);
mediaway_device_status_t mediaway_desktop_capture_close(mediaway_desktop_capture_t *capture);
void mediaway_desktop_frame_free(mediaway_desktop_frame_t *frame);

/* ── Audio I/O (Microphone) ──────────────────────────────────────────────────────── */

/* Plain value; no free. Microphone only (adr/0004). */
typedef struct mediaway_audio_capture_config {
    uint32_t device_index; /* 0 = default */
    mediaway_rational_t time_base;
    mediaway_sample_format_t sample_format; /* only F32 accepted today */
} mediaway_audio_capture_config_t;

/* Output of mediaway_audio_capture_poll_frame — OWNED; release with
 * mediaway_audio_frame_free. */
typedef struct mediaway_device_audio_frame {
    int64_t pts;
    uint64_t duration;
    uint32_t sample_rate; /* negotiated by the backend (e.g. WASAPI GetMixFormat) */
    uint16_t channels;    /* negotiated by the backend */
    mediaway_sample_format_t sample_format;
    uint8_t *data;    /* owned interleaved PCM; NULL after free */
    size_t data_len;
} mediaway_device_audio_frame_t;

mediaway_audio_capture_config_t mediaway_audio_capture_config_microphone(
    mediaway_rational_t time_base);

mediaway_device_status_t mediaway_audio_capture_open(
    const mediaway_audio_capture_config_t *config, mediaway_audio_capture_t **out_capture);
mediaway_device_status_t mediaway_audio_capture_format(
    const mediaway_audio_capture_t *capture, uint32_t *out_sample_rate, uint16_t *out_channels);
mediaway_device_status_t mediaway_audio_capture_poll_frame(
    mediaway_audio_capture_t *capture, mediaway_device_audio_frame_t *out_frame, bool *out_has_frame);
mediaway_device_status_t mediaway_audio_capture_close(mediaway_audio_capture_t *capture);
void mediaway_audio_frame_free(mediaway_device_audio_frame_t *frame);

/* ── Desktop audio (Loopback / ProcessLoopback) ──────────────────────────────────── */

typedef enum mediaway_desktop_audio_source_kind {
    MEDIAWAY_DESKTOP_AUDIO_SOURCE_LOOPBACK         = 0,
    MEDIAWAY_DESKTOP_AUDIO_SOURCE_PROCESS_LOOPBACK = 1,
} mediaway_desktop_audio_source_kind_t;

/* Plain value; no free. */
typedef struct mediaway_desktop_audio_capture_config {
    mediaway_desktop_audio_source_kind_t source_kind;
    uint32_t device_index;        /* loopback endpoint ordinal; ignored for ProcessLoopback */
    uint32_t process_id;          /* ProcessLoopback only */
    bool include_child_processes; /* ProcessLoopback tree_scope; ignored otherwise */
    mediaway_rational_t time_base;
    mediaway_sample_format_t sample_format;
} mediaway_desktop_audio_capture_config_t;

/* Output of mediaway_desktop_audio_capture_poll_frame — OWNED; release with
 * mediaway_desktop_audio_frame_free. Same shape as mediaway_device_audio_frame_t. */
typedef struct mediaway_desktop_audio_frame {
    int64_t pts;
    uint64_t duration;
    uint32_t sample_rate;
    uint16_t channels;
    mediaway_sample_format_t sample_format;
    uint8_t *data;    /* owned interleaved PCM; NULL after free */
    size_t data_len;
} mediaway_desktop_audio_frame_t;

mediaway_desktop_audio_capture_config_t mediaway_desktop_audio_capture_config_loopback(
    mediaway_rational_t time_base);
mediaway_desktop_audio_capture_config_t mediaway_desktop_audio_capture_config_process_loopback(
    uint32_t process_id, bool include_child_processes, mediaway_rational_t time_base);

mediaway_device_status_t mediaway_desktop_audio_capture_open(
    const mediaway_desktop_audio_capture_config_t *config, mediaway_desktop_audio_capture_t **out_capture);
mediaway_device_status_t mediaway_desktop_audio_capture_format(
    const mediaway_desktop_audio_capture_t *capture, uint32_t *out_sample_rate, uint16_t *out_channels);
mediaway_device_status_t mediaway_desktop_audio_capture_poll_frame(
    mediaway_desktop_audio_capture_t *capture, mediaway_desktop_audio_frame_t *out_frame, bool *out_has_frame);
mediaway_device_status_t mediaway_desktop_audio_capture_close(mediaway_desktop_audio_capture_t *capture);
void mediaway_desktop_audio_frame_free(mediaway_desktop_audio_frame_t *frame);

/* ── Hotplug (adr/0002-callback-event-delivery.md) ───────────────────────────────── */

typedef enum mediaway_device_kind {
    MEDIAWAY_DEVICE_KIND_SCREEN           = 0,
    MEDIAWAY_DEVICE_KIND_WINDOW           = 1,
    MEDIAWAY_DEVICE_KIND_CAMERA           = 2,
    MEDIAWAY_DEVICE_KIND_MICROPHONE       = 3,
    MEDIAWAY_DEVICE_KIND_LOOPBACK         = 4,
    MEDIAWAY_DEVICE_KIND_PROCESS_LOOPBACK = 5,
    MEDIAWAY_DEVICE_KIND_UNKNOWN          = 255, /* decode-side catch-all; not constructible */
} mediaway_device_kind_t;

typedef enum mediaway_device_event_kind {
    MEDIAWAY_DEVICE_EVENT_ADDED           = 0,
    MEDIAWAY_DEVICE_EVENT_REMOVED         = 1,
    MEDIAWAY_DEVICE_EVENT_DEFAULT_CHANGED = 2,
    MEDIAWAY_DEVICE_EVENT_STATE_CHANGED   = 3,
} mediaway_device_event_kind_t;

/* Owned by mediaway_device_hotplug_poll_event (release with
 * mediaway_device_hotplug_event_free); BORROWED when delivered to a registered
 * callback (valid for the duration of that one call only — copy device_id out
 * yourself if you need it afterward). */
typedef struct mediaway_device_event {
    mediaway_device_event_kind_t event_kind;
    mediaway_device_kind_t device_kind;
    char *device_id; /* owned NUL-terminated UTF-8; NULL for DEFAULT_CHANGED with no default */
} mediaway_device_event_t;

typedef void (*mediaway_device_hotplug_callback_fn)(void *user_data, const mediaway_device_event_t *event);

mediaway_device_status_t mediaway_device_hotplug_open(
    const mediaway_device_kind_t *kinds, size_t kinds_len, mediaway_device_hotplug_t **out_hotplug);
mediaway_device_status_t mediaway_device_hotplug_register_callback(
    mediaway_device_hotplug_t *hotplug, mediaway_device_hotplug_callback_fn callback, void *user_data);
mediaway_device_status_t mediaway_device_hotplug_unregister_callback(mediaway_device_hotplug_t *hotplug);
mediaway_device_status_t mediaway_device_hotplug_poll_event(
    mediaway_device_hotplug_t *hotplug, mediaway_device_event_t *out_event, bool *out_has_event);
mediaway_device_status_t mediaway_device_hotplug_close(mediaway_device_hotplug_t *hotplug);
void mediaway_device_hotplug_event_free(mediaway_device_event_t *event);

/* ── ABI version ─────────────────────────────────────────────────────────────────── */

uint32_t mediaway_device_ffi_abi_version(void);

#ifdef __cplusplus
}
#endif

#endif /* MEDIAWAY_DEVICE_H */
