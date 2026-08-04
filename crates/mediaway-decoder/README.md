# mediaway-decoder

<p align="center">
  <a href="https://docs.rs/mediaway-decoder"><img src="https://img.shields.io/docsrs/mediaway-decoder" alt="docs.rs"></a>
  <a href="https://crates.io/crates/mediaway-decoder"><img src="https://img.shields.io/crates/v/mediaway-decoder.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Hardware-accelerated video and audio decoding: the `VideoDecoder` trait plus per-platform
backends — Windows WMF (H.264/HEVC/AV1/VP9 hardware decode with DX11 Zero-Copy output,
plus the inbox Opus decoder MFT), Vulkan Video decode, and WebCodecs (wasm, planned).
Streaming `push_packet` / `poll_frame` shape; GPU frames stay on the GPU until the
session recycles the surface.

## Quick start

```rust
use mediaway_common::{CodecKind, PixelFormat, Rational, VideoOutputPreference};
use mediaway_decoder::{VideoDecoder, VideoDecoderConfig};
use mediaway_decoder::windows::WindowsVideoDecoder;

let config = VideoDecoderConfig {
    codec: CodecKind::H264,
    width: 1920, height: 1080,
    time_base: Rational::new(1, 30),
    pixel_format: PixelFormat::Nv12,
    output: VideoOutputPreference::ZeroCopyGpu,
    gpu_device: None,
    extra_data: avcc_bytes,
};

let mut decoder = WindowsVideoDecoder::open(&config)?; // WMF hardware session
decoder.push_packet(&h264_packet)?;
while let Some(frame) = decoder.poll_frame()? { /* DX11 texture or CPU frame */ }
decoder.flush()?;
```

For CPU output frames set `output: VideoOutputPreference::CpuFramesOk`.

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| `VideoDecoder` trait | ✅ | Streaming push/poll API |
| Windows WMF H.264 decode | ✅ | Hardware MFT, DX11 Zero-Copy output |
| Windows WMF HEVC / AV1 / VP9 decode | ✅ | Same DXGI path; MFT may be absent on a machine |
| Windows Opus decode (`WmfOpusDecoder`) | ✅ | Verified end-to-end (ffmpeg-produced Opus → exact PCM) |
| Vulkan Video decode | ✅ | H.264 general-GOP hardware-verified; HEVC GPU path unresolved; AV1 follow-up |
| CPU frame output path | 🛠️ | `CpuFramesOk` policy recognized, backend pending |
| WebCodecs decode (wasm32) | 🛠️ | Planned |
| Linux VA-API decode | 🛠️ | Planned |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway`](../mediaway/) — convenience pipeline (`platform::AutoDecoder`)
- Root [README](../../README.md) — codec support matrices (OS / GPU / vendor)

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
