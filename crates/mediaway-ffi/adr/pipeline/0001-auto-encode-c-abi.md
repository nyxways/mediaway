# ADR-0001: Auto-encode → fragmented MP4 C ABI surface (first pass)

- **Status**: Proposed
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi`

## Context

`mediaway-ffi` is the **second** `mediaway-*-ffi` crate in the
workspace, after `mediaway-container-ffi` (ADR-0004,
[`docs/spec/c-ffi.md`](../../../docs/spec/c-ffi.md)). It wraps the real,
working auto-encode convenience layer in
[`crates/mediaway/src/{session.rs,platform.rs,error.rs}`](../../mediaway/src):
`platform::AutoEncoder::open` (best-available OS/GPU `VideoEncoder` for a
config, or `EncodeError::NoBackend`) feeding a single-track
`EncodeSession<E: VideoEncoder>` that streams frames straight into a
fragmented-MP4 `mp4::Muxer<Live>` and returns complete bytes from `finish()`.

Nothing about the concrete C ABI shape exists yet.
`bindings/c/examples/encode_to_mp4.c` sketches an aspirational naming scheme,
written before either real crate existed — consumer-side input, not a binding
decision, same status as `mux_roundtrip.c` was for ADR-0001 of
`mediaway-container-ffi`.

Three things this ADR must decide that did not exist for the first `-ffi`
crate:

1. **Config field scope.** `mediaway_encoder::auto::AutoVideoEncodeConfig` has
   8 fields; `gpu_device: Option<GpuDeviceHandle>` and `backend:
   BackendSelection` reach into unsolved GPU-handle-across-C-boundary and
   backend-pinning territory. Which fields ship in v1.
2. **A second independently-compiled library needs an identical "leak/reclaim
   an owned byte buffer" capability and a status-code type**, with **no**
   shared header (`mediaway-common-ffi` does not exist). Whether to reuse a
   name, define a fresh one, or start the shared crate now.
3. **Two Rust construction steps** (`AutoEncoder::open` returning
   `Box<dyn VideoEncoder>`, then `EncodeSession::open(encoder)` consuming it)
   already imply two C handles in the aspirational example
   (`mediaway_auto_encoder_open` → `mediaway_encode_session_open`) — verify
   that shape against the real Rust ownership contract and decide whether to
   keep it or collapse it.

This ADR reuses `mediaway-container-ffi/adr/0001`'s established patterns
(single-`Box` opaque handles, `catch_unwind` + per-handle `poisoned` flag,
hand-written header, borrowed-input/owned-output+`_free` memory rule) and
states plainly where it deviates.

## Decision

> Adopt the two-handle shape and struct fields from
> `bindings/c/examples/encode_to_mp4.c`, **with corrections/additions**
> (below). Give this crate its **own**, distinctly-named status-code type and
> buffer-free function (not shared with `mediaway-container-ffi`) rather than
> starting `mediaway-common-ffi` in this pass. Hand-write the header. Config
> ships v1 fields only; GPU-handle and backend-pinning fields are deferred.

### 1. Config field scope (v1 vs deferred)

`mediaway_auto_video_encode_config_t` is a **plain, `repr(C)` value struct**
(not an opaque handle) — it owns no heap allocation, matching
`AutoVideoEncodeConfig`'s own `#[derive(Clone, PartialEq, Eq)]`, no `Drop`.
The aspirational example already treats it this way
(`config.bitrate_bps = 2000000;` after construction, on the stack). v1 ships:

