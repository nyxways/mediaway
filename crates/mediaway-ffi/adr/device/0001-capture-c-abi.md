# ADR-0001: Camera + microphone capture C ABI surface (first pass)

- **Status**: Proposed
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-ffi`

## Context

`mediaway-device-ffi` is the **third** `mediaway-*-ffi` crate in the workspace,
after `mediaway-container-ffi` (mux/demux) and `mediaway-ffi` (auto
encode). It wraps the real capture surface in
[`crates/mediaway-device/src/{video.rs,audio.rs,error.rs}`](../../mediaway-device/src)
(`VideoCapture`/`AudioCapture` traits, `VideoCaptureConfig`/`AudioCaptureConfig`,
`CaptureError`) and the concrete Windows implementations in
[`crates/mediaway-device-windows/src/{camera.rs,wasapi.rs,dxgi.rs,wgc.rs}`](../../mediaway-device-windows/src).

Neither crate exists in this ADR's assumed shape by default — the design brief
that produced this ADR expected camera capture might be purely aspirational
(no real backend) and screen capture to be the safe, obviously-real default.
**Reading the actual source flips that assumption.** Both findings below are
load-bearing for this ADR's scope decision and are stated up front:

### Finding 1 — Camera capture is real, hardware-verified, and CPU-only

[`crates/mediaway-device-windows/src/camera.rs`](../../mediaway-device-windows/src/camera.rs)
implements `WindowsCameraCapture::open` via Media Foundation
`IMFSourceReader`/`MFEnumDeviceSources`, is wired into the crate's public API
(`mod camera;` + `pub use camera::WindowsCameraCapture;` in
[`lib.rs`](../../mediaway-device-windows/src/lib.rs)), and its hardware test
(`camera_tests.rs::open_camera_capture_frames_or_skip`) captured real
1920x1080 frames from a physical "WeVO WV-1080" USB webcam
per this crate's own roadmap (Stage 4). Output is always
`VideoFrameStorage::Cpu` (module docs: no DX11 Zero-Copy path yet).

**Three existing docs disagree with this and are stale**, found while
verifying the aspirational examples against source:

- [`docs/ai/wiki/device/camera-device-handle.md`](../../../docs/ai/wiki/device/camera-device-handle.md)
  said "No backend implements camera capture yet."
- `crates/mediaway-device-windows/docs/roadmap.md` Stage 4 said "Not yet wired
  into this crate's public API ... no `mod camera;`/`pub use` in `lib.rs`" —
  contradicted by the `lib.rs` lines cited above.
- `mediaway_device::capability::Unavailable::NotImplemented`'s doc comment
  cites "`Camera` today" as an example of a kind with no backend on any
  platform.

The wiki page is corrected as part of this ADR's wiki upkeep (§ References);
the roadmap checklist wording and the Rust doc comment are flagged as
follow-ups against their own crates, not edited here (out of scope: this pass
only adds one ADR file, no crate-source edits per the task brief).

### Finding 2 — Screen and window capture are real but have **no CPU fallback**

[`crates/mediaway-device-windows/src/dxgi.rs`](../../mediaway-device-windows/src/dxgi.rs)
(`WindowsScreenCapture::open`) and
[`wgc.rs`](../../mediaway-device-windows/src/wgc.rs) (`WindowsWindowCapture::open`)
both hard-reject `CaptureOutputPreference::CpuFramesOk` with
`CaptureError::Unsupported`, and both **require** the caller to already supply
a live `GpuDeviceHandle::DirectX11(ID3D11Device*)` in
`VideoCaptureConfig::gpu_device` — there is no code path in either backend
that ever produces a CPU frame. Both are real, hardware-capable, DX11
Zero-Copy screen/window capture (per
[`docs/ai/wiki/device/windows-capture.md`](../../../docs/ai/wiki/device/windows-capture.md),
[`windows-window.md`](../../../docs/ai/wiki/device/windows-window.md)) — but
neither can be reached from C in this pass, because handing a live GPU device
pointer across the C boundary is the same unsolved problem
`mediaway-ffi/adr/0001` §1 already deferred for
`AutoVideoEncodeConfig::gpu_device` ([`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md)),
and here it blocks the *entire* capability, not just its fastest path.

Consequence: **this ADR's central scope decision is the inverse of the
brief's assumption** — Camera and Microphone/Loopback/ProcessLoopback (all
CPU-only, all real, all hardware-verified) are in scope for v1; Screen and
Window (also real, but GPU-handle-gated with no fallback) are deferred.

### Three things this ADR must decide that differ from the first two crates

1. **Dependency shape.** `mediaway-device` (the facade) intentionally carries
   no platform dependency ([ADR-0002](../../mediaway-device/adr/0002-facade-platform-boundary.md)).
   Cross-platform dispatch for capture exists today only inside
   `mediaway::platform::{ScreenCapture, Microphone}` — and that
   module has **no** `Camera` or `Window` dispatcher at all. Whether this
   crate reuses `mediaway`'s dispatch or writes its own.
2. **A second, structurally different "frame" struct is needed** —
   `mediaway-ffi` already shipped `mediaway_video_frame_t` as a
   **borrowed input** (`const uint8_t *raw_bytes`) for `write_frame`; this
   crate's `poll_frame` output is the opposite direction, an **owned** buffer
   that needs a matching free function. Reusing the same struct name for a
   different ownership contract is a real collision, not a hypothetical one.
