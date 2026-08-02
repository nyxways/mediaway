# ADR-0001: VideoDecoder streaming trait

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder`

## Context

Decode mirrors encode: streaming, Zero-Copy-capable (GPU out or future shared CPU), sync poll. Compressed `Packet` in → `VideoFrame` out. Marks: [wiki](../../../docs/ai/wiki/zero-copy/marks.md).

## Decision

> Facade owns **sync poll** decoder traits and configs. Sessions live in `mediaway-decoder-<platform>`. No `Box<dyn>` on the hot path.

### Public surface

| Item | Role |
|------|------|
| `VideoDecoderConfig` / `VideoOutputPreference` | Codec, size, timebase, output path, `gpu_device: Option<GpuDeviceHandle>` |
| `VideoDecoder` | `push_packet` → `poll_frame` → `flush` |
| `DecodeError` | Shared errors |
| Input / output | `Packet` / `VideoFrame` |

### Rules

1. Direction opposite of encode: push compressed, poll uncompressed.
2. Zero-Copy default (`VideoOutputPreference::ZeroCopyGpu`); CPU paths documented.
3. GPU frame lifetime: valid until the next recycle (platform ADR).
4. `Type::open` on platform crates.

## References

- Encoder traits: `mediaway-encoder` ADR-0001
- Windows: `mediaway-decoder-windows` ADR-0001