| Rust field | v1 C field | Decision |
|---|---|---|
| `codec: CodecKind` | `codec: mediaway_pipeline_codec_kind_t` | **In scope** — real Windows `AutoVideoEncoder::open` already resolves H.264/HEVC/AV1/VP9 (verified in `crates/mediaway-encoder-windows/src/wmf/codec.rs`), not H.264-only; restricting the C surface to one codec would under-serve a capability that already works. See §4 for the constructor shape. |
| `width`, `height`, `time_base` | same | In scope — required by `AutoVideoEncodeConfig::new`. |
| `bitrate_bps: u32` | same | In scope — plain integer, `0` = backend default, matches the aspirational example. |
| `pixel_format: PixelFormat` | `pixel_format: mediaway_pixel_format_t` | **In scope**, not hardcoded — `crates/mediaway-encoder-windows/src/wmf/video.rs:323` shows the CPU-upload path already accepts both `Nv12` and `Bgra8`, not NV12-only. Mirror all 5 `PixelFormat` variants in the C enum; document that only `Nv12`/`Bgra8` are exercised by the current Windows backend today (an existing Rust-level limitation, not a new FFI one — the existing `EncodeError::Unsupported` already covers the rest). |
| `max_path_class: EncodePathClass` | — | **Deferred.** Controls Zero-Copy vs GPU-copy vs CPU-upload vs readback vs software selection; meaningless without `gpu_device` also being choosable, and this pass hardcodes the Rust default (`CpuUpload`) by never setting it. |
| `gpu_device: Option<GpuDeviceHandle>` | — | **Deferred.** A GPU handle crossing a C boundary is its own unsolved design problem per [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md) — no `GpuBufferHandle`-equivalent C ABI type exists yet. v1 always passes `None`, i.e. CPU-upload-only from C. |
| `backend: BackendSelection` | — | **Deferred.** Pinning a specific vendor SDK/backend from C is a real, separable feature; v1 always uses the Rust default (`BackendSelection::Auto`). |

Consequence: from C in v1, every session runs the `CpuUpload`-or-lower path
(actually exactly `CpuUpload`, since `Readback`/`Software` also require
raising `max_path_class`, which is unreachable) — **never** the Zero-Copy GPU
path, even though the Rust layer underneath supports it. This is a real,
documented capability gap of this pass, not a silent one (§ Deferred).

### 2. Status enum — fresh, distinctly-named type; independent numbering

```c
typedef enum mediaway_pipeline_status {
    MEDIAWAY_PIPELINE_STATUS_OK                      = 0,
    MEDIAWAY_PIPELINE_STATUS_INVALID_ARGUMENT        = 1, /* null pointer, mismatched ptr/len */
    MEDIAWAY_PIPELINE_STATUS_HANDLE_POISONED         = 2, /* a previous call on this handle panicked */
    MEDIAWAY_PIPELINE_STATUS_NO_BACKEND              = 3, /* EncodeError::NoBackend — expected/graceful */
    MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED             = 4, /* EncodeError::Unsupported — context-dependent, not always graceful */
    MEDIAWAY_PIPELINE_STATUS_INVALID_INPUT           = 5, /* EncodeError::InvalidInput */
    MEDIAWAY_PIPELINE_STATUS_ENCODER_BACKEND_FAILURE = 6, /* EncodeError::Backend */
    MEDIAWAY_PIPELINE_STATUS_ENCODER_CLOSED          = 7, /* EncodeError::Closed */
    MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_TRACK       = 8, /* mp4::Error::InvalidTrack, via PipelineError::Mux */
    MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_PACKET      = 9, /* mp4::Error::InvalidPacket */
    MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_DATA        = 10, /* mp4::Error::InvalidData */
    MEDIAWAY_PIPELINE_STATUS_UNKNOWN_ERROR           = 11, /* future #[non_exhaustive] variant on EncodeError, mp4::Error, or PipelineError itself */
    MEDIAWAY_PIPELINE_STATUS_INTERNAL_PANIC          = 12, /* this call caught a Rust panic; handle now poisoned */
} mediaway_pipeline_status_t;
```

**Not** named `mediaway_status_t` and **not** numerically mirrored onto
`mediaway-container-ffi`'s `MediawayStatus`, for two independent reasons:

- **Type name must differ.** These are two separately compiled libraries
  shipping two separate headers with no shared header today. If a consumer
  ever includes both `<mediaway/container.h>` and `<mediaway/pipeline.h>` in
  the same translation unit, C requires an identical redefinition for a
  reused `typedef` name (C11 §6.7p3) — and the enumerator sets here are not
  identical (this crate needs `NO_BACKEND`/`ENCODER_CLOSED`/mux-error codes
  container-ffi has no reason to define). Reusing the name would be a latent
  compile error waiting for the first consumer that links both. A distinct
  name (`mediaway_pipeline_status_t`) sidesteps this outright.