3. **`PixelFormat`/`Rational` already have one canonical C definition**
   (`mediaway_pixel_format_t`, `mediaway_rational_t`, both defined in
   `mediaway-ffi` wrapping the exact same shared
   `mediaway_common` Rust types) — whether to reuse those names verbatim or
   mint crate-scoped duplicates, given the same-name-different-shape hazard
   just found in point 2 above cuts the other way for value-identical enums.

This ADR reuses `mediaway-container-ffi/adr/0001`'s and
`mediaway-ffi/adr/0001`'s established patterns (single-`Box` opaque
handles, `catch_unwind` + per-handle `poisoned` flag, hand-written header,
borrowed-input/owned-output+`_free` memory rule, crate-scoped status enum)
and states plainly where it deviates.

## Decision

> v1 ships **Camera** (video) and **Microphone / Loopback / ProcessLoopback**
> (audio) capture — all real, CPU-only, hardware-verified Windows backends.
> **Screen and Window capture are deferred**, not as unimplemented
> aspirations but as a real capability blocked on a GPU-handle-crossing-C-ABI
> design this pass does not attempt. This crate depends directly on
> `mediaway-device` + `#[cfg(windows)] mediaway-device-windows` (+
> `#[cfg(target_os = "linux")] mediaway-device-linux` for the same shape,
> unverified this pass), **not** on `mediaway`, to avoid that
> crate's documented unconditional decode/encode/device dependency graph.
> New, crate-scoped names for owned-output frame structs and the status
> enum; verbatim reuse of `mediaway_rational_t`/`mediaway_pixel_format_t`
> from `mediaway-ffi` since both wrap the identical shared Rust
> type. Hand-write the header.

### 1. Dependency shape — thin local dispatch, not `mediaway`

```toml
[dependencies]
mediaway-common  = { workspace = true }
mediaway-device  = { workspace = true }

[target.'cfg(windows)'.dependencies]
mediaway-device-windows = { workspace = true }

[target.'cfg(target_os = "linux")'.dependencies]
mediaway-device-linux = { workspace = true }
```

This crate's `lib.rs` contains a small `#[cfg(windows)]`/`#[cfg(target_os =
"linux")]` dispatch (open a `Camera`/`Microphone`/`Loopback`/`ProcessLoopback`
config against the matching backend type, `Err(CaptureError::NoBackend)`
otherwise) — the same shape `mediaway_pipeline::platform::ScreenCapture`/
`Microphone` already use, reimplemented locally rather than imported.

**Alternative considered and rejected:** depend on
`mediaway_pipeline::platform::{ScreenCapture, Microphone}` directly, reusing
the dispatch that already exists. Rejected for two independent reasons:

- `mediaway-ffi/adr/0001` §9 already documents that
  `mediaway`'s `Cargo.toml` depends **unconditionally** on
  `mediaway-decoder`, `mediaway-device`, and their Windows/Linux platform
  backends, with no Cargo feature to select against — a capture-only FFI
  crate would compile and link WMF video decode plus every other
  `mediaway` capability it never calls, purely to reach two
  dispatch functions. This is exactly the "one fat link graph" the C-FFI
  design rules (`docs/spec/c-ffi.md`, AGENTS.md § C-FFI) say to avoid.
- `mediaway_pipeline::platform` has **no** `Camera` dispatcher at all today —
  reusing it would still require adding new dispatch code somewhere for the
  one capability (Camera) this crate's own docs (§ Finding 1) confirm is the
  most immediately usable, real, hardware-verified surface. There is nothing
  to "reuse" for Camera; it would have to be written either way.

Writing ~30 lines of `#[cfg]` dispatch locally (mirroring an existing,
already-reviewed pattern) is cheaper than adding a heavy, unrelated
transitive dependency to save writing that dispatch once.

### 2. Opaque handles — trait objects, not a closed backend enum

```rust
// mediaway-device-ffi internal representation (never exposed to C).
struct VideoCaptureHandle {
    poisoned: bool,
    inner: Box<dyn mediaway_device::VideoCapture>,
}

struct AudioCaptureHandle {
    poisoned: bool,
    inner: Box<dyn mediaway_device::AudioCapture>,
}
```

Unlike `mediaway-container-ffi`'s `MuxerState` (a closed enum of concrete
typestate variants) or `mediaway-ffi`'s `AutoEncoderHandle` (a bare
`Box<dyn VideoEncoder>` with **no** wrapper struct), both handles here need
**both** a trait object *and* a `poisoned` flag:

