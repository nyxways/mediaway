# ADR-0001: VideoCapture streaming trait

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device`

## Context

Capture must stay streaming-first and Zero-Copy-capable (GPU **or** shared CPU — [wiki marks](../../../docs/ai/wiki/zero-copy/marks.md)). Apps need a cross-platform contract while Windows (DXGI/WGC/WASAPI), Web (`getUserMedia`), and later platforms differ. Frames use `mediaway-common` `VideoFrame` / `AudioFrame` / `GpuBufferHandle`.

## Decision

> Facade owns **sync poll** capture traits and configs. Concrete sessions live in `mediaway-device-<platform>`. No `Box<dyn>` on the hot path.

### Public surface

| Item | Role |
|------|------|
| `VideoCaptureConfig` / `CaptureSource` | Source, timebase, output preference, `d3d11_device` |
| `VideoCapture` | `poll_frame` → `release_frame` → `close` |
| `CaptureError` | Shared errors (`Unsupported`, `AccessDenied`, …) |
| Output | `VideoFrame` (often `Gpu` + `Bgra8` on Windows screen) |

### Rules

1. **Streaming** — poll one frame at a time; no whole-recording API in the trait.
2. **Explicit GPU lifetime** — DXGI (and similar) require `release_frame` before the next acquire.
3. **Zero-Copy first-class** — default `CaptureOutputPreference::ZeroCopyGpu` for video; audio may earn CPU↔CPU ⚡ via shared PCM. Payload-copy paths need honest names/docs (not silent ⚡).
4. **Factory** — `Type::open` on platform crates (`WindowsScreenCapture::open`).

## Consequences

- Callers must pair poll/release on DXGI; misuse → `CaptureError::Backend`.
- Screen surfaces may be `Bgra8` while encoders expect `Nv12` — color convert is a named GpuCopy, not silent readback.

## References

- Packaging: ADR-0002 (this crate), workspace ADR-0003
- Windows: `mediaway-device-windows` ADR-0001