- **Numeric mirroring is not attempted**, unlike `mediaway_pipeline_codec_kind_t`
  below (§5) — `MediawayStatus`'s 3 mux-error codes and this enum's 3
  mux-error codes both wrap the *same* underlying `mp4::Error` variants, but
  this enum also carries 5 encoder-error codes container-ffi has no analog
  for, so a shared numbering would only be accidentally aligned for 3 of 13
  values and would misleadingly imply a cross-library contract that doesn't
  exist yet. `mediaway_pipeline_codec_kind_t` mirrors container-ffi's numbers
  because both wrap the exact same shared Rust type
  (`mediaway_common::CodecKind`) end-to-end; this status enum wraps two
  *structurally different* per-crate Rust error hierarchies, so pretending
  value-compatibility buys nothing.

No `INVALID_STATE` value: unlike the muxer's `Open`/`Live` typestate, neither
`AutoEncoder` nor `EncodeSession` has a caller-visible illegal-transition to
guard against at the FFI boundary (see §3's handle design) — flagged here as
the "anything genuinely different" the design brief asked for.

`NO_BACKEND` vs `UNSUPPORTED`: both are plausible "nothing to do here"
outcomes, but only `NO_BACKEND` is **unconditionally** graceful (no backend
compiled in at all, matching the aspirational example's exit-cleanly
handling). `UNSUPPORTED` (bad codec/pixel-format/geometry combination for
whatever backend *is* present) may equally be a caller bug — callers should
not blanket-treat it as graceful the way the aspirational example's
`if (open_status != MEDIAWAY_OK)` currently does for *any* non-OK status.
This is a real behavioral looseness in `encode_to_mp4.c` worth tightening in
the follow-up bindings pass (not fixed in this ADR — bindings are out of
scope here, same two-step process as the container crate).

### 3. Opaque handles

```rust
// AutoEncoderHandle needs no wrapper struct or `poisoned` flag: the handle
// *is* the trait object, because its only two operations are "consume into a
// session" and "close" — both of which destroy the pointer unconditionally,
// so there is no repeated-call-after-panic scenario to guard against.
type AutoEncoderHandle = Box<dyn mediaway_encoder::VideoEncoder>;

struct EncodeSessionHandle {
    poisoned: bool, // write_frame is called repeatedly — needs the same guard as MuxerHandle/DemuxerHandle
    inner: mediaway_pipeline::EncodeSession<Box<dyn mediaway_encoder::VideoEncoder>>,
}
```

Two handles, matching the aspirational example's implied two-step shape
(`mediaway_auto_encoder_open` then `mediaway_encode_session_open`) — this is
**not** a correction, it independently validates against the real Rust
contract: `AutoEncoder::open` really does return an intermediate
`Box<dyn VideoEncoder>` before `EncodeSession::open` consumes it, so the
sketch's two-call shape is kept as-is.

**Non-obvious ownership rule, not stated in the aspirational example:**
`mediaway_encode_session_open(encoder, &out_session)` takes ownership of
`encoder` **unconditionally** — success or failure — because
`EncodeSession::open(encoder: E)` takes `E` by value in Rust; on the
`Err(PipelineError::Mux(_))` path (the muxer rejects the encoder's stream
info) the moved-in encoder is simply dropped as part of unwinding the
`Result`, same as Rust itself would do. After calling
`mediaway_encode_session_open`, the `encoder` pointer is invalid regardless of
the returned status; do **not** call `mediaway_auto_encoder_close` on it
afterward (double-free). This must be stated explicitly in the header, not
left implicit (ADR-0006 "code carries the contract").

### 4. Function list

```c
uint32_t mediaway_pipeline_ffi_abi_version(void);

/* — config (plain value struct, no handle, no free) — */
mediaway_auto_video_encode_config_t mediaway_auto_video_encode_config_new(
    mediaway_pipeline_codec_kind_t codec, uint32_t width, uint32_t height,
    mediaway_rational_t time_base);
/* Sugar over config_new(MEDIAWAY_PIPELINE_CODEC_H264, ...) — kept because the
 * aspirational example calls it, but v1 also exposes the general form since
 * H.264 is not the only real codec the Windows auto backend resolves today. */
mediaway_auto_video_encode_config_t mediaway_auto_video_encode_config_h264(
    uint32_t width, uint32_t height, mediaway_rational_t time_base);

/* — auto encoder (intermediate handle) — */
mediaway_pipeline_status_t mediaway_auto_encoder_open(
    const mediaway_auto_video_encode_config_t *config,
    mediaway_auto_encoder_t **out_encoder);
void mediaway_auto_encoder_close(mediaway_auto_encoder_t *encoder);
/* Only for abandoning an opened encoder without ever calling
 * mediaway_encode_session_open on it — not shown in the aspirational
 * example's happy path, added for the same resource-cleanup symmetry as
 * mediaway_muxer_close/mediaway_demuxer_close. */

/* — encode session — */
mediaway_pipeline_status_t mediaway_encode_session_open(
    mediaway_auto_encoder_t *encoder, /* consumed unconditionally — see §3 */
    mediaway_encode_session_t **out_session);
mediaway_pipeline_status_t mediaway_encode_session_write_frame(
    mediaway_encode_session_t *session, const mediaway_video_frame_t *frame);
mediaway_pipeline_status_t mediaway_encode_session_finish(
    mediaway_encode_session_t *session, /* consumed — do not close afterward */
    uint8_t **out_data, size_t *out_len);
void mediaway_encode_session_close(mediaway_encode_session_t *session);
/* Abandon a session without finishing it (no flush, no valid MP4 output) —
 * added for the same reason as mediaway_auto_encoder_close. */

/* — shared free — */
void mediaway_pipeline_ffi_buffer_free(uint8_t *data, size_t len);
```

Corrections/additions vs. the aspirational example:

| # | Aspirational sketch | Issue | Correction |
|---|---|---|---|
| a | `mediaway_status_t` (implied generic name) | Would collide (different enumerator set, same `typedef` name) if ever included alongside `mediaway_container.h`'s `mediaway_status_t` — see §2 | `mediaway_pipeline_status_t`, crate-scoped |
| b | `mediaway_buffer_free(ptr)` (one-arg, stale — same bug ADR-0001 of `mediaway-container-ffi` already found and fixed to a two-arg form for its own crate) | Needs `len` to reconstruct `Box<[u8]>`; **and** a second `-ffi` crate defining the *same* symbol name risks a duplicate-symbol link error if both crates' `staticlib` outputs are ever linked into one consumer (this crate's `Cargo.toml` already declares `crate-type = ["rlib", "cdylib", "staticlib"]`) | `mediaway_pipeline_ffi_buffer_free(uint8_t *data, size_t len)` — see § Buffer-free naming below |
| c | `.den = (int32_t)fps` in the `mediaway_rational_t` literal | Already flagged as a bug by `mediaway-container-ffi/adr/0001` §4d against this exact file — `mediaway_common::Rational` is `{num: u64, den: u32}`, not `{i32, i32}` | `mediaway_rational_t { uint64_t num; uint32_t den; }` (reused as-is from the sibling ADR, not re-derived) |
| d | Config implies H.264 as the only codec (`config_h264` is the only constructor shown) | Windows auto backend already resolves HEVC/AV1/VP9 too (§1) | Add the general `mediaway_auto_video_encode_config_new(codec, ...)`; keep `_h264` as sugar over it |
| e | No explicit `close`/discard calls for the intermediate encoder or an unfinished session | Needed for symmetry with `mediaway_muxer_close`/`mediaway_demuxer_close` and to give callers a defined way to release resources on an early-abort path | Add `mediaway_auto_encoder_close`, `mediaway_encode_session_close` |

