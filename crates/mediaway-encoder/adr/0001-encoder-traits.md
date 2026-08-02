# ADR-0001: VideoEncoder / AudioEncoder streaming traits

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder`

## Context

Encode must stay streaming-first and Zero-Copy-capable ([`docs/spec/async-and-streaming.md`](../../../docs/spec/async-and-streaming.md), [`api-layers.md`](../../../docs/spec/api-layers.md), [wiki marks](../../../docs/ai/wiki/zero-copy/marks.md)). Apps need a stable cross-platform contract while Windows (WMF), Web (WebCodecs), and later platforms differ wildly underneath. `GpuBufferHandle` / `VideoFrame` / `AudioFrame` live in `mediaway-common`.

## Decision

> Facade owns **sync poll** traits and configs. Concrete sessions live in `mediaway-encoder-<platform>`. No `Box<dyn>` on the hot path; callers use concrete backend types (or a thin typed enum later).

### Public surface (`mediaway-encoder`)

| Item | Role |
|------|------|
| [`VideoEncoderConfig`](../src/lib.rs) / [`AudioEncoderConfig`](../src/lib.rs) | Codec, size/rate, bitrate, timebase, input preference |
| [`VideoEncoder`](../src/lib.rs) | `push_frame` → `poll_packet` → `flush` |
| [`AudioEncoder`](../src/lib.rs) | `push_frame` → `poll_packet` → `flush` |
| [`EncodeError`](../src/lib.rs) | Shared errors (`Unsupported`, `InvalidInput`, …) |
| Output | [`mediaway_common::Packet`](../../mediaway-common) (compressed) |
| Input | [`VideoFrame`](../../mediaway-common) / [`AudioFrame`](../../mediaway-common) |

### Rules

1. **Streaming** — push one frame, poll zero-or-more packets; no whole-file encode API in the trait.
2. **Sync core** — traits are sync/`poll`. Async wrappers may appear later as optional adapters (not required here).
3. **Zero-Copy first-class** — `VideoFrameStorage::Gpu` must be acceptable when the backend supports the handle variant; audio may use shared CPU buffers for ⚡. CPU upload / payload-copy paths need honest names/docs (ADR-0006).
4. **Extradata** — backends expose codec config via `stream_info()` (e.g. AVCC) updated after the first keyframe if needed.
5. **Factory** — no mega `open()` that type-erases backends. Depend on `mediaway-encoder-windows` (etc.) for `open`. Optional facade features may re-export later.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Async-only traits | Blocks embedders / games without a runtime |
| `Box<dyn VideoEncoder>` as the only API | Fights ZCA; hides concrete WMF session types |
| CPU `Vec<u8>` frames only | Silently kills Zero-Copy |

## Consequences

### Positive

- Clear contract for WMF/WebCodecs; low-level backends stay first-class

### Negative / Trade-offs

- Apps pick a platform crate (or future feature) instead of one erased factory

## References

- Packaging: ADR-0002 (this crate), workspace ADR-0003
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md)