- A trait object, not an enum-of-concrete-backends, because `VideoCapture`/
  `AudioCapture` are the facade's actual dispatch mechanism (`Box<dyn
  VideoCapture>` is exactly what `mediaway_pipeline::platform::ScreenCapture::open`
  already returns) — adding a second, FFI-private enum mirroring the same
  choice would be redundant. This also means Screen/Window support, once a
  GPU-handle ABI exists, is purely a new dispatch arm inside
  `mediaway_video_capture_open` (§4) — the handle shape does not change.
- A `poisoned` flag, unlike `AutoEncoderHandle`, because both handles here
  are the **repeated-call** kind (`poll_frame`/`release_frame` called in a
  loop, potentially hundreds of times per session) — the same shape as
  `MuxerHandle`/`DemuxerHandle`/`EncodeSessionHandle`, not the
  "consumed-in-one-call" shape `AutoEncoderHandle` was designed for.

`mediaway_video_capture_t`/`mediaway_audio_capture_t` are forward-declared
incomplete C structs, same as every prior `-ffi` crate's handles.

No `INVALID_STATE` status value is needed: like `EncodeSession`, `VideoCapture`/
`AudioCapture` have no caller-visible illegal-transition beyond "already
closed" (already covered by `CLOSED`).

### 3. Status enum — fresh, distinctly-named type

```c
typedef enum mediaway_device_status {
    MEDIAWAY_DEVICE_STATUS_OK                = 0,
    MEDIAWAY_DEVICE_STATUS_INVALID_ARGUMENT  = 1,  /* null pointer, mismatched ptr/len */
    MEDIAWAY_DEVICE_STATUS_HANDLE_POISONED   = 2,  /* a previous call on this handle panicked */
    MEDIAWAY_DEVICE_STATUS_UNSUPPORTED       = 3,  /* CaptureError::Unsupported — includes Screen/Window in this pass, see Decision */
    MEDIAWAY_DEVICE_STATUS_NO_BACKEND        = 4,  /* CaptureError::NoBackend */
    MEDIAWAY_DEVICE_STATUS_INVALID_INPUT     = 5,  /* CaptureError::InvalidInput */
    MEDIAWAY_DEVICE_STATUS_BACKEND_FAILURE   = 6,  /* CaptureError::Backend */
    MEDIAWAY_DEVICE_STATUS_CLOSED            = 7,  /* CaptureError::Closed */
    MEDIAWAY_DEVICE_STATUS_ACCESS_DENIED     = 8,  /* CaptureError::AccessDenied */
    MEDIAWAY_DEVICE_STATUS_UNKNOWN_ERROR     = 9,  /* CaptureError is #[non_exhaustive]; catch-all */
    MEDIAWAY_DEVICE_STATUS_INTERNAL_PANIC    = 10, /* this call caught a Rust panic; handle now poisoned */
} mediaway_device_status_t;
```

Distinctly named from `mediaway_status_t` (container-ffi) and
`mediaway_pipeline_status_t` (pipeline-ffi) for the same reason both of those
gave each other: two separately compiled libraries, no shared header, and
the enumerator sets are not identical (`CaptureError`'s 6 variants have no
direct analog in either sibling crate's error type) — reusing a name would
be a latent duplicate-typedef hazard the moment a consumer includes more
than one of these headers. Not numerically mirrored onto either sibling for
the same reason `mediaway_pipeline_status_t` gave: these wrap a third,
structurally distinct Rust error enum, so pretending value-compatibility
would misleadingly imply a cross-library contract that doesn't exist.

`HANDLE_POISONED` is placed early (position 2), matching
`mediaway_pipeline_status_t`'s ordering rationale: it and
`INVALID_ARGUMENT` are both checked at the very start of every call, before
any domain logic runs.

### 4. Function list

```c
uint32_t mediaway_device_ffi_abi_version(void);

/* — video capture config (plain value struct, no handle, no free) — */
mediaway_video_capture_config_t mediaway_video_capture_config_camera(
    uint32_t device_index, mediaway_rational_t time_base);
mediaway_video_capture_config_t mediaway_video_capture_config_screen(
    uint32_t output_index, mediaway_rational_t time_base);
/* Kept (matches the aspirational example's call site) but mediaway_video_capture_open
 * on a Screen-kind config always returns MEDIAWAY_DEVICE_STATUS_UNSUPPORTED in this
 * pass — see §6 Corrections (b) and § Deferred. Not removed, so the full real
 * CaptureSource enum stays representable in C (c-ffi.md rule 1: "map existing Rust
 * surfaces; do not invent C-only capabilities" — the inverse also applies: do not
 * silently drop a real Rust surface from the C enum just because it isn't reachable
 * yet). mediaway_video_capture_config_window() is intentionally not added — Window
 * additionally needs a native HWND input this pass has no consumer-facing use for
 * yet; add together with the GPU-handle follow-up.
 */

/* — video capture — */
mediaway_device_status_t mediaway_video_capture_open(
    const mediaway_video_capture_config_t *config, mediaway_video_capture_t **out_capture);
mediaway_device_status_t mediaway_video_capture_geometry(
    const mediaway_video_capture_t *capture, uint32_t *out_width, uint32_t *out_height);
mediaway_device_status_t mediaway_video_capture_poll_frame(
    mediaway_video_capture_t *capture, mediaway_device_video_frame_t *out_frame,
    bool *out_has_frame);
mediaway_device_status_t mediaway_video_capture_release_frame(mediaway_video_capture_t *capture);
mediaway_device_status_t mediaway_video_capture_close(mediaway_video_capture_t *capture);

/* — audio capture config — */
mediaway_audio_capture_config_t mediaway_audio_capture_config_microphone(
    mediaway_rational_t time_base);
mediaway_audio_capture_config_t mediaway_audio_capture_config_loopback(
    mediaway_rational_t time_base);
mediaway_audio_capture_config_t mediaway_audio_capture_config_process_loopback(
    uint32_t process_id, bool include_child_processes, mediaway_rational_t time_base);

/* — audio capture — */
mediaway_device_status_t mediaway_audio_capture_open(
    const mediaway_audio_capture_config_t *config, mediaway_audio_capture_t **out_capture);
mediaway_device_status_t mediaway_audio_capture_format(
    const mediaway_audio_capture_t *capture, uint32_t *out_sample_rate, uint16_t *out_channels);
mediaway_device_status_t mediaway_audio_capture_poll_frame(
    mediaway_audio_capture_t *capture, mediaway_device_audio_frame_t *out_frame,
    bool *out_has_frame);
mediaway_device_status_t mediaway_audio_capture_close(mediaway_audio_capture_t *capture);

