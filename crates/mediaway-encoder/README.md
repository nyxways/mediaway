# mediaway-encoder

<p align="center">
  <a href="https://docs.rs/mediaway-encoder"><img src="https://img.shields.io/docsrs/mediaway-encoder" alt="docs.rs"></a>
  <a href="https://crates.io/crates/mediaway-encoder"><img src="https://img.shields.io/crates/v/mediaway-encoder.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Hardware-accelerated video and audio encoding: the `VideoEncoder` / `AudioEncoder`
traits, the `auto` selection layer (`AutoVideoEncodeConfig`, path classes from Zero-Copy
down to software), and per-platform backends — Windows WMF (H.264/AAC, DX11 Zero-Copy),
NVIDIA NVENC, Intel Quick Sync (via oneVPL), Vulkan Video, WebCodecs (wasm), and VA-API
on Linux. Feature-gated `audio` / `video` for slim builds.

## Quick start

```rust
use mediaway_common::{CodecKind, PixelFormat, Rational, VideoInputPreference};
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig};
use mediaway_encoder::windows::WindowsVideoEncoder;

let config = VideoEncoderConfig {
    codec: CodecKind::H264,
    width: 1920, height: 1080,
    time_base: Rational::new(1, 30),
    bitrate_bps: 8_000_000,
    pixel_format: PixelFormat::Nv12,
    input: VideoInputPreference::CpuUploadOk,
    gpu_device: None,
};

let mut encoder = WindowsVideoEncoder::open(&config)?; // WMF session
encoder.push_frame(&nv12_frame)?;
while let Some(packet) = encoder.poll_packet()? { /* H.264 packets */ }
encoder.flush()?;
```

For Zero-Copy GPU input, set `input: VideoInputPreference::ZeroCopyGpu` and pass the
matching `gpu_device` — or use `mediaway::platform::AutoEncoder::open` to let Mediaway
pick the best path.

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| `VideoEncoder` / `AudioEncoder` traits | ✅ | Streaming push/poll API |
| Windows WMF H.264 encode | ✅ | CPU upload + DX11 texture Zero-Copy; AAC encode |
| NVIDIA NVENC | ✅ | Hardware-verified H.264 / HEVC / AV1 (CPU upload) |
| Intel Quick Sync (oneVPL) | ✅ | Hardware-verified H.264 / HEVC; AV1 not supported on tested iGPU |
| Vulkan Video encode | ✅ | Hardware-verified H.264 + HEVC; AV1 implemented, driver-blocked |
| WebCodecs encode (wasm32) | ✅ | CPU + `GPUTexture` Zero-Copy via WebGPU canvas |
| Linux VA-API H.264 (CPU upload) | 👻 | Implemented; zero real-hardware verification yet |
| `auto` readback / software path classes | 🛠️ | Policy bits recognized; honest `NoBackend` error today |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway`](../mediaway/) — convenience pipeline (`EncodeSession`) over this crate
- Root [README](../../README.md) — codec support matrices (OS / GPU / vendor)

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
