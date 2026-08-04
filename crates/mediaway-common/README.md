# mediaway-common

<p align="center">
  <a href="https://docs.rs/mediaway-common"><img src="https://img.shields.io/docsrs/mediaway-common" alt="docs.rs"></a>
  <a href="https://crates.io/crates/mediaway-common"><img src="https://img.shields.io/crates/v/mediaway-common.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Shared types for the Mediaway media stack: integer timebases, pixel/sample formats,
packets and frames, stream metadata, and GPU buffer handles. Every Mediaway crate —
encode, decode, container, device — speaks these types, so they are the first thing to
reach for when composing a pipeline.

## Quick start

```rust
use mediaway_common::{Bytes, CodecKind, Rational, StreamInfo, VideoGeometry};

// A 30 fps H.264 video track at 1920x1080.
let stream = StreamInfo::Video {
    id: 0,
    codec: CodecKind::H264,
    time_base: Rational::new(1, 30),
    geometry: VideoGeometry { width: 1920, height: 1080 },
    extra_data: Bytes::new(),
};
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| `Rational` timebase, `PixelFormat` / `SampleFormat` | ✅ | Tested |
| `StreamInfo` / `Packet` / `VideoFrame` / `AudioFrame` | ✅ | Used across encoder/decoder/mux |
| `GpuBufferHandle` / `GpuDeviceHandle` | ✅ | All platform variants declared; platform impls live with their backends |
| Handle ownership / lifetime contract docs | 🛠️ | Planned (filled when GPU sessions land) |
| WebGPU / Metal / Android handle completeness | 🛠️ | Planned as those platforms land |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- Root [README](../../README.md) — codec/container/device support matrices across all crates

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