/* — owned output frees (each reads its own length off the struct, see §6) — */
void mediaway_device_video_frame_free(mediaway_device_video_frame_t *frame);
void mediaway_device_audio_frame_free(mediaway_device_audio_frame_t *frame);
```

No bare `mediaway_device_ffi_buffer_free`: unlike `mediaway_muxer_poll_bytes`/
`mediaway_encode_session_finish`, every owned output in this crate is already
wrapped in a struct that carries its own length field, so a struct-specific
free (reading `data_len` off the struct, nulling the pointer after freeing,
same convention as `mediaway_packet_free`/`mediaway_stream_info_free`) is
sufficient — adding an unused generic buffer-free function purely for naming
symmetry with the sibling crates would violate "no features beyond the
request" (AGENTS.md § Simplicity first).

`mediaway_audio_capture_format` and `mediaway_video_capture_geometry` are the
audio/video analogs of each other: both read a value the backend only knows
*after* negotiating with the OS (WASAPI's real mix-format sample rate/channel
count via `GetMixFormat`; DXGI/MF's real negotiated width/height) — "don't
assume a resolution"/"don't assume a format" is already the stated principle
in `dxgi.rs`'s and `camera.rs`'s module docs for geometry; `wasapi.rs`
negotiates sample rate/channels the exact same way, so this crate needs the
audio equivalent even though the aspirational `screen_record.c`/
`camera_record.c` examples never call it (their loop prints geometry but not
audio format — an omission in those examples, not evidence the real API
doesn't need it).

### 5. Struct layouts

```c
/* Reused verbatim from mediaway-ffi — both wrap mediaway_common::Rational /
 * mediaway_common::PixelFormat identically; see §7 for the reuse-vs-mint reasoning. */
typedef struct mediaway_rational {
    uint64_t num;
    uint32_t den;
} mediaway_rational_t;

typedef enum mediaway_pixel_format {
    MEDIAWAY_PIXEL_FORMAT_NV12  = 0,
    MEDIAWAY_PIXEL_FORMAT_I420  = 1,
    MEDIAWAY_PIXEL_FORMAT_BGRA8 = 2,
    MEDIAWAY_PIXEL_FORMAT_RGBA8 = 3,
    MEDIAWAY_PIXEL_FORMAT_YUYV  = 4,
} mediaway_pixel_format_t;

/* First definition of this enum in the workspace's C headers — mediaway_common::SampleFormat
 * has no prior FFI precedent, unlike Rational/PixelFormat. */
typedef enum mediaway_sample_format {
    MEDIAWAY_SAMPLE_FORMAT_S16 = 0,
    MEDIAWAY_SAMPLE_FORMAT_S32 = 1,
    MEDIAWAY_SAMPLE_FORMAT_F32 = 2,
} mediaway_sample_format_t;

typedef enum mediaway_device_video_source_kind {
    MEDIAWAY_DEVICE_VIDEO_SOURCE_SCREEN = 0, /* CaptureSource::Screen — open() -> UNSUPPORTED this pass */
    MEDIAWAY_DEVICE_VIDEO_SOURCE_WINDOW = 1, /* CaptureSource::Window — no constructor exposed this pass either (§4) */
    MEDIAWAY_DEVICE_VIDEO_SOURCE_CAMERA = 2, /* CaptureSource::Camera — supported this pass */
} mediaway_device_video_source_kind_t;

typedef struct mediaway_video_capture_config {
    mediaway_device_video_source_kind_t source_kind;
    uint32_t source_index;          /* output_index (Screen, unusable this pass) / device ordinal (Camera) */
    mediaway_rational_t time_base;
    /* No output-preference / gpu_device fields: Camera is always CpuFramesOk
     * internally in this pass (the only mode it accepts), and Screen/Window
     * cannot accept any gpu_device value this ABI could construct yet (§ Deferred) —
     * exposing unusable knobs would invite a config that looks configurable but
     * can never do anything different, the same reasoning mediaway-ffi/adr/0001
     * §1 used to hardcode max_path_class/backend rather than exposing them inertly. */
} mediaway_video_capture_config_t; /* plain value type; no free function */

typedef enum mediaway_device_audio_source_kind {
    MEDIAWAY_DEVICE_AUDIO_SOURCE_MICROPHONE       = 0,
    MEDIAWAY_DEVICE_AUDIO_SOURCE_LOOPBACK         = 1,
    MEDIAWAY_DEVICE_AUDIO_SOURCE_PROCESS_LOOPBACK = 2,
} mediaway_device_audio_source_kind_t;

typedef struct mediaway_audio_capture_config {
    mediaway_device_audio_source_kind_t source_kind;
    uint32_t device_index;             /* Microphone / Loopback endpoint index; ignored for ProcessLoopback */
    uint32_t process_id;                /* ProcessLoopback only */
    bool include_child_processes;       /* ProcessLoopback tree_scope; ignored otherwise */
    mediaway_rational_t time_base;
    mediaway_sample_format_t sample_format; /* only F32 accepted by the real Windows backend today (§6) */
} mediaway_audio_capture_config_t; /* plain value type; no free function */

/* Output of mediaway_video_capture_poll_frame — owned; release with
 * mediaway_device_video_frame_free. Distinct name from mediaway-ffi's
 * mediaway_video_frame_t (borrowed input there, owned output here — see §7). */
typedef struct mediaway_device_video_frame {
    int64_t pts;
    uint64_t duration;
    uint32_t width;
    uint32_t height;
    mediaway_pixel_format_t pixel_format;
    uint8_t *data;      /* owned; NULL after mediaway_device_video_frame_free */
    size_t data_len;
} mediaway_device_video_frame_t;

/* Output of mediaway_audio_capture_poll_frame — owned; release with
 * mediaway_device_audio_frame_free. No prior audio-frame struct exists in any
 * workspace C header yet — first definition. */
