# mediaway

<p align="center">
  <a href="https://docs.rs/mediaway"><img src="https://img.shields.io/docsrs/mediaway" alt="docs.rs"></a>
  <a href="https://crates.io/crates/mediaway"><img src="https://img.shields.io/crates/v/mediaway.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

The Mediaway convenience pipeline: composes encoder + container (+ device capture for
input) so apps don't hand-roll the encoder → muxer poll loop. `EncodeSession` wraps an
encoder and an MP4 muxer behind `write_frame`/`finish`, `FrameFilter` gives you a
mid-pipeline transform hook, and `platform` auto-selects the best available backend per
OS. The low-level traits stay fully public and reachable without this crate.

## Quick start

```rust
use mediaway::{platform, EncodeSession};
use mediaway_common::{CodecKind, Rational};
use mediaway_encoder::auto::AutoVideoEncodeConfig;

let config = AutoVideoEncodeConfig {
    bitrate_bps: 8_000_000,
    ..AutoVideoEncodeConfig::new(CodecKind::H264, 1920, 1080, Rational::new(1, 30))
};

let encoder = platform::AutoEncoder::open(&config)?; // per-OS backend selection
let mut session = EncodeSession::open(encoder)?;

session.write_frame(&nv12_frame)?;
let mp4_bytes = session.finish()?; // flush + mux flush + poll_bytes
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

### Pipeline features (OS-independent)

| Area | Status | Notes |
| ---- | ------ | ----- |
| `EncodeSession` (open / write_frame / finish) | ✅ | Video track; optional second audio track via `open_with_audio` |
| `FrameFilter` mid-pipeline hook | ✅ | CPU frames; GPU-backed frames fail loudly (`GpuFrameUnsupported`) |
| APM / VAD wiring (AEC3 + NS + AGC2, RNN VAD) | ✅ | `attach_audio_processor` / `attach_vad` / `poll_vad_score` |

### `platform` auto-dispatch by OS

`platform::AutoEncoder` / `AutoDecoder` / `ScreenCapture` / `Microphone` auto-select the
best backend per OS. `❌ NoBackend` means the capability isn't wired into `platform` yet —
reach the backend module directly (e.g. `mediaway_encoder::web`) instead.

| Capability | Windows | Linux | Web (wasm) | Other (macOS / Android) |
| ---------- | ------- | ----- | ---------- | ------------------------ |
| `AutoEncoder::open` | ✅ `windows::auto` (path selection: Zero-Copy → CPU upload; WMF H.264/AAC, NVENC, QuickSync, Vulkan) | 🆗 `linux::LinuxVideoEncoder` (VA-API, CPU upload only; zero real-hardware verification) | ❌ `NoBackend` (`mediaway_encoder::web` WebCodecs exists, not wired) | ❌ `NoBackend` |
| `AutoDecoder::open` | ✅ `windows::WindowsVideoDecoder` (WMF HW decode, DX11 Zero-Copy out) | 🆗 `linux::LinuxVideoDecoder` (VA-API CPU output; unverified) | ❌ `NoBackend` (`mediaway_decoder::web` planned) | ❌ `NoBackend` |
| `ScreenCapture::open` | ✅ ⚡ DXGI Desktop Duplication (Zero-Copy out) | 🆗 `linux::LinuxScreenCapture` (portal + PipeWire, CPU copy; unverified) | ❌ `NoBackend` (`getDisplayMedia` not wired) | ❌ `NoBackend` |
| `Microphone::open` | ✅ WASAPI | ❌ `NoBackend` (Linux mic module exists, not wired) | ❌ `NoBackend` | ❌ `NoBackend` |
| `encoder_support(codec)` | ✅ live probe (incl. Opus via `mediaway-sw` software path) | empty | empty | empty |
| `decoder_support(codec)` | ✅ live probe (incl. inbox WMF Opus decoder) | ✅ live probe (VA-API) | `NotImplemented` | `NotImplemented` |
| `device_support` / `request_device_permission` | ✅ | ✅ | — | — |

Remaining wiring (`platform` dispatch for Web, Linux microphone, camera, and other
platforms) follows the workspace platform order — see [`docs/roadmap.md`](docs/roadmap.md).

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway-encoder`](../mediaway-encoder/) — the `VideoEncoder` trait this composes
- [`mediaway-container`](../mediaway-container/) — the muxer this composes
- Root [README](../../README.md) — codec/container/device support matrices

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
