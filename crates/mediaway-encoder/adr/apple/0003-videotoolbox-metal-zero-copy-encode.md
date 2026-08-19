# ADR-0003: `VideoToolbox` encode Zero-Copy input (`GpuBufferHandle::Metal`)

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (module `mediaway-encoder::apple`)

## ⚠️ Zero real-hardware / zero compile verification in this session

Same structural constraint as [ADR-0001](0001-videotoolbox-h264-cpu-upload.md)/
[ADR-0002](0002-videotoolbox-hevc-encode.md). Every API name/signature cited below is a direct
read of the locally cloned [`objc2`](https://github.com/madsmtm/objc2) checkout
(`local/vendor-ref/objc2/framework-crates/objc2-core-foundation/src/retained.rs`,
`local/vendor-ref/objc2/generated/CoreVideo/CVPixelBuffer.rs`).

## Context

`VideoInputPreference::ZeroCopyGpu` has returned `EncodeError::Unsupported` since ADR-0001 —
`mediaway_common::GpuBufferHandle::Metal { buffer: NativeHandle }` has existed since before any
Apple backend (per that ADR's own § Deferred), but nothing in this workspace ever constructed or
consumed one: confirmed by grepping every crate for `GpuBufferHandle::Metal` — zero real call
sites anywhere, only doc-comment mentions of the deferred stage.

### `VTCompressionSession::encode_frame` already takes a `&CVPixelBuffer` — CPU upload is not load-bearing

`open_session`'s CPU path builds an owned `CFRetained<CVPixelBuffer>` via `upload_cpu_nv12`
purely to have *something* to pass to `encode_frame(&pixel_buffer, ...)`
(`crates/mediaway-encoder/src/apple/videotoolbox/video.rs`, confirmed by reading the current
call site). `encode_frame`'s real signature (confirmed,
`local/vendor-ref/objc2/generated/VideoToolbox/VTCompressionSession.rs`) takes `&CVPixelBuffer`
by reference — it does not care whether that reference is backed by a buffer this backend just
allocated or one the caller already owns. Zero-Copy input is therefore not a new session-level
capability to build; it is a different way to *obtain* the `&CVPixelBuffer` reference this
backend already needs every `push_frame` call.

## Decision

> `VideoInputPreference::ZeroCopyGpu` borrows the caller's `GpuBufferHandle::Metal` `CVPixelBuffer`
> directly for one `encode_frame` call — no copy, no retain, no release. `open()` accepts both
> `CpuUploadOk` and `ZeroCopyGpu` (session creation is input-path-agnostic); `push_frame` dispatches
> on `frame.storage` and rejects a mismatch against `self.input` as `EncodeError::InvalidInput`
> (a caller that opened `ZeroCopyGpu` but pushes a `Cpu` frame, or vice versa, gets a real error,
> not a silent path switch).

### Ownership: pure borrow, confirmed against the real `CFRetained` API

`objc2_core_foundation::CFRetained<T>` (confirmed, `retained.rs`) exposes:

- `CFRetained::retain(ptr: NonNull<T>) -> Self` — performs a **new** `CFRetain`, independent of
  whatever owns `ptr` already.
- `CFRetained::from_raw(ptr: NonNull<T>) -> Self` — **takes ownership** of an existing `+1`
  (no new retain call; the caller must have actually owned that reference count).
- `CFRetained::into_raw(this: Self) -> NonNull<T>` — releases the wrapper without dropping
  (releasing) the underlying `+1`, handing the raw pointer back out.

This backend uses **none of these** for the encode-input direction — `NonNull::new(buffer.get()
as *mut CVPixelBuffer)` then `unsafe { ptr.as_ref() }` is a **plain reference dereference**, no
retain/release at all. This is deliberately the *simplest* of the three Zero-Copy directions in
this session's series (see the companion decoder/device ADRs) precisely because encode input
needs the buffer valid for only the duration of one `encode_frame` call, with the caller retaining
sole ownership throughout — no lifecycle to manage on this backend's side.

### `PixelBufferRef` — one `encode_frame` call site, two buffer sources

`push_frame` builds a `PixelBufferRef<'a> { Owned(CFRetained<CVPixelBuffer>), Borrowed(&'a
CVPixelBuffer) }` from either input path, then calls `encode_frame(pixel_buffer.as_ref(), ...)`
once — avoids duplicating the `encode_frame` FFI call (and its `// SAFETY:` comment) per branch.
`Owned` (CPU path) drops (frees the upload buffer) after `push_frame` returns, matching today's
existing behavior unchanged; `Borrowed` (Zero-Copy path) never owns anything to drop.

### Geometry validation — real, not skipped for Zero-Copy

The CPU path already validates `data.len() < self.yuv420_bytes`. The Zero-Copy path has no CPU
buffer length to check, but does have `CVPixelBuffer::width()`/`height()` (confirmed real, plain
safe `fn` — not `unsafe fn`, per the generated binding — accessors, same file as the already-used
`pixel_format_type()`/`width_of_plane()` this crate's decoder ADR-0001 cited): validated against
`self.width`/`self.height` before the buffer reaches `encode_frame`, same "no silent guessing"
posture as every other geometry check in this crate.

## Scope (this stage)

**In:**

- `VideoInputPreference::ZeroCopyGpu` for `GpuBufferHandle::Metal` — H.264 and HEVC (this
  backend's full codec scope per ADR-0001/0002), any pixel format `VTCompressionSession` accepts
  from an externally-supplied `CVPixelBuffer` (this backend does not itself constrain the
  caller's buffer's `CVPixelBufferPixelFormatTypeKey` — `VideoToolbox` rejects an incompatible
  one with a real `OSStatus`, surfaced as `EncodeError::Backend`, same as every other backend
  failure path).

**Out (deferred):**

- No `GpuDeviceHandle::Metal` (`MTLDevice`) validation — this backend does not check that the
  caller's buffer came from a compatible device, mirroring the DirectX11 Zero-Copy input path's
  identical scope cut (device/adapter matching is a caller responsibility, undocumented cross-
  device behavior is real but unexplored here, same posture as every existing `GpuBufferHandle`
  variant in this workspace).
- No `IOSurface`-only (non-`CVPixelBuffer`-wrapped) input path — `GpuBufferHandle::Metal`'s own
  doc comment already names both `CVPixelBuffer` and `IOSurface` as valid tokens for this
  variant; this backend narrows to `CVPixelBuffer` specifically (the type `VTCompressionSession`
  natively accepts), consistent with how the decoder/device companion ADRs make the same choice.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Retain the caller's buffer for the call (`CFRetained::retain` + `into_raw` after, mirroring the decoder's Zero-Copy *output* direction) | Unnecessary — `encode_frame` is a synchronous call from this backend's perspective (the async part is the *output* callback, a completely separate `CMSampleBuffer`, not this input buffer); a plain borrowed reference valid for the call's duration is sufficient and is one memory operation cheaper than retain+release for no correctness benefit. |
| Accept `IOSurfaceRef` bits directly (bypass `CVPixelBuffer`) and wrap with `CVPixelBufferCreateWithIOSurface` internally | Rejected as unnecessary indirection — `VTCompressionSession::encode_frame` already accepts `&CVPixelBuffer` natively; wrapping an `IOSurfaceRef` first would add a real allocation this backend does not need, and `GpuBufferHandle::Metal`'s doc already treats `CVPixelBuffer`/`IOSurface` as interchangeable tokens for this variant — a caller can already hand over an `IOSurface`-backed `CVPixelBuffer` (they are commonly the same underlying object on Apple platforms) without this backend doing extra work. |
| Duplicate the `encode_frame` call site per branch instead of `PixelBufferRef` | More code, same behavior — `PixelBufferRef` costs one small enum and reads more clearly at the one real FFI call site that matters (the `unsafe` block with its `// SAFETY:` comment). |

## Consequences

### Positive

- Genuinely Zero-Copy — confirmed no `memcpy`, no `CFRetain`/`CFRelease` on the hot path,
  the cheapest of this session's three Metal Zero-Copy directions.
- `open()`'s session-creation code needed zero changes — confirms `VTCompressionSession` was
  already input-path-agnostic, same finding the companion ADRs report for their own sessions.
- No new `mediaway-common` type or ADR needed — `GpuBufferHandle::Metal` already existed exactly
  shaped for this use.

### Negative / Trade-offs

- **Zero compile verification as authored** — carries over unchanged.
- No cross-device (`GpuDeviceHandle::Metal`) validation — an incompatible buffer's failure mode
  is whatever `OSStatus` `VTCompressionSessionEncodeFrame` happens to return, not a friendlier
  pre-check; acceptable given the identical scope cut on every other platform's Zero-Copy input.

## References

- [ADR-0001](0001-videotoolbox-h264-cpu-upload.md), [ADR-0002](0002-videotoolbox-hevc-encode.md)
- `mediaway-decoder` [ADR-apple/0003](../../../mediaway-decoder/adr/apple/0003-videotoolbox-metal-zero-copy-decode.md) —
  companion decode-output ADR from the same session, the direction that *does* need retain/release
- `crates/mediaway-common/src/gpu.rs` — `GpuBufferHandle::Metal`, `NativeHandle`
- Local grounding source (read directly): `local/vendor-ref/objc2/framework-crates/
  objc2-core-foundation/src/retained.rs` (`CFRetained::{retain,from_raw,into_raw}`),
  `local/vendor-ref/objc2/generated/CoreVideo/CVPixelBuffer.rs` (`width`/`height`),
  `local/vendor-ref/objc2/generated/VideoToolbox/VTCompressionSession.rs` (`encode_frame` real
  signature)
- [`docs/spec/gpu-interop.md`](../../../../docs/spec/gpu-interop.md) (ADR-0005)
- `README.md` § Codec support — Apple OS · GPU H.264/HEVC encode cell: `👻` → `🆗` once implemented

ADRs are written in **English**.
