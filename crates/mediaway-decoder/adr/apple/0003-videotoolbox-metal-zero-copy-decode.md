# ADR-0003: `VideoToolbox` decode Zero-Copy output (`GpuBufferHandle::Metal`)

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (module `mediaway-decoder::apple`)

## ⚠️ Zero real-hardware / zero compile verification in this session

Same structural constraint as [ADR-0001](0001-videotoolbox-h264-cpu-out.md)/
[ADR-0002](0002-videotoolbox-hevc-vp9-av1-decode.md). Every API name/signature cited below is a
direct read of the locally cloned [`objc2`](https://github.com/madsmtm/objc2) checkout
(`local/vendor-ref/objc2/framework-crates/objc2-core-foundation/src/retained.rs`,
`local/vendor-ref/objc2/generated/CoreVideo/CVPixelBuffer.rs`).

## Context

`VideoOutputPreference::ZeroCopyGpu` has returned `DecodeError::Unsupported` since ADR-0001 —
`GpuBufferHandle::Metal` has existed since before any Apple backend but was never constructed
anywhere in this workspace (confirmed by grepping every crate). Unlike the encode-input direction
(this session's companion [encoder ADR-0003](../../../mediaway-encoder/adr/apple/0003-videotoolbox-metal-zero-copy-encode.md),
which only *borrows* an already-alive caller-owned buffer for one synchronous call), decode output
has a real lifetime problem to solve: `VTDecompressionOutputCallback`'s `image_buffer` parameter
is **only guaranteed valid for the callback's own duration** (confirmed, ADR-0001 § Callback
design already established this for the CPU-copy path — that's *why* the CPU path locks, copies,
and unlocks entirely inside the callback). A Zero-Copy caller needs the buffer to outlive the
callback, so this ADR must decide who keeps it alive, and for how long.

### This crate does not need a fixed-slot recycling scheme — `CVPixelBufferPool` already isn't one

The closest precedent in this workspace, `mediaway-decoder` [ADR-linux/0006](../linux/0006-vaapi-dmabuf-zero-copy-output.md)
(DMA-BUF Zero-Copy), had to reintroduce DPB-slot `outstanding` tracking because VA-API's surface
pool is a **fixed-size array** — recycling a slot while a caller still holds its exported fd
corrupts memory the caller may still be reading. `VTDecompressionSession`'s internal
`CVPixelBufferPool` (created automatically, not by this crate) is architecturally different: **it
grows on demand** when more buffers are retained than are immediately free, per Apple's own
documented `CVPixelBufferPool` behavior (a retain-counted pool never overwrites a buffer another
owner still holds a reference to — that is the entire point of `CFRetain`/`CFRelease` reference
counting). Retaining a decoded `CVPixelBuffer` past its callback therefore does **not** risk the
tear/corruption class ADR-linux/0006 had to guard against — this ADR needs no `outstanding`-style
bookkeeping, a genuinely different (simpler) risk profile than every prior Zero-Copy ADR in this
workspace.

The real risk this ADR does need to manage is **memory growth**: a caller that holds every
returned Zero-Copy handle forever grows the pool without bound. This ADR's chosen contract (below)
bounds that to "at most one caller-visible handle outstanding," matching this crate's existing
documented convention for every other Zero-Copy decode backend.

## Decision

> `VideoOutputPreference::ZeroCopyGpu` hands out a real, independent `CFRetain` on each decoded
> `CVPixelBuffer` as `GpuBufferHandle::Metal`. The decoder — not the caller — owns releasing it:
> `VideoToolboxVideoDecoder` holds the most-recently-`poll_frame`-returned handle's retain
> internally (`last_zero_copy_retain: Option<CFRetained<CVPixelBuffer>>`) and drops (releases) it
> at the start of the next `push_packet`/`poll_frame`/`flush` call — the exact "valid until the
> next call that would recycle it" contract [`VideoDecoder::poll_frame`]'s existing rustdoc
> already documents for every other Zero-Copy backend in this crate (Vulkan, VA-API/DMA-BUF,
> Windows D3D11). This ADR does not weaken or special-case that contract for Apple, even though
> `CVPixelBufferPool`'s own growth behavior would technically tolerate a looser one (see
> § Alternatives).

### Verified `CFRetained` API — `retain` + `as_ptr`, no `into_raw`/`from_raw` needed