typedef struct mediaway_device_audio_frame {
    int64_t pts;
    uint64_t duration;
    uint32_t sample_rate;
    uint16_t channels;
    mediaway_sample_format_t sample_format;
    uint8_t *data;      /* owned; NULL after mediaway_device_audio_frame_free */
    size_t data_len;
} mediaway_device_audio_frame_t;
```

### 6. Memory ownership

- **`poll_frame` (video/audio, output):** `VideoFrame.storage.Cpu.data` and
  `AudioFrame.data` are both `bytes::Bytes` on the Rust side — refcounted,
  cheap to clone *within* Rust. Same as `mediaway-container-ffi`'s
  `Packet.payload` and `mediaway-ffi`'s frame handling, C has no
  refcounted-buffer concept to hand across without inventing one, so the FFI
  layer copies once (`Bytes::to_vec()` → `into_boxed_slice()` →
  `Box::into_raw()`) into a fresh owned allocation, freed via the matching
  `_free` function. **This is the third instance of the identical deferred
  CPU Zero-Copy opportunity** both sibling ADRs already logged for their own
  `Bytes`-backed outputs — no new reasoning needed, but the repetition across
  three crates now is its own argument for prioritizing a shared, explicit
  CPU-side Zero-Copy buffer-handle ABI type (§ Deferred).
- **`release_frame` (video only):** matches `VideoCapture::release_frame`
  1:1. For `WindowsCameraCapture` specifically this is a **documented no-op**
  (CPU-owned frames hold no backend resource — the copy already happened on
  the worker thread) — callers must still call it, both because the trait
  contract requires it before the next `poll_frame` that acquires a new
  frame, and because a future Screen/Window backend behind the same handle
  type *would* need the real DXGI/WGC `ReleaseFrame` call this convention
  reserves the slot for.
- **`_free` functions read `data_len`/dimensions off the struct itself**,
  nulling the pointer/length after freeing (same double-free-becomes-a-no-op
  convention as `mediaway_packet_free`/`mediaway_stream_info_free`).
- **Config structs:** plain values, no heap allocation, no free function —
  same as `mediaway_auto_video_encode_config_t`.

### 7. `Rational`/`PixelFormat` reuse vs. the new frame-struct names

Two different collision risks came up in this pass, resolved two different
ways:

| Type | Decision | Why |
|------|----------|-----|
| `mediaway_rational_t` | **Reuse verbatim** (same name, same fields) from `mediaway-ffi` | Identical shape, wrapping the identical shared `mediaway_common::Rational`; `mediaway-ffi/adr/0001` §5 already reused this same definition from `mediaway-container-ffi` without renaming — this is the second reuse of an already-established precedent, not a new one. |
| `mediaway_pixel_format_t` | **Reuse verbatim** from `mediaway-ffi` | Same reasoning; `mediaway-ffi/adr/0001` §5 explicitly called this "the first definition of this enum in the workspace's C headers — no mirroring precedent to reconcile against," i.e. it was written expecting reuse, not per-crate redefinition. |
| `mediaway_video_frame_t` (already shipped by `mediaway-ffi`) | **Do not reuse** — mint `mediaway_device_video_frame_t` | Same struct *name* would be reused for a **different ownership direction** (borrowed `const uint8_t *raw_bytes` input there vs. owned `uint8_t *data` output here) — a genuine field-shape collision, not merely a duplicate-typedef risk. `mediaway-container-ffi/adr/0001` §4c hit exactly this problem for muxer/demuxer packets and split into two differently-named/shaped structs; the same fix applies across crates here. |

**Open risk not resolved by this ADR — confirmed worse than assumed above.**
This ADR originally claimed reusing `mediaway_pixel_format_t`/`mediaway_rational_t`
verbatim across two independently compiled headers was safe for a **C**
translation unit that includes both, citing C11 §6.7p3, and only doubtful for
C++. **That C claim was wrong**, confirmed empirically during this crate's own
real link+run verification pass (`docs/roadmap.md` Stage 4): `gcc -std=c11`
rejects `#include <mediaway/device.h>` together with `<mediaway/pipeline.h>`
in one translation unit with a hard redefinition error — C11 §6.7p3's
"compatible redefinition" allowance applies to repeated *tentative*
declarations of the same tag, not two full `typedef struct { ... } name;`
definitions with bodies in the same translation unit, which is what both
headers independently do. So this hazard is real in **plain C**, not merely
an unconfirmed C++ risk. `bindings/c/examples/camera_record.c` (the first
consumer to need both a capture header and an encode header at once) hit this
immediately; worked around there by not including `pipeline.h` at all and
hand-declaring the small surface it needs instead (see that file's own
comments). This is now the **third** instance of a cross-crate C-type-sharing
question this workspace's `-ffi` crates have hit (after
`mediaway_status_t`-vs-`mediaway_pipeline_status_t` naming and the two
crates' independent `_buffer_free` functions) — and the first one with a
demonstrated, not merely theoretical, compile failure. Logged under
§ Deferred as added pressure toward actually starting `mediaway-common-ffi`
with a genuinely shared (not independently-redefined) header for these two
value types, not solved here.

### 8. Panic safety