### 5. Struct layouts

```c
typedef enum mediaway_pipeline_codec_kind {
    MEDIAWAY_PIPELINE_CODEC_H264 = 0,   MEDIAWAY_PIPELINE_CODEC_HEVC = 1,
    MEDIAWAY_PIPELINE_CODEC_AV1  = 2,   MEDIAWAY_PIPELINE_CODEC_VP9  = 3,
    MEDIAWAY_PIPELINE_CODEC_AAC  = 4,   MEDIAWAY_PIPELINE_CODEC_OPUS = 5,
    MEDIAWAY_PIPELINE_CODEC_MP3  = 6,   MEDIAWAY_PIPELINE_CODEC_VORBIS = 7,
    MEDIAWAY_PIPELINE_CODEC_WEBVTT = 8, MEDIAWAY_PIPELINE_CODEC_TX3G = 9,
    MEDIAWAY_PIPELINE_CODEC_RAW_VIDEO = 10, MEDIAWAY_PIPELINE_CODEC_RAW_AUDIO = 11,
} mediaway_pipeline_codec_kind_t;
/* Distinct type name from mediaway_codec_kind_t (mediaway-container-ffi), but
 * numeric values are deliberately mirrored 1:1 — both wrap the *same* shared
 * Rust type (mediaway_common::CodecKind) end-to-end, so keeping the same
 * integer mapping is low-risk today and eases a future mediaway-common-ffi
 * merge. Passing a non-video codec (AAC..RAW_AUDIO) is a runtime
 * MEDIAWAY_PIPELINE_STATUS_INVALID_ARGUMENT, not a compile-time restriction —
 * AutoVideoEncodeConfig.codec's Rust type really is the full CodecKind. */

typedef enum mediaway_pixel_format {
    MEDIAWAY_PIXEL_FORMAT_NV12  = 0,
    MEDIAWAY_PIXEL_FORMAT_I420  = 1,
    MEDIAWAY_PIXEL_FORMAT_BGRA8 = 2,
    MEDIAWAY_PIXEL_FORMAT_RGBA8 = 3,
    MEDIAWAY_PIXEL_FORMAT_YUYV  = 4,
} mediaway_pixel_format_t; /* first definition of this enum in the workspace's C headers — no mirroring precedent to reconcile against */

typedef struct mediaway_rational {
    uint64_t num;
    uint32_t den;
} mediaway_rational_t; /* identical shape to mediaway-container-ffi's — reused, not re-derived (§4c) */

typedef struct mediaway_auto_video_encode_config {
    mediaway_pipeline_codec_kind_t codec;
    uint32_t width;
    uint32_t height;
    mediaway_rational_t time_base;
    uint32_t bitrate_bps;        /* 0 = backend default */
    mediaway_pixel_format_t pixel_format;
    /* max_path_class / gpu_device / backend: deferred, not exposed (§1) */
} mediaway_auto_video_encode_config_t; /* plain value type; no free function */

/* Input to mediaway_encode_session_write_frame — borrowed view, CPU-only. */
typedef struct mediaway_video_frame {
    int64_t pts;
    uint64_t duration;            /* matches VideoFrame::duration's u64 */
    uint32_t width;
    uint32_t height;
    mediaway_pixel_format_t pixel_format;
    const uint8_t *raw_bytes;     /* borrowed; valid for the call only */
    size_t raw_bytes_len;
} mediaway_video_frame_t;
/* Models VideoFrameStorage::Cpu{data} only. VideoFrameStorage::Gpu(GpuBufferHandle)
 * has no C representation yet — a frame built from this struct can never carry
 * a GPU handle, so this pass can never exercise the Zero-Copy encode path from
 * C, consistent with the config-level gpu_device deferral in §1. */
```