`objc2_core_foundation::CFRetained<T>` (confirmed, `retained.rs`) exposes `retain(ptr:
NonNull<T>) -> Self` (new independent `+1`), `as_ptr(this: &Self) -> NonNull<T>`
(non-consuming — peek at the raw pointer without giving up ownership), `from_raw`/`into_raw`
(raw-pointer round-trip, **not used by this ADR**), and a `Drop` impl that calls `CFRelease`.
This ADR's shape: the callback calls `CFRetained::retain` once per decoded frame to obtain an
owned reference, reads its bits via `as_ptr` (non-consuming) to build the `NativeHandle`, and
**keeps the `CFRetained` wrapper itself** (not just the bits) inside a new `PendingFrame` struct
pushed into the existing `pending` queue alongside the plain `VideoFrame`. Release is then just
Rust's ordinary `Drop` — no manual `from_raw`+drop round-trip needed, because the owning wrapper
is never converted away from a `CFRetained` in the first place. Simpler and safer than the
`into_raw`/`from_raw` round-trip this ADR's own first draft considered (flagged in
§ Alternatives): fewer raw-pointer operations is strictly better when both shapes cost the same.

### `PendingFrame` — a parallel field, not a parallel queue

```rust
struct PendingFrame {
    frame: VideoFrame,
    /// `None` for `CpuFramesOk` frames (own no backend resource).
    zero_copy_retain: Option<CFRetained<CVPixelBuffer>>,
}
struct SharedState {
    pending: Mutex<VecDeque<PendingFrame>>, // was VecDeque<VideoFrame>
    time_base: Rational,
    output: VideoOutputPreference, // new — callback needs it to choose CPU-copy vs. Zero-Copy
}
```

Bundling the optional retain *into* the existing queue's element type (rather than a second,
separately-locked `VecDeque`, mirroring the shape ADR-linux/0006 used for `exported_fds`) avoids
any lockstep-desync risk between two collections updated from different call sites under
different locks — both fields of one `PendingFrame` are always pushed/popped together, atomically,
because they are one value.

### Handle lifetime — who releases, and exactly when

- **Callback** (`decompression_output_callback` → `build_zero_copy_frame`, `output ==
  ZeroCopyGpu` only): `CFRetained::retain` on the callback's borrowed `pixel_buffer`, geometry
  read via `CVPixelBuffer::{width,height}` (confirmed real, plain safe `fn`, not `unsafe fn`, per
  the generated binding — same accessor family this crate's encoder ADR-0003 also cites), no
  `lock_base_address`/plane read at all (the whole point — this is the path that skips the copy
  the CPU branch still does). Pushed as `PendingFrame { frame, zero_copy_retain: Some(retained) }`.
- **`push_packet`/`poll_frame`/`flush`** (all three, at entry): `self.last_zero_copy_retain =
  None` — drops (releases) whatever was stored from the *previous* `poll_frame` return. A no-op
  for `CpuFramesOk` sessions (`last_zero_copy_retain` is always `None` there).
- **`poll_frame`**, after popping a `PendingFrame`: `self.last_zero_copy_retain =
  popped.zero_copy_retain` — the just-returned frame's retain now becomes "the next call's
  release target," superseding the invalidation that already ran at this same call's entry.
- **`Drop` for `VideoToolboxVideoDecoder`**: no explicit code needed — `last_zero_copy_retain`'s
  ordinary field-drop (compiler-generated, runs after the custom `Drop::drop` body) releases
  whatever was outstanding; any `PendingFrame`s still sitting unpolled in `shared.pending` are
  released the same way whenever `SharedState`'s last `Arc` reference (the decoder's own `shared`
  field plus the reclaimed callback refcon) actually drops.

This gives Rust's ordinary ownership/`Drop` machinery the entire job — no manual reference-count
arithmetic anywhere in this crate's own code, unlike the DMA-BUF ADR's explicit `OwnedFd`
management (POSIX has no automatic refcounting; Core Foundation does).

## Scope (this stage)

**In:**

- `VideoOutputPreference::ZeroCopyGpu` for H.264/HEVC/VP9/AV1 (this backend's full decode scope
  per ADR-0001/0002) — one outstanding handle at a time, released on the next
  `push_packet`/`poll_frame`/`flush` call.
- `GpuBufferHandle::Metal { buffer }` — `buffer` bits are a live `CVPixelBufferRef` the decoder
  itself keeps retained; the caller never calls `CFRelease` (it does not own the reference count
  at all, only borrows the pointer for its documented validity window).

**Out (deferred):**

- Multiple simultaneously-outstanding Zero-Copy handles (e.g. a caller wanting to buffer several
  frames for its own B-frame-style reordering) — this ADR's one-slot contract matches every
  other Zero-Copy decode backend in this crate; a multi-slot variant would need its own ADR and
  a real consumer motivating it (none exists yet, same reasoning ADR-linux/0006 gave for not
  building a speculative consumer-side bridge).