Identical `catch_unwind(AssertUnwindSafe(...))` strategy to both prior
crates, applied to every exported function. Both `VideoCaptureHandle` and
`AudioCaptureHandle` carry a `poisoned` flag (§2) — a caught panic during
`poll_frame`/`release_frame`/`geometry`/`format` sets it, and every
subsequent call short-circuits to `MEDIAWAY_DEVICE_STATUS_HANDLE_POISONED`,
**except** `mediaway_*_capture_close`, always safe to call. Null/argument
checks happen before entering `catch_unwind`, returning
`MEDIAWAY_DEVICE_STATUS_INVALID_ARGUMENT` directly. `open()` distinguishes
three outcomes (normal `Ok`, normal `Err`, caught panic) with the same
`NULL`-on-failure shape both sibling ADRs already established.

Allocator OOM and the default panic hook are out of scope, same as both
priors.

### 9. Thread safety

Handles are thread-confined by convention (same documentation obligation as
both sibling ADRs: moving a handle to another thread is fine, concurrent
calls on the same handle pointer from two threads without external
synchronization is a data race). **Crate-specific addition:** every backend
this crate wraps (`WindowsCameraCapture`, `WindowsWasapiCapture`) runs a
background OS-callback/worker thread internally that fills a bounded,
`Mutex`-guarded queue — `poll_frame` itself is cheap (a queue pop), but
`mediaway_*_capture_close` **joins that worker thread** and can block for up
to one frame/period interval (documented in `camera.rs`'s/`wasapi.rs`'s own
`close()` comments) — this must be stated as a real, non-instantaneous cost
on the header's `close` functions, not left implicit (`docs/spec/caveats-and-clarity.md`).

### 10. Header authoring

**Hand-written** `include/mediaway/device.h`, same reasoning as both sibling
ADRs (opaque handles, a struct split a mechanical translation wouldn't infer,
`cbindgen` still not justified for one more evolving header). This is now
the **third** hand-written header in the workspace; both sibling ADRs
deferred a `cbindgen` evaluation "once ≥2 headers exist" — that condition
has been true since `mediaway-ffi`, and is now true a second time
over. Not decided in this ADR (still someone else's call to make
unilaterally for three sibling crates at once), but flagged as increasingly
overdue.

Same version-macro + runtime-accessor convention:
`MEDIAWAY_DEVICE_FFI_ABI_VERSION` (compile-time) +
`mediaway_device_ffi_abi_version()` (runtime).

### 11. Feature flags

`[features]` table mirroring `mediaway-container-ffi`'s `mux`/`demux` split:
`default = ["video", "audio"]`, gating `mediaway_video_capture_*`/
`mediaway_audio_capture_*` symbols independently behind
`#[cfg(feature = "video")]`/`#[cfg(feature = "audio")]` — a genuinely useful
split here (e.g. a mic-only voice-note tool never needs camera code, or vice
versa), unlike `mediaway-ffi`'s single always-on surface. This
crate's own dependency shape (§1, no `mediaway`) also means there is
no unconditional-transitive-dependency gap analogous to the ones both prior
ADRs flagged against their own facade crates' `Cargo.toml`s — the dependency
graph this crate adds is already minimal by construction.

## Corrections to the aspirational examples

`bindings/c/examples/screen_record.c` and `camera_record.c` are written
before this ADR — consumer-side input, not a binding decision, same status
`mux_roundtrip.c`/`encode_to_mp4.c` had for the first two crates.

| # | Aspirational sketch | Problem | Correction |
|---|---|---|---|
| a | `mediaway_video_capture_poll_frame(video)` returns a bare `MEDIAWAY_POLL_FRAME_READY`/`NO_FRAME`/`ERROR` enum with no way shown to read the real polled frame's bytes (the example builds a synthetic placeholder frame instead, admitted in its own comments) | `VideoCapture::poll_frame`'s real signature returns `Result<Option<VideoFrame>, CaptureError>` in one call — there is no separate "get the frame I was just told is ready" accessor to invent | Combine status + owned frame + has-frame flag into one call, `mediaway_video_capture_poll_frame(capture, out_frame, out_has_frame) -> status` — same shape as `mediaway_demuxer_poll_packet` in `mediaway-container-ffi` |
| b | `mediaway_video_capture_config_screen(0, tb)` shown as if it opens successfully with no GPU device supplied | `WindowsScreenCapture::open` requires `Some(GpuDeviceHandle::DirectX11(...))` and rejects `CpuFramesOk` outright — no code path produces a usable session from this call today | `mediaway_video_capture_open` on a Screen-kind config deterministically returns `MEDIAWAY_DEVICE_STATUS_UNSUPPORTED` in this pass (§ Finding 2, § Deferred) — not silently "successful but broken" |
| c | Same missing-accessor problem as (a), for `mediaway_audio_capture_poll_frame(mic)` — the loop drains poll results but never reads PCM bytes (`TODO(#issue): push to an audio encoder` in both example files) | Same root cause as (a) | Same fix as (a): `mediaway_audio_capture_poll_frame(capture, out_frame, out_has_frame) -> status` |
| d | Neither example queries the real negotiated sample rate/channel count for the opened microphone | `WindowsWasapiCapture` negotiates its actual sample rate/channels from `IAudioClient::GetMixFormat` — not something the caller's config dictates, the same "don't assume, query it" principle the examples already apply to video geometry | Add `mediaway_audio_capture_format` (§4) — an addition, not a correction of something the sketch got wrong, since the sketch simply never attempted this |
| e | `.den = (int32_t)fps` literal (same file `screen_record.c`/`camera_record.c` reuse from `encode_to_mp4.c`) | Already found and fixed twice (`mediaway-container-ffi/adr/0001` §4d, `mediaway-ffi/adr/0001` §4c) — `mediaway_common::Rational` is `{num: u64, den: u32}` | Reuse the already-corrected `mediaway_rational_t` verbatim (§7); no new finding here |