### 6. Memory ownership

- **`write_frame` (input):** `raw_bytes` is a caller-owned borrow, valid for
  the call only. One copy at the boundary
  (`bytes::Bytes::copy_from_slice`) builds the owned `VideoFrame` /
  `VideoFrameStorage::Cpu` the Rust core needs — same non-Zero-Copy shape and
  same reasoning as `mediaway-container-ffi`'s `push_packet` (§6 of that
  ADR): C has no refcounted-buffer concept to hand across without inventing
  one. Do not describe this path with a `zc`/⚡ label.
- **`finish` (output):** hands back an owned buffer
  (`uint8_t **out_data, size_t *out_len`), same `Vec<u8>` →
  `into_boxed_slice()` → `Box::into_raw()` leak/reclaim shape as
  `mediaway_muxer_poll_bytes`/`mediaway_buffer_free`, freed here via
  `mediaway_pipeline_ffi_buffer_free`.
- **`mediaway_auto_video_encode_config_t` (value struct):** no heap
  allocation, no free function — passed and returned by value like
  `mediaway_rational_t`.

#### Buffer-free naming decision

Both aspirational examples (`mux_roundtrip.c`, `encode_to_mp4.c`) call a
function named `mediaway_buffer_free(ptr)` for their respective owned output
buffers. `mediaway-container-ffi` already shipped a **real**
`mediaway_buffer_free(uint8_t *data, size_t len)`. This crate needs the
identical capability. Two options:

(a) **This crate defines its own, distinctly-named function**
    (`mediaway_pipeline_ffi_buffer_free`).
(b) **Start `mediaway-common-ffi` now**, exporting one shared
    `mediaway_buffer_free` that `mediaway-container-ffi` adopts in a
    follow-up.

