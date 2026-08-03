# ADR-0003: GPU device/buffer handles across the C ABI — unblocking Screen capture

- **Status**: Accepted (2026-07-31 — implemented; `cargo check --workspace` and
  `clippy --all-targets --all-features -D warnings` clean across
  `mediaway-common-ffi`/`mediaway-device`/`mediaway-device-windows`/
  `mediaway-device-ffi`; the Screen dispatch itself reuses
  `WindowsScreenCapture`, already hardware-verified in `mediaway-device-windows`.
  Not yet hardware-verified: the new C-facing
  `mediaway_video_capture_poll_frame_blocking`/`_capture_once` entry points
  themselves, and `bindings/c/examples/screen_record.c` (predates this ADR's
  `gpu_device` parameter) — see `docs/roadmap.md` Stage 6.)
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-ffi`

## Context

Two prior ADRs deferred the same open problem, stated in almost identical words:

- [`mediaway-ffi/adr/0001`](../../mediaway-ffi/adr/0001-auto-encode-c-abi.md)
  §1: *"A GPU handle crossing a C boundary is its own unsolved design problem
  ... v1 always passes `None`, i.e. CPU-upload-only from C."*
- [`mediaway-device-ffi/adr/0001`](0001-capture-c-abi.md) § Finding 2 / § Deferred:
  `WindowsScreenCapture`/`WindowsWindowCapture` are real, hardware-verified,
  Zero-Copy backends, but **both** hard-require a live
  `GpuDeviceHandle::DirectX11(ID3D11Device*)` with **no** CPU fallback
  (`dxgi.rs`/`wgc.rs` reject `CaptureOutputPreference::CpuFramesOk`
  outright) — so `mediaway_video_capture_open` on a Screen-kind config
  always returns `MEDIAWAY_DEVICE_STATUS_UNSUPPORTED` today, blocking the
  *entire* capability, not merely its fastest path.

This ADR solves that problem concretely, scoped to what's needed to make
**Screen** capture (`WindowsScreenCapture`, DXGI Desktop Duplication) reachable
from C for a single-frame "screenshot" use case feeding a C# binding. Window
capture's HWND-input gap is a separate, still-open item (§ Deferred) —
solving the GPU-handle problem does not by itself unblock Window.

### The Rust surface this wraps has changed since ADR-0001

- [`mediaway-common/src/gpu.rs`](../../mediaway-common/src/gpu.rs):
  `NativeHandle` (opaque `NonZeroUsize`-backed pointer bits, `forbid(unsafe_code)`),
  `GpuBufferHandle` (`#[non_exhaustive]`, 7 variants — `DirectX11{texture,
  subresource}`, `DirectX12{resource}`, `DirectXShared{handle}`,
  `Metal{buffer}`, `AndroidSurface{buffer}`, `Vulkan{image, memory}`,
  `WebGpu{texture_id}`), `GpuDeviceHandle` (`#[non_exhaustive]`, 5 variants —
  `DirectX11`, `DirectX12`, `Vulkan`, `Metal`, `WebGpu{device_id}`).
- `WindowsScreenCapture` is no longer per-session Zero-Copy: every `open()`
  routes through a shared, refcounted `IDXGIOutputDuplication` driven by one
  dedicated background thread ([`mediaway-device-windows` ADR-0006](../../mediaway-device-windows/adr/0006-shared-desktop-duplication.md)),
  which pays one `CopyResource` per frame per attached consumer — a real,
  documented cost, but still GPU-resident (no CPU readback).
- `mediaway-device` ADR-0006 added `VideoCapture::capture_next_frame_blocking`
  (default-provided, retries `poll_frame` with bounded pacing) and the
  facade-level `capture_video_once(open, timeout)` — open → block → release
  → close, hardware-verified for Screen.
- `mediaway-common-ffi` exists (ADR-0015): an `rlib`-only, header-less crate
  sharing `Rational`/`CodecKind` `#[repr(C)]` mirrors + the buffer
  leak/reclaim helper across `-ffi` crates.

### A real bug found while designing this — fixed ahead of this ADR

Investigating whether to expose `capture_video_once` from C for Screen
(§ Decision 5) surfaced a genuine Rust-level correctness bug in
`mediaway-device`, confirmed by reading source line-by-line:
`capture_video_once` called `session.close()` **before** returning the
already-captured `VideoFrame`. For `WindowsScreenCapture`, closing the
**last** attached consumer of a shared DXGI session
(`mediaway-device-windows` ADR-0006) synchronously joins the driver thread
and drops the COM texture the just-captured frame's
`GpuBufferHandle::DirectX11` handle points to — so the caller would receive
an already-dangling handle. This was invisible to this session's own
hardware test (`capture_video_once_screen_returns_a_frame_or_skip`), which
only asserts `width`/`height` (plain `u32` fields, unaffected by the
dangling texture) and never dereferences the GPU handle.