Everything else — the config → open → poll/release loop → close lifecycle
shape, `mediaway_video_capture_geometry`, mic-may-be-`NULL`-and-recording-
continues-without-audio handling, the overall two-device (`video`/`mic`)
`record()` structure — is adopted as-is; it matches the real trait shapes
directly.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Ship Screen/Window capture in v1 with a raw `void *` GPU device pointer field | Would informally solve the exact GPU-handle-crossing-C-ABI problem both this ADR and `mediaway-ffi/adr/0001` say is unsolved, without the design rigor (ownership, lifetime, device-vs-buffer distinction, multi-backend `GpuDeviceHandle` variants) that problem deserves — a shortcut that would need to be redone properly later anyway |
| Depend on `mediaway::platform::{ScreenCapture, Microphone}` for cross-platform dispatch | Pulls in `mediaway`'s documented unconditional decode/encode/device/platform-backend dependency graph for a capture-only crate (§1); also has no `Camera` dispatcher to reuse in the first place |
| Reuse `mediaway_video_frame_t`'s name for this crate's poll output | Real field-shape collision: borrowed `const uint8_t*` (pipeline-ffi, input) vs. owned `uint8_t*` (this crate, output) under the same type name — same class of bug `mediaway-container-ffi/adr/0001` §4c already found and fixed for packets |
| Mint a fresh, crate-scoped `mediaway_device_pixel_format_t`/`mediaway_device_rational_t` instead of reusing pipeline-ffi's | Both wrap the identical shared `mediaway_common` Rust type end-to-end with identical shape; `mediaway-ffi/adr/0001` explicitly wrote `mediaway_pixel_format_t` expecting reuse, not per-crate redefinition — minting a duplicate would fragment the one case where cross-crate sharing was already the stated intent |
| `cbindgen`-generated header | Same reasoning as both sibling ADRs — this pass's struct-split/config-hardcoding decisions are not a mechanical translation; revisit once a dedicated shared-tooling ADR exists |
| Expose `output`/`gpu_device` fields on `mediaway_video_capture_config_t` even though unusable this pass | Would present a config knob that looks meaningful but can never select a working path yet — same reasoning `mediaway-ffi/adr/0001` §1 used to hardcode rather than inertly expose `max_path_class`/`backend` |

## Consequences

### Positive

- Concrete, reviewable ABI surface for the third `-ffi` crate, covering
  every real, hardware-verified, CPU-only capture surface in the workspace
  today (Camera + Microphone + Loopback + ProcessLoopback).
- Corrected three stale docs found while verifying the aspirational examples
  against source (§ Context Finding 1) — the wiki page is fixed as part of
  this ADR's own wiki upkeep; the roadmap checklist and a Rust doc comment
  are flagged as follow-ups.
- A concrete, load-bearing precedent that "real in Rust" and "reachable
  from C" are independent questions — Screen/Window capture being real,
  hardware-verified, and Zero-Copy-capable in Rust does not make them
  C-ABI-ready; the blocker (a live GPU device pointer with no CPU fallback)
  is now documented precisely, not hand-waved.
- Establishes that `mediaway_pixel_format_t`/`mediaway_rational_t` are
  genuinely reusable canonical C types across `-ffi` crates when they wrap
  the same shared Rust type, while frame-direction-specific structs are not
  — a distinction future `-ffi` crates can now follow directly instead of
  re-deriving.

### Negative / Trade-offs

- Screen and window capture — the two Zero-Copy, most performance-relevant
  capture paths in the whole `mediaway-device` stack — are **not** reachable
  from C in this pass at all, not even via a documented copy/readback
  fallback, because no such fallback exists in the wrapped Rust backends
  themselves. This is a real capability gap, not merely a C-ABI omission.
- A third independently-named/numbered status enum
  (`mediaway_device_status_t`) and a third pair of frame-struct names now
  exist across the workspace's three `-ffi` crates, alongside two already-
  independent buffer-free functions — `mediaway-common-ffi` remains
  unstarted while the fragmentation it is meant to resolve keeps growing
  (§7's open risk is the sharpest instance of this yet: reusing
  `mediaway_pixel_format_t` verbatim is *probably* fine in C, but not
  verified safe in C++).
- This crate's own dispatch code duplicates, rather than reuses,
  the shape of `mediaway_pipeline::platform::{ScreenCapture, Microphone}` —
  a deliberate trade-off (§1), but real duplication nonetheless.

## Deferred to a later ADR / explicit open questions

- **Screen (`WindowsScreenCapture`) and Window (`WindowsWindowCapture`)
  capture** — real, hardware-verified, Zero-Copy Rust backends with no C ABI
  path yet; blocked on a `GpuDeviceHandle` C representation
  (`docs/spec/gpu-interop.md`), the same open problem
  `mediaway-ffi/adr/0001` deferred for `AutoVideoEncodeConfig::gpu_device`.
  A `Window`-specific follow-up also needs an `HWND`/`NativeHandle` C input
  shape not designed here.