- `GpuDeviceHandle::Metal` (`MTLDevice`) — not surfaced; a Zero-Copy consumer importing this
  handle into Metal/`wgpu` picks its own device, same scope cut the companion encoder ADR makes.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Manual `CFRetained::into_raw` at production time, `CFRetained::from_raw` + drop at release time (round-tripping through raw bits twice) | Considered first, rejected once `PendingFrame` could just keep the `CFRetained` wrapper itself alongside the plain `VideoFrame` — strictly fewer raw-pointer operations for identical behavior; `into_raw`/`from_raw` earn their keep only when the wrapper truly cannot travel with its data (not the case here, since `PendingFrame` is a plain Rust struct that can hold both). |
| No per-call invalidation — rely on `CVPixelBufferPool`'s natural growth and let handles live until the caller drops its own `VideoFrame` | Rejected — this crate's `NativeHandle` is bare bits with no `Drop` glue; without an explicit release call *somewhere*, nothing would ever call `CFRelease`, a real unbounded leak. This ADR needs an explicit release trigger regardless of whether the pool could technically tolerate looser timing — reusing the existing "valid until next call" contract is the cheapest way to get one without inventing new API surface (e.g. a `release_frame` method `VideoDecoder` does not have, unlike the device-capture traits). |
| A caller-facing explicit `release_gpu_frame(handle)` method on `VideoDecoder` | Rejected as new, cross-cutting API surface — `VideoDecoder` has no such method today (only the device-capture traits do), and adding one would need its own workspace-level ADR affecting every decode backend, not just this one. The existing implicit-invalidation contract already solves the release problem without a trait change. |
| Multi-slot outstanding tracking (small ring buffer of retains, e.g. depth 2-3) to give callers slightly more breathing room than "exactly one" | Rejected as speculative scope growth with no concrete caller need identified — easy to add later without breaking the one-slot contract (a strict widening), so deferring costs nothing. |

## Consequences

### Positive

- No DPB/slot-recycling bookkeeping needed at all — a genuinely simpler Zero-Copy story than
  every other backend in this workspace, because Apple's own reference-counted pool already
  provides the safety property those backends had to build by hand.
- Reuses `VideoDecoder::poll_frame`'s existing documented handle-lifetime contract verbatim — no
  new caller-facing concept to learn for an app already using Vulkan/VA-API/D3D11 Zero-Copy
  decode output in this workspace.
- `PendingFrame`'s bundled-field shape avoids the lockstep-desync risk a second parallel
  `VecDeque` would introduce.

### Negative / Trade-offs

- **Zero compile verification as authored** — carries over unchanged.
- One-outstanding-handle limit is a real constraint versus what `CVPixelBufferPool` could
  technically support — a caller wanting to hold multiple decoded frames simultaneously (e.g. for
  its own multi-frame compositing) cannot, without this ADR's own scope-cut (§ Alternatives)
  being revisited.
- `shared.output` is read from the callback thread without synchronization beyond the existing
  "Core Foundation objects are safe to touch from any thread with external synchronization"
  reasoning this file's own `unsafe impl Send` already relies on — `output` is set once at
  construction and never mutated afterward, so this is a plain immutable read, not a new race,
  but worth naming explicitly since it is a new field read from that thread.

## References

- [ADR-0001](0001-videotoolbox-h264-cpu-out.md), [ADR-0002](0002-videotoolbox-hevc-vp9-av1-decode.md)
- `mediaway-encoder` [ADR-apple/0003](../../../mediaway-encoder/adr/apple/0003-videotoolbox-metal-zero-copy-encode.md) —
  companion encode-input ADR from the same session, the simpler (borrow-only) direction
- `mediaway-decoder` [ADR-linux/0006](../linux/0006-vaapi-dmabuf-zero-copy-output.md) — the
  contrast baseline: why *that* backend needs `outstanding` slot tracking and this one does not
- `crates/mediaway-common/src/gpu.rs` — `GpuBufferHandle::Metal`, `NativeHandle`
- Local grounding source (read directly): `local/vendor-ref/objc2/framework-crates/
  objc2-core-foundation/src/retained.rs` (`CFRetained::{retain,as_ptr,from_raw,into_raw}`),
  `local/vendor-ref/objc2/generated/CoreVideo/CVPixelBuffer.rs` (`width`/`height`)
- [`docs/spec/gpu-interop.md`](../../../../docs/spec/gpu-interop.md) (ADR-0005)
- `README.md` § Codec support — Apple OS · GPU H.264/HEVC/AV1/VP9 decode cell: `👻` → `🆗` once
  implemented (implemented/compiles, not hardware-verified)

ADRs are written in **English**.
