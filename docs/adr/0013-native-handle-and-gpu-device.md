# ADR-0013: Typed native handles (`NativeHandle`, `GpuDeviceHandle`) + `StreamInfo` video geometry split

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)

## Context

`GpuBufferHandle` and several facade configs (`VideoEncoderConfig`, `VideoDecoderConfig`,
`VideoCaptureConfig`, `AutoVideoEncodeConfig`) store opaque platform pointers as raw `usize`,
with an undocumented-at-the-type-level "`0` = unset" convention repeated across multiple doc
comments.

- Windows backends (`dxgi.rs`, `wgc.rs`, `d3d12_share.rs`, `wmf/dx11.rs`) cast these `usize`
  fields directly via `as *mut std::ffi::c_void` — confirming they are native pointers smuggled
  through an untyped integer, a C-style handle-passing idiom with no type safety.
- `VideoEncoderConfig`, `VideoDecoderConfig`, `VideoCaptureConfig`, and `AutoVideoEncodeConfig`
  all name this field `d3d11_device`, hard-coding a Windows/DX11-specific name into
  cross-platform facade types (`mediaway-encoder`, `mediaway-decoder`, `mediaway-device`).
  This violates the api-layers principle that platform detail should surface as explicit typed
  variants, not be baked by name into the shared surface (`docs/spec/api-layers.md` rule 4).
- `StreamInfo` (`mediaway-common`) always carries `width`/`height`, forcing every non-video
  track (audio, subtitle) to fill in fake `0, 0` geometry (see `examples/mux_roundtrip.rs`,
  `mediaway-container/src/convert.rs`).

## Decision

> Introduce `NativeHandle(NonZeroUsize)` and `GpuDeviceHandle` in `mediaway-common::gpu`;
> retype `GpuBufferHandle`'s pointer-bit fields as `NativeHandle`; replace every facade
> `d3d11_device: usize` field with `gpu_device: Option<GpuDeviceHandle>`; split
> `StreamInfo::{width, height}` into `geometry: Option<VideoGeometry>`.

- `NativeHandle` wraps `NonZeroUsize`. "Unset" is `Option<NativeHandle>::None`, not a `0`
  sentinel. The newtype niches to the same size as `usize` — zero runtime cost. Never
  dereferenced in `mediaway-common` (`forbid(unsafe_code)` stays); platform backends cast to/from
  the real pointer type, same boundary as today.
- `GpuDeviceHandle` mirrors `GpuBufferHandle`'s platform variants, but names **the device that
  owns a buffer** rather than the buffer itself: `DirectX11(NativeHandle)`,
  `DirectX12(NativeHandle)`, `Vulkan(NativeHandle)`, `Metal(NativeHandle)`,
  `WebGpu { device_id: u64 }`. `#[non_exhaustive]`, declared ahead of backend support — same
  precedent as `GpuBufferHandle`.
- Facade configs rename `d3d11_device: usize` → `gpu_device: Option<GpuDeviceHandle>`. No crate
  under `mediaway-encoder` / `mediaway-decoder` / `mediaway-device` (the cross-platform facades)
  may spell a platform name in a field name — only inside `GpuDeviceHandle` / `GpuBufferHandle`
  variant tags and inside `mediaway-*-<platform>` backend crates.
- `StreamInfo.width` / `.height` become `geometry: Option<VideoGeometry>`
  (`struct VideoGeometry { width: u32, height: u32 }`); `None` for audio/subtitle tracks.
  Video-only configs (`VideoEncoderConfig`, `VideoDecoderConfig`, `VideoCaptureConfig`) keep bare
  `width`/`height` — they are never constructed for non-video streams, so no split is needed
  there.
- `CaptureSource::Window { window: usize }` → `NativeHandle` (an `HWND` is a genuine non-null
  pointer). `CaptureSource::Camera { device: usize }` is **left unchanged** — undecided whether
  it is an index (legitimately `0`) or a pointer, and no backend implements camera capture yet.
  See `docs/ai/wiki/device/camera-device-handle.md`.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Keep raw `usize` + doc-comment convention | No type-level guarantee; silently allows an accidental `0` to compile |
| Newtype only `GpuBufferHandle`, leave `d3d11_device` fields as-is | Doesn't fix the DX11 name leaking into cross-platform facade configs |
| Erase device selection entirely; require a separate builder session for Zero-Copy setup | Bigger API surface change than the current gap warrants |

## Consequences

### Positive

- `mediaway-common` implies no pointer semantics without an explicit type; `forbid(unsafe_code)`
  is unaffected.
- Facade types no longer mention any specific OS/graphics API by name — a future Linux/Web port
  does not inherit "d3d11" vocabulary in its config structs.
- Non-video tracks stop carrying fake geometry.

### Negative / Trade-offs

- Breaking API change across `mediaway-common` + 3 facades + 3 Windows backends + examples/README.
  Acceptable pre-1.0 (`docs/spec/status.md`).
- One more type to learn (`NativeHandle`) versus a bare integer.

## References

- spec: `docs/spec/api-layers.md`, `docs/spec/gpu-interop.md`, `docs/spec/zero-cost-abstractions.md`
- related ADR: ADR-0005 (GPU interop), ADR-0009 (ZCA)
- wiki: `docs/ai/wiki/zero-copy/handles.md`, `docs/ai/wiki/common/index.md`

ADRs are written in **English**.