- ~~**`mediaway-common-ffi`**~~ — resolved by
  [`docs/adr/0015-common-ffi-unification.md`](../../../docs/adr/0015-common-ffi-unification.md):
  this crate's `Rational` mirror now re-exports the shared
  `mediaway_common_ffi::types::Rational` (its C-facing `mediaway_rational_t`
  name is unchanged). Partial only — status enums and header text stay
  independent by that ADR's own decision, so the real plain-C redefinition
  hazard from co-including `device.h` and `pipeline.h` (confirmed during this
  crate's own link+run verification, §7) is **not** fixed by that migration;
  `mediaway_pixel_format_t` in particular is still copy-pasted per-crate
  (out of ADR-0015's scope, Rational/CodecKind only) and remains a real,
  open duplicate-typedef risk. A follow-up would need either shared header
  text or `cbindgen` (see the next bullet) to actually close it.
- **`cbindgen` adoption** — [`docs/adr/0016-cbindgen-ffi-headers.md`](../../../docs/adr/0016-cbindgen-ffi-headers.md)
  (drafted in parallel with this ADR) decided to adopt `cbindgen` **starting
  with `mediaway-device-ffi`** specifically, to avoid a third hand-written
  header. Because both ADRs were drafted concurrently, this crate's `include/mediaway/device.h`
  was already hand-written and hardware-link-verified before ADR-0016
  concluded — a real sequencing gap, not a silent contradiction: this crate's
  header is the first, and so far only, concrete candidate for ADR-0016's
  migration, tracked in `docs/roadmap.md`, not yet executed.
- **Capability / permission probe** (`mediaway_device::capability::{DeviceKind,
  Support, PermissionState}`, `mediaway_pipeline::platform::{device_support,
  request_device_permission}`) — a real, separate Rust surface
  ([ADR-0003](../../mediaway-device/adr/0003-capability-and-permission-probe.md))
  not covered by this pass; a capture-session "open and poll" ABI and a
  "should I even try" probe ABI are different enough capabilities to design
  independently.
- **`cbindgen` adoption** — now overdue by the sibling ADRs' own stated
  "once ≥2 headers exist" condition (three now exist); still not decided
  unilaterally here.
- **`mediaway-device-linux` hardware verification** — real source exists
  (`camera.rs`, `mic.rs`, `window.rs`, `screencast.rs`) with the same
  `#[cfg(target_os = "linux")]` dispatch shape this ADR adds for Windows,
  but this pass (on a Windows dev machine, per its own brief) does not
  verify or scope Linux capture behavior — the `#[cfg]` arm compiles against
  it but is untested here.
- **Stale-doc follow-ups** (§ Context Finding 1) — `crates/mediaway-device-windows/docs/roadmap.md`
  Stage 4's "not yet wired" bullet and `mediaway_device::capability::Unavailable::NotImplemented`'s
  "e.g. `Camera` today" doc comment both need a source-touching fix against
  their own crates; out of scope for this ADR-only pass.

## References

- [`crates/mediaway-device/src/{video.rs,audio.rs,error.rs,lib.rs}`](../../mediaway-device/src) — wrapped facade traits/configs/errors
- [`crates/mediaway-device/adr/0001-capture-traits.md`](../../mediaway-device/adr/0001-capture-traits.md), [`adr/0002-facade-platform-boundary.md`](../../mediaway-device/adr/0002-facade-platform-boundary.md) — facade/platform split this crate's dependency shape (§1) follows
- [`crates/mediaway-device-windows/src/camera.rs`](../../mediaway-device-windows/src/camera.rs), [`camera_tests.rs`](../../mediaway-device-windows/src/camera_tests.rs) — real, hardware-verified Camera backend (§ Finding 1)
- [`crates/mediaway-device-windows/src/dxgi.rs`](../../mediaway-device-windows/src/dxgi.rs), [`wgc.rs`](../../mediaway-device-windows/src/wgc.rs) — real Screen/Window backends, GPU-only, no CPU fallback (§ Finding 2)
- [`crates/mediaway-device-windows/src/wasapi.rs`](../../mediaway-device-windows/src/wasapi.rs) — real Microphone/Loopback/ProcessLoopback backend
- [`crates/mediaway-device-windows/docs/roadmap.md`](../../mediaway-device-windows/docs/roadmap.md) — Stage 4 stale-wiring note (§ Context, § Deferred)
- [`crates/mediaway-common/src/{frame.rs,formats.rs,gpu.rs,lib.rs}`](../../mediaway-common/src) — `VideoFrame`/`AudioFrame`/`VideoFrameStorage`, `PixelFormat`/`SampleFormat`, `GpuBufferHandle`/`GpuDeviceHandle`, `StreamInfo`/`VideoGeometry`/`Rational` field types (verified)
- [`crates/mediaway/src/platform.rs`](../../mediaway/src/platform.rs) — existing `ScreenCapture`/`Microphone` dispatch shape this crate's own dispatch (§1) mirrors without importing
- [`crates/mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md`](../../mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md), [`crates/mediaway-ffi/adr/0001-auto-encode-c-abi.md`](../../mediaway-ffi/adr/0001-auto-encode-c-abi.md) — precedent this ADR reuses/deviates from (handle shape, status enums, `Rational`/`PixelFormat` reuse, frame-struct collision)
- [`bindings/c/examples/screen_record.c`](../../../bindings/c/examples/screen_record.c), [`camera_record.c`](../../../bindings/c/examples/camera_record.c) — aspirational naming input (non-binding)
- [`docs/spec/c-ffi.md`](../../../docs/spec/c-ffi.md), [`docs/adr/0004-c-ffi.md`](../../../docs/adr/0004-c-ffi.md) — workspace policy this ADR concretizes
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md) — why Screen/Window are deferred
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) — honest-copy-path / honest-scope / blocking-close documentation requirement
- [`docs/ai/wiki/device/{windows-capture.md,windows-window.md,windows-audio.md,camera-device-handle.md}`](../../../docs/ai/wiki/device) — capture backend knowledge; `camera-device-handle.md` corrected as part of this ADR's wiki upkeep

ADRs are **English**. Numbering is local to this `adr/` folder.