**Fixed in `mediaway-device/src/video.rs`, ahead of writing this ADR**:
`capture_video_once` now returns `Err(CaptureError::Unsupported)` instead of
the captured frame when its storage is `VideoFrameStorage::Gpu` — closing the
session to return control to the caller can free the underlying GPU resource
before the caller can read it. Callers that need a GPU-backed frame must keep
the session open themselves (`poll_frame` / `capture_next_frame_blocking`),
releasing and closing only after they've read the frame. Camera's CPU
`Bytes`-backed frames are unaffected (independent of session lifetime).

This fix is exactly why § Decision 5 below does **not** expose a composed
`capture_once`-style convenience for Screen at the C ABI layer either — the
same closing-order hazard would otherwise resurface one layer up.

A second, narrower finding (`DXGI_ERROR_ACCESS_LOST` silently swallowed by
`dxgi_shared.rs`'s driver loop, never surfaced as a distinguishable error) is
logged under § Deferred — a `mediaway-device-windows` fix, out of scope here.

## Decision

> Add a shared, `#[repr(C)]`, flat-struct-plus-discriminant representation of
> `GpuDeviceHandle`/`GpuBufferHandle` to **`mediaway-common-ffi`** (not
> crate-local to `mediaway-device-ffi`), wire real Screen dispatch into
> `mediaway_video_capture_open`, extend the existing `poll_frame` output
> struct with a storage-kind tag rather than inventing a second frame type or
> function, and add a new **blocking single-poll** function
> (`mediaway_video_capture_poll_frame_blocking`) that does **not** close the
> session. `mediaway_video_capture_capture_once` (Camera-only, matching this
> crate's existing v1 scope) is added separately as a genuine one-call
> convenience — safe because Camera is always CPU-backed. `Window` stays
> deferred (separate HWND-input gap, unaffected by this decision). No new
> `unsafe` boundary is introduced: `mediaway-common-ffi`'s new module stays
> plain, safe `#[repr(C)]` + conversions (mirroring its existing `types.rs`);
> all `unsafe` needed to reach the new dispatch already exists in
> `mediaway-device-windows` (`dxgi.rs`/`dxgi_shared.rs`) and
> `mediaway-device-ffi`'s own pointer-arg functions.

### 1. C struct shape — flat struct + discriminant, in `mediaway-common-ffi`

Both `GpuDeviceHandle` and `GpuBufferHandle` are **data-carrying** Rust
enums — unlike `CodecKind`/`PixelFormat` (plain, fieldless, effectively
C-like enums this workspace already mirrors 1:1 numerically), there is no
existing Rust discriminant sequence to preserve here; the C `kind` enum's
numbering is a fresh FFI-layer invention. Consistent with this crate's
established "flat struct, not a C union" convention
(`mediaway_device_event_t`, `mediaway_video_capture_config_t`,
`mediaway_audio_capture_config_t` all use a kind field + fields that are only
meaningful for some kinds), not this crate's first tagged union:

```c
/* mediaway-common-ffi — new shared value types, no C symbols of their own
 * (this crate never gets a header/cdylib; each consuming -ffi crate declares
 * this text in its own header, same duplication story Rational/PixelFormat
 * already accept per ADR-0015 § Deferred). */

typedef enum mediaway_gpu_device_kind {
    MEDIAWAY_GPU_DEVICE_NONE      = 0, /* no device supplied — the safe zero-init default */
    MEDIAWAY_GPU_DEVICE_DIRECTX11 = 1,
    MEDIAWAY_GPU_DEVICE_DIRECTX12 = 2,
    MEDIAWAY_GPU_DEVICE_VULKAN    = 3,
    MEDIAWAY_GPU_DEVICE_METAL     = 4,
    MEDIAWAY_GPU_DEVICE_WEBGPU    = 5,
} mediaway_gpu_device_kind_t;

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

typedef struct mediaway_gpu_buffer_handle {
    mediaway_gpu_buffer_kind_t kind;
    uintptr_t native_a;         /* texture / resource / handle / buffer / image, per kind */
    uintptr_t native_b;         /* Vulkan memory cookie only; 0 otherwise */
    uint32_t subresource;       /* DirectX11 only; 0 otherwise */
    uint64_t webgpu_texture_id; /* WebGpu only; 0 otherwise */
} mediaway_gpu_buffer_handle_t;
```

**Placement: `mediaway-common-ffi`, not crate-local.** Applying ADR-0015's own
decision criterion directly: both types wrap the *identical* shared
`mediaway_common::{GpuDeviceHandle, GpuBufferHandle}` end-to-end, and a second
real consumer is already named and waiting (`mediaway-ffi`'s
twice-deferred `gpu_device`/`max_path_class`, § Deferred). ADR-0015 built
`Rational`/`CodecKind` there only after two crates had *already* duplicated
them; here we can skip ever creating a first crate-local copy that would need
migrating later — a strictly better position than ADR-0015 started from.
`mediaway-common-ffi` stays `rlib`-only with **zero** new `#[no_mangle]`
symbols (a new `src/gpu.rs` module: two `#[repr(C)]` structs + two enums +
plain, **safe** `From`/helper conversions — no `unsafe` needed, matching the
existing `types.rs`). Each consuming crate's hand-written header still
declares the C struct text locally (the same known, accepted duplicate-`typedef`
risk `mediaway_rational_t`/`mediaway_pixel_format_t` already carry per
ADR-0001 §7 / ADR-0015 § Deferred — not solved by this ADR either).

Rust-side sketch:

```rust
// mediaway-common-ffi/src/gpu.rs
use mediaway_common::{
    GpuBufferHandle as CommonGpuBufferHandle, GpuDeviceHandle as CommonGpuDeviceHandle,
    NativeHandle,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuDeviceKind { None = 0, DirectX11 = 1, DirectX12 = 2, Vulkan = 3, Metal = 4, WebGpu = 5 }

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuDeviceHandle {
    pub kind: GpuDeviceKind,
    pub native: usize,
    pub webgpu_device_id: u64,
}

impl GpuDeviceHandle {
    /// `None` for `GpuDeviceKind::None`, or when `native == 0` for a
    /// non-WebGpu kind (malformed/zero-initialized input) — the caller
    /// treats this identically to "no device supplied".
    pub fn to_common(self) -> Option<CommonGpuDeviceHandle> {
        match self.kind {
            GpuDeviceKind::None => None,
            GpuDeviceKind::DirectX11 => Some(CommonGpuDeviceHandle::DirectX11(NativeHandle::new(self.native)?)),
            GpuDeviceKind::DirectX12 => Some(CommonGpuDeviceHandle::DirectX12(NativeHandle::new(self.native)?)),
            GpuDeviceKind::Vulkan => Some(CommonGpuDeviceHandle::Vulkan(NativeHandle::new(self.native)?)),
            GpuDeviceKind::Metal => Some(CommonGpuDeviceHandle::Metal(NativeHandle::new(self.native)?)),
            GpuDeviceKind::WebGpu => Some(CommonGpuDeviceHandle::WebGpu { device_id: self.webgpu_device_id }),
        }
    }
}

// GpuBufferHandle -> C: infallible (Rust always produces a valid variant);
// `Unknown` only guards a future #[non_exhaustive] variant this header
// doesn't know about yet, mirroring `MediawayDeviceKind::Unknown`'s
// established idiom in mediaway-device-ffi.
impl From<CommonGpuBufferHandle> for GpuBufferHandle { /* ... */ }
```

### 2. `GpuDeviceHandle` ownership/lifetime contract

The caller owns the underlying `ID3D11Device`. Tracing the real acquisition
path: `WindowsScreenCapture::open` reconstructs only a **borrowed** reference
from the raw pointer (`ID3D11Device::from_raw_borrowed`, no `AddRef`) purely
to enumerate the adapter/output; the actual owned COM reference is taken
later, **synchronously**, inside `dxgi_shared::attach` → `spawn_driver` →
`open_duplication`'s `device_ref.clone()` (a real `AddRef`), which runs on a
newly spawned thread but `attach()` **blocks on `ready_rx.recv()`** until that
clone has already happened before `open()` returns to its caller.

**Stated contract:** the caller must keep the `ID3D11Device` alive for at
least the duration of `mediaway_video_capture_open()` (i.e., until it
returns, `Ok` or `Err`). After a successful `open()`, Mediaway's internal
driver thread holds its own COM reference and the caller's own reference may
be released without invalidating the session — though in practice most
Windows hosts keep their D3D11 device alive for the app's lifetime regardless.
**Caveat not fully closed by this ADR:** a *second*, joining `open()` on an
already-shared output compares only raw pointer bits
(`shared.device_raw != device_raw`) before taking any new reference — a
caller presenting a stale/freed device pointer that happens to alias a live
session's stored bits (ABA) is not detected. This is documented as a caller
obligation (present a genuinely live device on every `open()` call,
joining or not), not fixed here — the same class of "trust the raw pointer"
boundary `NativeHandle` already establishes everywhere else in this codebase
(§ Hazards).

### 3. `poll_frame`'s GPU output path — extend the existing frame struct, not a new function

```c
typedef enum mediaway_video_frame_storage_kind {
    MEDIAWAY_VIDEO_FRAME_STORAGE_CPU = 0, /* data/data_len valid; gpu_buffer unused/zeroed */
    MEDIAWAY_VIDEO_FRAME_STORAGE_GPU = 1, /* gpu_buffer valid; data == NULL, data_len == 0 */
} mediaway_video_frame_storage_kind_t;

typedef struct mediaway_device_video_frame {
    int64_t pts;
    uint64_t duration;
    uint32_t width;
    uint32_t height;
    mediaway_pixel_format_t pixel_format;
    mediaway_video_frame_storage_kind_t storage_kind;
    uint8_t *data;                            /* CPU only; owned, NULL after free */
    size_t data_len;                          /* CPU only */
    mediaway_gpu_buffer_handle_t gpu_buffer;   /* GPU only — BORROWED, see below */
} mediaway_device_video_frame_t;
```

Chosen over a second function (`poll_frame_gpu`) or a C union: matches this
crate's own established "kind field decides which fields matter" idiom
(`mediaway_audio_capture_config_t`'s `process_id` is present but ignored
outside `ProcessLoopback`) rather than doubling the poll surface. **No change
needed to `mediaway_device_video_frame_free`'s signature or logic**: GPU
frames simply never populate `data`/`data_len` (they stay `NULL`/`0`), and
the existing free function already treats `(NULL, 0)` as a no-op — the
existing implementation is already correct for this case with zero code
change, a direct benefit of `release_frame`/`data_len`-null-is-safe already
being designed defensively.

**`gpu_buffer` is a *borrowed* handle, not owned:** it aliases the shared
session's per-consumer `ID3D11Texture2D*` (`dxgi_shared.rs`'s
`attach_consumer`/`copy_to_ready_consumers`), which is allocated **once** at
attach time and **reused** (refreshed via `CopyResource`) on every frame for
that consumer — **the same raw pointer value is returned across multiple
`poll_frame` calls on one session**, unlike Camera's fresh-allocation-per-frame
CPU model. The caller must never attempt to free it. It is valid to read
between a successful `poll_frame` (transitions the slot to `Held`) and the
matching `release_frame` (transitions back to `Empty`) — reading it after
`release_frame`, or without calling `release_frame` promptly, races with the
driver thread's next `CopyResource` into that same texture (§ Hazards).

### 4. Config surface — add `gpu_device` only; do **not** add `output`

```c
typedef struct mediaway_video_capture_config {
    mediaway_device_video_source_kind_t source_kind;
    uint32_t source_index;
    mediaway_rational_t time_base;
    mediaway_gpu_device_handle_t gpu_device;  /* NEW — Screen only; MEDIAWAY_GPU_DEVICE_NONE for Camera */
} mediaway_video_capture_config_t;

mediaway_video_capture_config_t mediaway_video_capture_config_camera(
    uint32_t device_index, mediaway_rational_t time_base);
/* gpu_device set to {MEDIAWAY_GPU_DEVICE_NONE, 0, 0} — Camera never uses it. */

mediaway_video_capture_config_t mediaway_video_capture_config_screen(
    uint32_t output_index, mediaway_rational_t time_base,
    mediaway_gpu_device_handle_t gpu_device);   /* CHANGED: new required 3rd parameter */
```

**`output`/`CaptureOutputPreference` is deliberately *not* added as a C
field**, unlike the strawman this ADR was asked to evaluate. Reasoning: as of
today, **no real backend actually branches on it** — Camera hard-requires
`CpuFramesOk` internally (already hardcoded, ADR-0001 §5) and Screen
hard-requires `ZeroCopyGpu` (`dxgi.rs::open` rejects `CpuFramesOk` outright).
Exposing `output` today would be exactly the "config knob that looks
configurable but can never do anything different" ADR-0001 §5 already
rejected once for this whole config — the same reasoning applies again, now
to a specific field rather than the whole surface. `mediaway-device-ffi`'s
dispatch derives it internally instead, mirroring
`VideoCaptureConfig::screen()`'s own Rust-level hardcoding of
`output: CaptureOutputPreference::ZeroCopyGpu`. Revisit only once a real
backend exists that meaningfully honors both preferences.

**Enforcement for `gpu_device`, not just documentation:** because
`mediaway_video_capture_config_t` is a plain, caller-mutable value struct,
nothing stops a caller from setting `gpu_device.kind != NONE` on a
Camera-sourced config after construction. `mediaway_video_capture_open`
**rejects this at open time** — `source_kind == Camera` with a non-`NONE`
`gpu_device` returns `MEDIAWAY_DEVICE_STATUS_INVALID_INPUT`, rather than
silently ignoring the field. This is an *active* enforcement of "no unusable
knobs presented as configurable," not merely a doc comment — the concrete
gap the task's own framing of this question anticipated. Symmetrically,
`source_kind == Screen` with `gpu_device.kind == NONE` (or a malformed
non-zero-kind-but-zero-native value, which `to_common()` also collapses to
`None`) returns `MEDIAWAY_DEVICE_STATUS_INVALID_INPUT` — identical to the
existing Rust-level `let Some(GpuDeviceHandle::DirectX11(handle)) =
config.gpu_device else { return Err(CaptureError::InvalidInput) };` in
`dxgi.rs`, not a new rule invented at the FFI layer.

`MEDIAWAY_GPU_DEVICE_NONE = 0` (not the last value, unlike
`MediawayDeviceKind::Unknown = 255`) is a deliberate choice: a
zero-initialized `mediaway_gpu_device_handle_t` (e.g. `{0}` in C, or a
forgotten field in a language binding that zero-fills structs by default)
decodes as "no device supplied" → a clean `InvalidInput` for Screen, rather
than an attempt to interpret zeroed bits as a real `DirectX11` device
pointer. `NativeHandle::new(0)` already returns `None` on the Rust side
(`NonZeroUsize`-backed), so this safety property is structurally reinforced,
not merely conventional.

### 5. `capture_once` — Camera only; Screen uses `poll_frame_blocking` instead

Given the closing-order bug already found and fixed (§ Context), this ADR
adds two distinct convenience functions with different scopes:

```c
/* Camera-only, mirrors mediaway_video_capture_open + capture_next_frame_blocking
 * + release_frame + close, matching this crate's existing v1 (Camera-only) scope.
 * Safe because Camera's output is always CPU-backed — no dangling-handle risk. */
mediaway_device_status_t mediaway_video_capture_capture_once(
    const mediaway_video_capture_config_t *config, uint32_t timeout_ms,
    mediaway_device_video_frame_t *out_frame);

/* Screen (and Camera) — session-scoped blocking poll; does NOT close the
 * session. No out_has_frame parameter: capture_next_frame_blocking returns
 * Result<VideoFrame, CaptureError> (not Option), so an OK status
 * unconditionally means *out_frame was written. */
mediaway_device_status_t mediaway_video_capture_poll_frame_blocking(
    mediaway_video_capture_t *capture, uint32_t timeout_ms,
    mediaway_device_video_frame_t *out_frame);
```

`mediaway_video_capture_capture_once` composes open → block → release →
close purely at the FFI layer (calling the already-open `open_camera_capture`
dispatch), rejecting Screen/Window configs with
`MEDIAWAY_DEVICE_STATUS_UNSUPPORTED` — it never calls the (now-fixed)
`mediaway_device::capture_video_once` for a GPU-storage source, so it cannot
resurrect the dangling-handle bug even indirectly.

The C caller's Screen "screenshot" recipe becomes: `mediaway_video_capture_open`
(Screen kind, real `gpu_device`) → `mediaway_video_capture_poll_frame_blocking`
→ consume the CPU bytes or GPU texture on the caller's own time →
`mediaway_video_capture_release_frame` → `mediaway_video_capture_close`. This
sidesteps the closing-order hazard entirely: the caller — not this library —
decides when the session closes, always *after* it has finished consuming
the frame.

### 6. Status enum — add `TIMEOUT`

```c
typedef enum mediaway_device_status {
    /* ... existing 0-12 unchanged ... */
    MEDIAWAY_DEVICE_STATUS_TIMEOUT = 13, /* CaptureError::Timeout, from *_capture_once / *_poll_frame_blocking */
} mediaway_device_status_t;
```

`CaptureError::Timeout` already exists on the Rust side (added by
`mediaway-device` ADR-0006) but today falls into `MediawayDeviceStatus`'s
`UnknownError` catch-all — harmless until now because no exported function
could ever produce it. The two new blocking functions are the first that
can, so it needs a real, distinct status value rather than an ambiguous
catch-all. **Not** adding a status for `CaptureError::DeviceLost` in this
pass — no code path this ADR touches can produce it yet (dxgi_shared's
access-loss handling doesn't surface it as a distinguishable error yet), so
adding a status for it now would be speculative (against "no features beyond
the request"); tracked under § Deferred instead.

### 7. Screen dispatch — mirrors the existing Camera dispatch shape exactly

```rust
// mediaway-device-ffi/src/video.rs (sketch, not final code)
fn screen_select(source_index: u32) -> Result<Select, CaptureError> {
    // Identical shape to camera_select (existing code): 0 => Select::Default,
    // nonzero => mediaway_device_windows::enumerate(DeviceKind::Screen) round trip.
}

fn open_screen_capture(config: &VideoCaptureConfig) -> Result<Box<dyn VideoCapture>, CaptureError> {
    #[cfg(windows)]
    { Ok(Box::new(mediaway_device_windows::WindowsScreenCapture::open(config)?)) }
    #[cfg(not(windows))]
    { let _ = config; Err(CaptureError::NoBackend) } // Linux Screen backend: see ADR-0001 § Deferred, unrelated to this ADR
}
```

Inside `mediaway_video_capture_open`'s match: `Camera` unchanged except for
the new `gpu_device`-must-be-`NONE` check (§4); `Screen` now builds a real
`VideoCaptureConfig { source: CaptureSource::Screen { select },
output: CaptureOutputPreference::ZeroCopyGpu, gpu_device: config.gpu_device.to_common(), .. }`
and calls `open_screen_capture` instead of unconditionally returning
`CaptureError::Unsupported`; `Window` is **unchanged** — still
`CaptureError::Unsupported` (no HWND constructor exists, § Deferred, orthogonal
to the GPU-handle problem this ADR solves).

**Zero changes required in `mediaway-device` beyond the Finding-A fix, and
none in `mediaway-device-windows`** — `WindowsScreenCapture::open`/
`poll_frame`/`release_frame`/`close` already accept and produce exactly what
this dispatch needs; this ADR's entire job is making an *already-complete*
Rust capability reachable from C.

### 8. Hazards (must be documented on the header, not merely known internally)

1. **COM reference discipline on `gpu_buffer.native_a` (DirectX11 case).**
   The returned pointer is a **non-owning, borrowed** `ID3D11Texture2D*` —
   the driver thread retains the only owning COM reference for the
   consumer's whole session lifetime. The C caller **must not call
   `Release()`** on it. If the caller wants to retain the texture (or a copy
   of it) beyond the `poll_frame`/`release_frame` window, it must `AddRef()`
   its own reference (or, more simply, copy the content out) before calling
   `release_frame`. This is a genuinely new-to-C-callers rule this ADR
   introduces (the Rust side's `Copy`/no-`Drop` `GpuBufferHandle` gave no
   prior signal that COM refcount discipline applies at all).
2. **Texture read-window, not just handle thread-confinement.** This crate's
   existing "handles are thread-confined by convention" rule (ADR-0001 §9)
   covers the opaque `mediaway_video_capture_t*` only — it does **not**
   extend to the GPU texture a Screen session hands out. That texture is
   concurrently touched by (a) whatever thread the caller uses to
   read/consume it, and (b) Mediaway's own internal driver thread, which
   issues a fresh `CopyResource` into the *same* texture object once
   `release_frame` flips its slot back to `Empty`. Reading the texture after
   calling `release_frame` (or holding it without calling `release_frame`
   promptly while new frames keep arriving) races with that write. This is a
   **new, separate contract**, not a restatement of handle-level
   thread-confinement — must be its own header paragraph.
3. **`ID3D11Device` immediate-context concurrency is *not* automatically
   safe**, and is a pre-existing Rust-level caveat this ADR is the first to
   surface for a *C* audience (C callers have no access to
   `mediaway-device-windows`'s own rustdoc/ADR). Every consumer's texture is
   refreshed by the driver thread calling `device.GetImmediateContext()` +
   `CopyResource` on **its own** thread, using the **same** `ID3D11Device`
   the caller passed in. If the caller's own code issues immediate-context
   GPU commands on that same device concurrently (e.g., to render into a
   swap chain, or to itself read the shared texture via the immediate
   context) without either (a) enabling
   `ID3D11Multithread::SetMultithreadProtected(TRUE)` on the device before
   passing it in, or (b) confining all of the caller's own immediate-context
   use to a period when no Screen/Window capture session is open on that
   device, this is the textbook "ID3D11 immediate context is not safe for
   concurrent multi-thread command submission" hazard. Must be stated
   explicitly in the header, not left to be discovered.
4. **Raw device pointer trust is not a new class of hole.** Accepting an
   opaque `uintptr_t`/`void*` device pointer from C with no runtime type
   validation is the same trust boundary `NativeHandle` already establishes
   everywhere in `mediaway-common` (e.g., `VideoCaptureConfig::window`'s
   `HWND` token) — there is no practical way to validate an opaque COM
   pointer's real type from outside the object itself. The *consequence* of
   misuse is worse here (a garbage `ID3D11Device*` reaching `.cast::<IDXGIDevice>()`
   can crash the process), but the *shape* of the hazard is identical to
   every other `NativeHandle` use, not novel to this ADR.
5. **ABA on the second-attacher device-liveness check** (§ Decision 2) —
   flagged, not fixed.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Crate-local `mediaway_device_gpu_*` types instead of `mediaway-common-ffi` | Would create the exact "first independent copy that needs migrating later" situation ADR-0015 already had to clean up once for `Rational`/`CodecKind`, with `mediaway-ffi`'s twice-deferred `gpu_device` already a known, named second consumer |
| A C union (`union { dx11; dx12; vulkan; ... }`) for `gpu_buffer`/`gpu_device` | Breaks this workspace's own established "flat struct + discriminant, not a C union" convention (`mediaway_device_event_t`, `mediaway_video_capture_config_t`) with no offsetting benefit — the flat shape already handles per-kind field relevance elsewhere in this crate |
| `gpu_device` as a nullable `const mediaway_gpu_device_handle_t *` pointer field | Would make the config struct the first pointer-bearing, lifetime-obligated field among this crate's otherwise plain-value "no heap, no free" config structs; a `NONE`-sentinel flat value keeps the exact same POD-by-value property every sibling config struct already has |
| Expose `output: CaptureOutputPreference` as a C config field | No real backend today branches on it — Camera and Screen each hard-require exactly one value; would be the same "unusable knob" ADR-0001 §5 already rejected once, applied again |
| Expose a composed all-in-one `capture_once` for Screen | Would package the closing-order dangling-GPU-handle bug directly into a new C convenience function; `mediaway_video_capture_poll_frame_blocking` gets the same "single-frame screenshot" outcome without inheriting the bug |
| Silently ignore `gpu_device` for Camera-kind configs (accept but do nothing) | Exactly the "config knob that looks configurable but can never do anything different" ADR-0001 §5 warned against; an explicit `INVALID_INPUT` makes the no-op visible instead of silent |
| Number `mediaway_gpu_buffer_kind_t`/`mediaway_gpu_device_kind_t` to numerically mirror the Rust enums' *declaration order* including a shared `Unknown`/`None` placement scheme | `GpuDeviceHandle`/`GpuBufferHandle` carry no existing Rust discriminant to mirror (unlike `CodecKind`/`PixelFormat`) — there is no "correct" numbering to preserve; `NONE = 0` for `GpuDeviceKind` (defensive zero-init) and `UNKNOWN = 255` for `GpuBufferKind` (decode-side catch-all, no `NONE` needed since C never constructs this direction) are each chosen for their own reason, not forced to match |

## Consequences

### Positive

- Screen capture — a real, hardware-verified, GPU Zero-Copy-*capable* Rust
  backend that has been completely unreachable from C since ADR-0001 — becomes
  reachable, closing the exact gap that ADR (and `mediaway-ffi/adr/0001`
  §1) both named as "an unsolved design problem," with a concrete, reviewable
  C representation.
- `mediaway-common-ffi` gains its second real type family with a stated,
  named future second consumer (`mediaway-ffi`) already in hand —
  the intended trajectory ADR-0015 set up, realized without ever creating a
  throwaway crate-local copy first.
- A genuine Rust-level correctness bug in `capture_video_once` was found and
  fixed *before* a matching C convenience function could have paved over it.
- `mediaway_video_capture_release_frame`/`mediaway_device_video_frame_free`
  need **zero** code changes to become correct for the new GPU path — both
  were already written defensively (no-op-safe on null/zero) anticipating
  exactly this future case, per ADR-0001 §6's own stated intent.
- No changes required in `mediaway-device-windows` Rust source — the entire
  capability this ADR unlocks was already built and hardware-verified; this
  pass is purely an ABI-reachability fix.
- No new `unsafe` boundary: the new `mediaway-common-ffi::gpu` module is
  plain, safe Rust, identical in kind to its existing `types.rs`.

### Negative / Trade-offs

- `mediaway_video_capture_config_screen`'s signature changes (new required
  parameter) — an ABI break, acceptable pre-1.0 (`publish = false`), but
  requires bumping `MEDIAWAY_DEVICE_FFI_ABI_VERSION`.
- `mediaway_device_video_frame_t` grows by 3 fields (`storage_kind`,
  `gpu_buffer`, implicitly reordering existing fields around them) — another
  ABI break to the same header, same version-bump requirement.
- Two new, real caveats (COM refcount discipline on the borrowed texture;
  immediate-context concurrency) now exist that no prior `-ffi` crate's
  header has had to state, because no prior crate exposed a live GPU handle
  to a C caller at all. Getting this wrong is a genuine crash/UB risk, not
  merely a logic bug.
- Screen's own screenshot convenience remains split into two calls (`open` +
  `poll_frame_blocking`, caller still manages `release_frame`/`close`
  explicitly) rather than one call — a real ergonomics cost accepted in
  exchange for not shipping a known-buggy composed convenience.
- `Window` capture is still entirely unreachable from C — this ADR solves
  the GPU-handle problem generally, but `WindowsWindowCapture` also needs an
  HWND C input shape this pass does not design (unchanged deferral from
  ADR-0001).

## Deferred to a later ADR / explicit open questions

- **`mediaway-ffi`'s `gpu_device`/`max_path_class`** — this ADR's
  `mediaway_gpu_device_handle_t`/`mediaway_gpu_buffer_handle_t` are the real,
  intended future beneficiary named in `mediaway-ffi/adr/0001` §1;
  designing that crate's own config/encode-path changes is explicitly out of
  scope here.
- **`Window` capture (`WindowsWindowCapture`)** — still needs a native
  `HWND`/`NativeHandle` C input shape; this ADR removes the GPU-handle
  blocker but does not design that constructor.
- **`DXGI_ERROR_ACCESS_LOST` silently swallowed in the shared driver thread**,
  never surfaced as `CaptureError::DeviceLost` — a `mediaway-device-windows`
  fix; no `MEDIAWAY_DEVICE_STATUS_DEVICE_LOST` is added in this pass since no
  current code path can produce it.
- **ABA on the second-attacher device-liveness check** (§ Decision 2) — the
  shared-session registry compares only raw pointer bits; a stale/freed
  device aliasing a live session's stored bits is undetected. Not fixed here.
- **Cross-`ID3D11Device` sharing / `OpenSharedResource`** — already deferred
  by `mediaway-device-windows` ADR-0006; unaffected by this ADR.
- **`cbindgen` migration** — [`docs/adr/0016-cbindgen-ffi-headers.md`](../../../docs/adr/0016-cbindgen-ffi-headers.md)
  targeted this crate's header already; this ADR's additions grow the
  hand-written surface further before that migration happens, tracked in
  `docs/roadmap.md`.
- **Shared C header text** for `mediaway-common-ffi`'s value types (still
  textually duplicated per crate, per ADR-0015 § Deferred) — this ADR adds a
  *third* pair of types (`gpu_device`/`gpu_buffer`) to that duplication list.
- **`mediaway-common-ffi/docs/roadmap.md`** needs a new stage entry pointing
  at this ADR once implemented (its "Out of scope (by design, not a gap)"
  section currently has no GPU-handle row).
- **Linux Screen capture** (`mediaway-device-linux`'s portal/PipeWire
  backend) — this ADR's `#[cfg(not(windows))]` dispatch arm stays
  `CaptureError::NoBackend`; a real Linux GPU-handle story (likely `Vulkan`
  or `DirectXShared`-equivalent) is a separate, unverified follow-up.

## References

- [`crates/mediaway-common/src/gpu.rs`](../../mediaway-common/src/gpu.rs), [`frame.rs`](../../mediaway-common/src/frame.rs) — `GpuDeviceHandle`/`GpuBufferHandle`/`NativeHandle`/`VideoFrameStorage` (verified)
- [`crates/mediaway-device/src/video.rs`](../../mediaway-device/src/video.rs) — `VideoCaptureConfig`, `VideoCapture` trait, `capture_next_frame_blocking`, `capture_video_once` (Finding A fixed here)
- [`crates/mediaway-device/adr/0006-capture-once-screenshot.md`](../../mediaway-device/adr/0006-capture-once-screenshot.md) — single-shot capture design this ADR's `poll_frame_blocking` composes with instead of `capture_once`
- [`crates/mediaway-device-windows/src/dxgi.rs`](../../mediaway-device-windows/src/dxgi.rs), [`dxgi_shared.rs`](../../mediaway-device-windows/src/dxgi_shared.rs) — real `WindowsScreenCapture` backend
- [`crates/mediaway-device-windows/adr/0006-shared-desktop-duplication.md`](../../mediaway-device-windows/adr/0006-shared-desktop-duplication.md) — shared/refcounted session design this ADR's hazards (§8.2–8.3) build on
- [`crates/mediaway-device-ffi/adr/0001-capture-c-abi.md`](0001-capture-c-abi.md) — first pass; Finding 2 / § Deferred this ADR resolves
- [`crates/mediaway-device-ffi/adr/0002-callback-event-delivery.md`](0002-callback-event-delivery.md) — `mediaway_device_event_t`'s flat-struct-not-union precedent this ADR reuses
- [`crates/mediaway-ffi/adr/0001-auto-encode-c-abi.md`](../../mediaway-ffi/adr/0001-auto-encode-c-abi.md) §1 — the sibling deferral this ADR's shared type is meant to eventually unblock (not designed here)
- [`docs/adr/0015-common-ffi-unification.md`](../../../docs/adr/0015-common-ffi-unification.md) — `mediaway-common-ffi` placement precedent/criteria this ADR applies
- [`crates/mediaway-common-ffi/src/{types.rs,buffer.rs,lib.rs}`](../../mediaway-common-ffi/src) — existing shared-type module shape the new `gpu.rs` module mirrors
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md), [`docs/spec/c-ffi.md`](../../../docs/spec/c-ffi.md) — workspace policy this ADR concretizes
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) — honest-cost/hazard documentation requirement (§8)
- [`docs/ai/wiki/device/{windows-capture.md,ffi-c-abi.md,capture-once.md}`](../../../docs/ai/wiki/device) — wiki pages to update on implementation

ADRs are **English**. Numbering is local to this `adr/` folder.