**Decision: (a).** Reasons:

- **A real, not hypothetical, link hazard exists today.** This crate's
  `Cargo.toml` already declares `crate-type = ["rlib", "cdylib",
  "staticlib"]`, same as `mediaway-container-ffi`. If a consumer statically
  links both crates' `staticlib` outputs into one final binary (a normal
  thing to do for a native app that wants both container demux and pipeline
  encode), two global C symbols named `mediaway_buffer_free` is a guaranteed
  duplicate-symbol link error on every platform's linker — not a
  runtime/dlopen concern that dylib-per-process export tables would avoid,
  but a static-link-time one. A distinctly-named function sidesteps this
  outright, today, with zero design cost.
- **`docs/spec/c-ffi.md` already lists `mediaway-common-ffi` as optional**
  ("error codes, `Rational`, buffer view only"). Starting it well is a
  cross-crate design decision — what exactly it owns, how both existing
  `-ffi` crates migrate to it, whether `mediaway_status_t` itself moves there
  too (a much bigger change than one free function, see §2's reasoning for
  why the two crates' status enums are not simply mergeable today) —
  deserving of its own ADR with its own review, not a rider on this crate's
  first pass.
- Symmetric with how `mediaway-container-ffi/adr/0001` deferred exactly this
  same question in its own § Deferred, rather than force it into that
  crate's first pass either.

This is logged under § Deferred, not silently dropped.

### 7. Panic safety

Same `catch_unwind(AssertUnwindSafe(...))` strategy as
`mediaway-container-ffi` (its ADR-0001 §7) for every exported function.
Null/argument checks happen before entering `catch_unwind`, returning
`MEDIAWAY_PIPELINE_STATUS_INVALID_ARGUMENT` directly.

`EncodeSessionHandle.poisoned` mirrors `MuxerHandle`/`DemuxerHandle`: a caught
panic during `write_frame` or `finish` sets it, and every subsequent call
short-circuits to `MEDIAWAY_PIPELINE_STATUS_HANDLE_POISONED` — except
`mediaway_encode_session_close`, always safe to call.

`AutoEncoderHandle` needs **no** `poisoned` flag (§3): its only two
operations (`mediaway_encode_session_open`, `mediaway_auto_encoder_close`)
both destroy the pointer unconditionally on return, so there is no
call-again-after-panic scenario a flag would need to guard.

`mediaway_auto_encoder_open`/`mediaway_encode_session_open` distinguish three
outcomes, not two: (1) a normal `Ok` — build the handle; (2) a normal `Err`
(e.g. `EncodeError::NoBackend`) — no handle exists, map to the matching
status, `*out_*` set to `NULL`; (3) a caught panic — same `NULL`/
`INTERNAL_PANIC` shape as `mediaway_muxer_create` returning `NULL`. (2) is
an expected Result, not a panic — the distinction the design brief called
out explicitly.

Out of scope, same as the sibling ADR: allocator OOM (aborts the process,
uncatchable) and the default panic hook's stderr output.

### 8. Header authoring

**Hand-written** `include/mediaway/pipeline.h`, same reasoning as
`mediaway-container-ffi/adr/0001` §8: this pass still makes shape decisions
(POD config struct returned by value, two-handle split, CPU-only frame
struct collapsing an enum Rust type) a mechanical `cbindgen` pass has no way
to infer. Now that **two** hand-written `-ffi` headers exist, a follow-up
ADR should evaluate `cbindgen`/shared tooling across both — not blocking this
pass, and not this ADR's call to make unilaterally for a sibling crate.

Same version-macro + runtime-accessor convention:
`MEDIAWAY_PIPELINE_FFI_ABI_VERSION` (compile-time) +
`mediaway_pipeline_ffi_abi_version()` (runtime, for dynamically-loaded
consumers).

### 9. Feature flags

**No `[features]` table** — a single always-on surface, unlike
`mediaway-container-ffi`'s `mux`/`demux` split. `mediaway` itself has
no Cargo features; its Windows/Linux dispatch (`platform.rs`) is a runtime
`#[cfg(target_os = …)]` concern inside one always-compiled crate, not a
Cargo-feature-gated one, so there is no natural per-capability split to
mirror at this layer either. This crate's own exported function set (§4) is
one coherent capability — auto video encode → fMP4 — with no sub-parts a
slim build would plausibly want to drop.

**Explicitly documented limitation, not fixed here:** `mediaway`'s
`Cargo.toml` depends unconditionally on `mediaway-decoder`, `mediaway-device`,
and their platform backends (`mediaway-decoder-windows`,
`mediaway-device-windows`, and Linux equivalents) — none of which this v1
FFI surface (`AutoEncoder` + `EncodeSession`, encode-only) calls. Building
`mediaway-ffi` today compiles and links WMF video decode, WASAPI
capture, and screen-capture platform code no exported function here ever
reaches, because `mediaway` has no `[features]` table to select
against. Same class of gap as `mediaway-container-ffi/adr/0001` §9's
unconditional format-core deps — a `mediaway` Cargo.toml concern,
flagged as a follow-up against that crate's own roadmap, not this ADR's to
fix.

### 10. Thread safety

Handles are thread-confined by convention, same documentation obligation as
`mediaway-container-ffi` (its ADR-0001 §10): moving a handle to another
thread is fine, concurrent calls on the same handle pointer from two threads
without external synchronization is a data race. **Caveat specific to this
crate, not yet verified:** the underlying `VideoEncoder` trait object may be
a Windows Media Foundation / COM-backed backend, which can impose a stricter
same-thread-only (STA-apartment) requirement than plain "no concurrent
calls" — whether that constraint actually applies is left to verify when
`src/lib.rs` is implemented and documented precisely then, not asserted here
without evidence.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Collapse `AutoEncoder::open` + `EncodeSession::open` into one C call | Contradicts the real two-step Rust ownership contract and the aspirational example's own two-call shape; would also remove the caller's ability to inspect/discard an opened encoder before committing to a session |
| Reuse `mediaway_status_t`/`mediaway_buffer_free` names from `mediaway-container-ffi` | Real duplicate-typedef / duplicate-symbol hazard once both crates' `staticlib` outputs (or headers) are ever combined in one consumer — see §2, § Buffer-free naming |
| Start `mediaway-common-ffi` in this pass | Bigger cross-crate design question (what it owns, how both existing `-ffi` crates migrate) deserving its own ADR, not a rider on this crate's first pass |
| Expose `gpu_device`/`backend`/`max_path_class` in v1 | GPU handles crossing a C boundary are an unsolved design problem (`docs/spec/gpu-interop.md`); backend pinning is a separable feature; neither blocks a first working CPU-upload encode path |
| Hardcode `pixel_format` to NV12 only | The real Windows CPU-upload path already accepts `Bgra8` too (verified in source) — hardcoding would under-serve existing capability for no simplification benefit |
| `cbindgen`-generated header | Same reasoning as the sibling ADR: this pass's shape decisions aren't a mechanical translation; revisit once shared tooling has a dedicated ADR |

## Consequences

### Positive

- Concrete, reviewable ABI surface for the second `-ffi` crate; every
  cross-crate ambiguity (status-enum naming/numbering, buffer-free naming)
  the first crate's ADR could not have anticipated is now resolved with
  stated reasoning.
- `AutoEncoderHandle`'s no-`poisoned`-flag simplification and the
  unconditional-consume rule for `mediaway_encode_session_open` are new,
  documented precedent for future `-ffi` crates with similar
  intermediate-handle shapes.
- v1's CPU-upload-only reach (no `gpu_device`/`backend`/`max_path_class`) is
  a stated, deliberate scope cut, not a silently dropped capability.

### Negative / Trade-offs

- No Zero-Copy GPU encode path is reachable from C in this pass — every
  frame pushed through `mediaway_encode_session_write_frame` takes the
  `CpuUpload` path underneath, even on a machine where the Rust layer could
  do better.
- Two independently-numbered/named status enums and two independently-named
  buffer-free functions across the workspace's two `-ffi` crates today — a
  real (if currently harmless) fragmentation that `mediaway-common-ffi` is
  meant to eventually resolve.
- `mediaway-ffi` cannot yet ship an encode-only compiled artifact
  free of unrelated decode/device platform code, for the same structural
  reason `mediaway-container-ffi` cannot ship an MP4-only one (§9).

## Deferred to a later ADR / explicit open questions

- **`gpu_device` / `max_path_class` / `backend`** config fields and any
  Zero-Copy or GPU-copy encode path reachable from C (§1) — real Rust
  capability exists, no C ABI shape decided yet.
- ~~**`mediaway-common-ffi`** — whether/how to unify `mediaway_status_t` /
  `mediaway_pipeline_status_t` and the two `mediaway_buffer_free`-style
  functions behind one shared crate (§ Buffer-free naming, §2). Two `-ffi`
  crates now exist; a concrete unification ADR is more informed with both
  precedents in hand than either crate's own first pass was.~~ — resolved by
  [`docs/adr/0015-common-ffi-unification.md`](../../../docs/adr/0015-common-ffi-unification.md):
  the shared crate unifies the `Rational`/`CodecKind` value-type mirrors and
  the buffer leak/reclaim helper *implementation* only, as an `rlib`-only
  internal dependency with no C symbols of its own.
  `mediaway_status_t`/`mediaway_pipeline_status_t` stay independent,
  distinctly-named types, and each crate keeps its own distinctly-named
  exported free function (`mediaway_buffer_free` vs.
  `mediaway_pipeline_ffi_buffer_free`) — only their internal implementation is
  now shared.
- **`cbindgen` adoption** — revisit now that two hand-written headers exist
  (§8), as its own ADR, not decided unilaterally here.
- **`mediaway`'s unconditional decode/device/platform deps** (§9) —
  a facade-level Cargo.toml fix, tracked against that crate's own roadmap.
- ~~**Whether `UNSUPPORTED` should ever be treated as gracefully as
  `NO_BACKEND`** (§2)~~ — resolved: `bindings/c/examples/encode_to_mp4.c` now
  only treats `MEDIAWAY_PIPELINE_STATUS_NO_BACKEND` as a graceful skip;
  `UNSUPPORTED` and every other non-OK status from
  `mediaway_auto_encoder_open` go through `CHECK` (hard failure), since the
  example's config is a known-good default that a present backend should
  never reject.
- **Screen/camera capture and decode surfaces** (`platform::ScreenCapture`,
  `platform::Microphone`, `platform::AutoDecoder`) — explicitly out of scope
  for this crate's first pass per its own roadmap; separate capabilities,
  own ADRs.
- **COM/STA thread-affinity verification** for Windows backends (§10) — to
  be confirmed and documented precisely when `src/lib.rs` is implemented.

## References

- [`crates/mediaway-ffi/README.md`](../README.md), [`docs/roadmap.md`](../docs/roadmap.md)
- [`crates/mediaway/src/session.rs`](../../mediaway/src/session.rs), [`platform.rs`](../../mediaway/src/platform.rs), [`error.rs`](../../mediaway/src/error.rs) — wrapped Rust surface
- [`crates/mediaway-encoder/src/auto.rs`](../../mediaway-encoder/src/auto.rs) — `AutoVideoEncodeConfig`, `EncodePathClass`, `BackendSelection`
- [`crates/mediaway-encoder/src/error.rs`](../../mediaway-encoder/src/error.rs) — `EncodeError`'s 5 variants (verified)
- [`crates/mediaway-common/src/frame.rs`](../../mediaway-common/src/frame.rs) — `VideoFrame`/`VideoFrameStorage` (verified)
- [`crates/mediaway-common/src/formats.rs`](../../mediaway-common/src/formats.rs) — `PixelFormat`'s 5 variants (verified)
- [`crates/mediaway-encoder-windows/src/wmf/codec.rs`](../../mediaway-encoder-windows/src/wmf/codec.rs), [`wmf/video.rs`](../../mediaway-encoder-windows/src/wmf/video.rs) — verified real codec/pixel-format coverage behind `AutoEncoder::open`
- [`crates/mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md`](../../mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md) — precedent this ADR reuses/deviates from
- [`bindings/c/examples/encode_to_mp4.c`](../../../bindings/c/examples/encode_to_mp4.c) — aspirational naming input (non-binding)
- [`docs/spec/c-ffi.md`](../../../docs/spec/c-ffi.md), [`docs/adr/0004-c-ffi.md`](../../../docs/adr/0004-c-ffi.md) — workspace policy
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md) — why `gpu_device` is deferred
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) — honest-copy-path / honest-scope documentation requirement

ADRs are **English**. Numbering is local to this `adr/` folder.
