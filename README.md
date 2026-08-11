<p align="center">
  <a href="https://github.com/nyxways/mediaway">
    <img src="docs/assets/mediaway-logo.svg" width="180" height="180" alt="Mediaway Logo" style="display: block; margin: 0 auto;">
  </a>
</p>

<h1 align="center">Mediaway</h1>

<p align="center">
  <b>High-Performance Native-First Media Engine</b>
</p>

<p align="center">
  <sub>Zero-Copy Support (GPU/CPU Handles) • Hardware Offloading • Sans-IO Cores • Permissive (MIT / Apache-2.0)</sub>
</p>

<p align="center">
  <a href="https://github.com/nyxways/mediaway/actions/workflows/ci.yml"><img src="https://github.com/nyxways/mediaway/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://nyxways.github.io/mediaway/"><img src="https://img.shields.io/badge/docs-mdBook-blue" alt="Docs"></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
  <a href="docs/spec/status.md"><img src="https://img.shields.io/badge/status-pre--1.0%20%7C%20early%20development-orange" alt="Status"></a>
</p>

<br>

> **Status:** early development (`0.x`), building toward **1.0** with the community.
> **Not recommended for production yet.** Pre-1.0: public APIs, crate layout, and backends may
> change often (no stability guarantee) — contributions and real-hardware testing are what move
> each backend closer to 1.0. Details: [docs/spec/status.md](docs/spec/status.md).

Rust media stack: **high-level pipelines** built from **first-class low-level** APIs — OS/GPU codec sessions, `GpuBufferHandle`, and **sans-io** mux/demux/bitstream cores. Prefer Zero-Copy paths (GPU handles **or** shared CPU buffers); name CPU readback, cross-API copies, and SW fallbacks when they exist.

Design: [docs/spec/vision.md](docs/spec/vision.md).

Covers device capture (camera, mic, screen), encode/decode, containers, and FFI / WASM bindings across Windows, Web, Linux, other. Layout: sans-io cores, facade crates with OS backends as `#[cfg]`-gated modules (e.g. `mediaway-device` contains `windows`/`linux`/`web`). C ABI: a single `mediaway-ffi` facade — [docs/spec/c-ffi.md](docs/spec/c-ffi.md) · [docs/spec/crate-packaging.md](docs/spec/crate-packaging.md).

### Verified Windows slice

The Windows H.264 path has an enabled end-to-end verification slice:

- Rust API: H.264 encode → fragmented MP4 mux/demux → CPU decode, including a
  trim/splice/re-encode round trip (`crates/mediaway/tests/trim_and_splice_windows.rs`).
- C ABI: the same encode → MP4 mux/demux → decode flow through
  `mediaway-ffi`, including decoded pixel-content assertions
  (`crates/mediaway-ffi/tests/decode_smoke.rs`).

These tests skip only when the host has no usable Windows Media Foundation
backend. The slice is verified; broader production readiness, API stability,
and cross-platform hardware coverage remain in progress.

**Repository:** [github.com/nyxways/mediaway](https://github.com/nyxways/mediaway)

**Questions / support:** please open a [GitHub Issue](https://github.com/nyxways/mediaway/issues/new/choose) (English). Do not rely on private email for general inquiries.

## Why Mediaway?

Mediaway resolves the fundamental trade-offs and fragmentation in modern multimedia engineering:

- **OS & Hardware Offloading (Small Binaries & Peak Efficiency)**: Rather than bundling heavy C/Rust software codecs that inflate binary sizes to tens of megabytes and consume high CPU power, Mediaway delegates heavy encoding/decoding workloads to native OS APIs (WMF, VideoToolbox, VA-API, WebCodecs) and dedicated GPU hardware engines (NVENC, QuickSync, AMF). This keeps binary footprints minimal while enabling low-power 4K/8K real-time execution.
- **Zero-Copy Engine & Framework Interop (Seamless DX)**: Designed for zero-copy integration with graphics frameworks and game engines (`wgpu`, `Three.js` / WebGPU, `Unity`, `Godot`). Passes native GPU surface handles (`GpuBufferHandle` / D3D11, D3D12, Vulkan, Metal pointers) directly across capture, processing, encoding, and rendering without CPU readback stalls.
- **Permissive & GPL-Free (MIT OR Apache-2.0)**: Shipped under MIT OR Apache-2.0 with zero LGPL/GPL dependencies (no linking to FFmpeg/`libav*`), eliminating licensing risks for commercial and proprietary applications.
- **Unified & Composable Stack**: Integrates device capture (camera, mic, screen, window), hardware video/audio/image codecs, and container muxing/demuxing into a single cohesive pipeline.
- **Uncompromised Freedom + High-Level Ergonomics**: Provides clean end-to-end abstractions (`mediaway`) while keeping raw GPU handles, **sans-io** state machines, and bitstream timebase controls 100% public and unblocked.

## Quick start

> All examples are in [`examples/`](examples/), one capability per file, grouped by
> sector — [`container/`](examples/container/) (mux/demux only), [`encode/`](examples/encode/) /
> [`decode/`](examples/decode/) (one codec direction, no container), [`device/`](examples/device/)
> (one capture source, no encode), [`pipeline/`](examples/pipeline/) (composed end-to-end flows).
> Run any of them with:
> ```bash
> cargo run --example <name>
> ```

### Mux + demux roundtrip — all platforms

Register tracks, push packets, collect bytes, demux — no OS codec, no unsafe.
The muxer is typed via a typestate (`Open` → `Live`); the demuxer is
fully streaming (feed bytes, poll packets).

```rust
use mediaway_common::{Bytes, CodecKind, Packet, Rational, StreamInfo, VideoGeometry};
use mediaway_container::mp4;

let mut muxer = mp4::Muxer::new();
let v_track = muxer.add_track(StreamInfo::Video {
    id: 0, codec: CodecKind::H264,
    time_base: Rational::new(1, 30),
    geometry: VideoGeometry { width: 1920, height: 1080 },
    extra_data: Bytes::new(),
})?;

let mut muxer = muxer.begin(); // Open -> Live
muxer.push_packet(&Packet {
    stream_id: v_track, pts: 0, dts: 0, duration: 1,
    is_keyframe: true, is_discard: false,
    payload: Bytes::new(),
})?;
muxer.flush();

let mut mp4_bytes: Vec<u8> = Vec::new();
muxer.poll_bytes(&mut mp4_bytes);

let mut demux = mp4::Demuxer::new();
demux.push_bytes(&mp4_bytes);
while let Some(pkt) = demux.poll_packet() { /* … */ }
```

Full runnable example: [`examples/container/mux_demux_mp4.rs`](examples/container/mux_demux_mp4.rs)

---

### Auto encode to MP4 — high-level (Windows WMF backend shown)

`mediaway` composes the low-level `VideoEncoder` trait + `mp4::Muxer` for
you: `platform::AutoEncoder::open` selects the best available path (Zero-Copy GPU →
CPU upload → …), and `EncodeSession` handles the poll-loop → mux wiring so callers
just push frames.

```rust
use mediaway_encoder::auto::AutoVideoEncodeConfig;
use mediaway::{platform, EncodeSession};

let config = AutoVideoEncodeConfig {
    bitrate_bps: 8_000_000,
    // gpu_device: Some(GpuDeviceHandle::DirectX11(handle)), // for Zero-Copy
    ..AutoVideoEncodeConfig::new(CodecKind::H264, 1920, 1080, Rational::new(1, 30))
    // backend defaults to Auto, max_path_class to CpuUpload (ZC -> CPU upload, no readback/SW)
};

let encoder = platform::AutoEncoder::open(&config)?;
let mut session = EncodeSession::open(encoder)?;

session.write_frame(&nv12_frame)?;
let mp4_bytes = session.finish()?; // flush + mux flush + poll_bytes
```

The low-level path (`VideoEncoder` trait + manual `poll_packet`/mux loop) stays fully
public and usable without this crate — see [`examples/encode/encode_h264.rs`](examples/encode/encode_h264.rs)
(encoder in isolation) and [`examples/container/mux_demux_mp4.rs`](examples/container/mux_demux_mp4.rs).

Full runnable example: [`examples/pipeline/encode_to_mp4.rs`](examples/pipeline/encode_to_mp4.rs)

---

### Screen record pipeline — Windows (DXGI + WMF), video + audio

`platform::ScreenCapture` / `platform::Microphone` / `platform::AutoEncoder` are typed
against **facade traits** (`&mut dyn VideoCapture` / `&mut dyn AudioCapture` / `&mut dyn
VideoEncoder`); platform-specific construction is the only OS-specific code, and it
lives entirely in `mediaway::platform`.

```rust
use mediaway_common::Rational;
use mediaway_device::desktop::DesktopVideoCaptureConfig;
use mediaway_device::{Select, AudioCaptureConfig};
use mediaway::{platform, EncodeSession};

let tb = Rational::new(1, 30);
let mut cap = platform::ScreenCapture::open(&DesktopVideoCaptureConfig::screen(Select::Default, tb))?;
let mut mic = platform::Microphone::open(&AudioCaptureConfig::microphone(Rational::new(1, 48_000)))?;
let encoder = platform::AutoEncoder::open(&enc_config)?;
let mut session = EncodeSession::open(encoder)?;

loop {
    if let Some(frame) = cap.poll_frame()? {
        session.write_frame(&frame)?; // handles poll_packet -> mux internally
        cap.release_frame()?;
    }
    // … encode `mic.poll_frame()` PCM to AAC and mux as a second track —
    // full version below composes that against a shared `mp4::Muxer` directly,
    // since `EncodeSession` stays video-only/single-track by design (ADR-0014).
}
```

Full runnable example, with real second-track AAC audio muxed alongside video:
[`examples/pipeline/screen_record.rs`](examples/pipeline/screen_record.rs). For each
capture source on its own (no encode), see [`examples/device/`](examples/device/).

---

### Decode → trim → splice → re-encode — cross-platform (Windows + Linux)

A real non-linear edit built from the low-level `VideoDecoder`/`VideoEncoder` traits +
`mediaway-container` mux/demux — no new `DecodeSession`/`EditTimeline` abstraction. Encodes
two synthetic clips, decodes each back, drops the first/last frame of each (**trim**),
concatenates what's left with renumbered contiguous timestamps (**splice**), and re-encodes.

```rust
use mediaway::platform;

let encoder = platform::AutoEncoder::open(&enc_config)?;
let decoder = platform::AutoDecoder::open(&dec_config)?; // dispatches per-OS under the hood
// … encode two clips, mux, demux, decode, trim by index/PTS, chain + renumber, re-encode
```

Full runnable example: [`examples/pipeline/trim_and_splice.rs`](examples/pipeline/trim_and_splice.rs).
For decode on its own, see [`examples/decode/decode_h264.rs`](examples/decode/decode_h264.rs). Detail:
[`docs/ai/wiki/pipeline/trim-and-splice.md`](docs/ai/wiki/pipeline/trim-and-splice.md).

---

## Language support & FFI

Rust is the primary, always-first-class native API. Bindings for the supported
host languages are **real, verified code** under [`bindings/`](bindings/) — not
aspirational sketches. Multi-language bindings and engine wrappers are
architected with **selective linking**:

- **Per-Capability C ABI (`mediaway-*-ffi`)**: Non-Rust applications link only the specific C ABI crates or features they need (e.g. `mediaway-container-ffi` for standalone MP4/WebM muxing, or `mediaway-encoder-ffi` for hardware encoding). This ensures host applications in C, C++, C#, Python, and Node.js keep their own binary sizes minimal without linking unused codecs or capture drivers.
- **Seamless Engine & Graphics Wrappers (`wgpu`, Three.js, Unity, Godot)**: High-level binding wrappers designed to hand off native GPU texture pointers (`GpuBufferHandle` / D3D11, D3D12, Vulkan, Metal) between real-time game engines or renderers and Mediaway's hardware pipeline with zero CPU copy overhead.
- **Browser WebAssembly (WASM)**: Web hosts target WASM (`wasm-bindgen`) directly integrated with native browser WebCodecs and WebGPU APIs — bypassing the C ABI entirely for zero-overhead browser execution.

| Language / Host | Interop Path | Status | Target Ergonomics & Use Cases | Examples |
|-----------------|--------------|--------|-------------------------------|----------|
| **Rust** | Native crates | ✅ primary | Primary API (100% Zero-Copy, Sans-IO, traits) | [`examples/`](examples/) |
| [**C**](bindings/c/) | Direct C ABI | ✅ verified | ABI contract baseline, zero-overhead FFI | [`bindings/c/examples/`](bindings/c/examples/) |
| [**C++**](bindings/cpp/) | Thin RAII C ABI | ✅ verified | Native desktop apps, custom engines, rendering pipelines | [`bindings/cpp/examples/`](bindings/cpp/examples/) |
| [**C# (.NET)**](bindings/csharp/) | P/Invoke | ✅ verified | Windows desktop, WPF/WinUI, Unity native plugins | [`bindings/csharp/`](bindings/csharp/) |
| [**Python**](bindings/python/) | `ctypes` / `cffi` | ✅ verified | Data processing pipelines, ML model input/output streams | [`bindings/python/examples/`](bindings/python/examples/) |
| [**Node.js (TS)**](bindings/nodejs/) | Native Addon / N-API (koffi FFI today) | ✅ verified | Node.js server-side video processing and CLI tools | [`bindings/nodejs/examples/`](bindings/nodejs/examples/) |
| [**Browser (TS)**](bindings/browser/) | WASM + WebCodecs | ✅ verified | Browser-native high-performance video apps (no C FFI) | [`bindings/browser/`](bindings/browser/) |
| **Unity / Godot** | Native GPU Plugin | Planned | Seamless Zero-Copy GPU texture sharing & camera/screen record | — |

Status legend: ✅ verified = real binding source built and run against the native
libraries; 📐 design = README brief + example code only (nothing compiles/ships
yet). Per-language detail lives in [`bindings/README.md`](bindings/README.md)
and each folder's own README.

Node.js (C ABI FFI) and the Browser (WASM + WebCodecs) are two distinct JS/TS environments with distinct interop paths — see [`docs/spec/c-ffi.md`](docs/spec/c-ffi.md) § Tier C.

---

## Codec support

<!-- ANCHOR: codec-support -->

| Mark | Meaning                                                                       |
| ---- | ----------------------------------------------------------------------------- |
| ✅    | First-class (tests for claimed scope)                                         |
| ⚡    | Zero-Copy path — **no payload `memcpy`** (GPU handle **or** shared CPU buffer; implies ✅) |
| 🆗   | Best-effort / prototype                                                       |
| 🛠️  | Planned                                                                       |
| ❌   | Attempted and genuinely blocked — no upstream API to build on, a hard version/license conflict, or a real query returned "unsupported." Not a "ran out of time" 🛠️. |
| 👻   | Not exercisable yet — **license/patent blocked**, no target hardware, out of scope, or no device/daemon/session available to run otherwise-tested code against |


Cell = **encode/decode**. One mark means both; `A/B` means encode / decode.  
For now only **Windows · Web · Linux** are planned; Apple / Android (and Metal / Apple / Qualcomm) are 👻.

**Zero-Copy honesty:** ⚡ means no payload copy — a GPU handle **or** a shared CPU buffer, not "GPU only." Allocating a new `Vec` and copying into it is 🆗, not ⚡.

### OS · CPU

OS codec APIs (WMF, WebCodecs, VA-API, …) fed with CPU buffers (upload may apply); ⚡ here means a shared/borrowed buffer, not software encode.


| Codec        | Windows  | Web      | Linux | Apple | Android |
| ------------ | -------- | -------- | ----- | ----- | ------- |
| H.264 / AVC  | 🆗 / 🆗  | 🆗 / 🆗 | 👻   | 👻    | 👻      |
| HEVC / H.265 | 🆗 / 🆗 | ❌ / 🆗 | 🛠️   | 👻    | 👻      |
| AV1          | 🛠️ / 🛠️ | 🆗      | 🛠️   | 👻    | 👻      |
| VP9          | 🆗 / 🆗 | 🆗      | 🛠️   | 👻    | 👻      |
| ProRes       | 👻       | 👻       | 👻    | 👻    | 👻      |
| AAC          | 🆗       | 🆗       | 🛠️   | 👻    | 👻      |
| Opus         | 🆗 / 🆗 | 🛠️      | 🛠️   | 👻    | 👻      |

> Windows Opus: **encode** runs through `mediaway-sw` (no inbox encoder MFT exists —
> verified via `MFTEnumEx`), wired into `WindowsAudioEncoder`; **decode** uses the inbox
> decoder MFT session (`CMSOpusDecMFT`), public as
> `mediaway_decoder::windows::WmfOpusDecoder` — both verified end-to-end
> (encode→Ogg→ffprobe 2.000 s + mpv; decode of ffmpeg-produced Opus → exact PCM).


Detail: backends live as `#[cfg]`-gated modules — `mediaway-decoder::{windows, web, linux}`, `mediaway-encoder::{windows, web, linux}`.

### OS · GPU

Same OS APIs with GPU surfaces (`GpuBufferHandle`, DXGI, …). Video only — audio Zero-Copy lives under OS · CPU.


| Codec        | Windows | Web | Linux | Apple | Android |
| ------------ | ------- | --- | ----- | ----- | ------- |
| H.264 / AVC  | ⚡ / 🆗  | 🆗 | 🛠️   | 👻    | 👻      |
| HEVC / H.265 | 🆗 / 🆗  | 🛠️ | 🛠️   | 👻    | 👻      |
| AV1          | 🆗 / 🆗  | 🛠️ | 🛠️   | 👻    | 👻      |
| VP9          | 🆗 / 🆗  | 🛠️ | 🛠️   | 👻    | 👻      |
| ProRes       | 👻      | 👻  | 👻    | 👻    | 👻      |


Detail: `mediaway-encoder::{windows, web}` (`windows` = WMF + DX11 Zero-Copy; `web` = WebCodecs + WebGPU).

### GPU — by API

**Graphics interop** (D3D11, Vulkan, Metal, …) — which API your textures use. Orthogonal to OS · CPU/GPU. **Video only.**

Adapters: [`mediaway`](crates/mediaway/README.md) `wgpu` module 🆗 (DX12 ↔ WMF `GpuCopy` bridges).


| Codec        | D3D11  | D3D12 | Vulkan | Metal |
| ------------ | ------ | ----- | ------ | ----- |
| H.264 / AVC  | ⚡ / 🆗 | 🆗 / 🛠️ | 🆗 / 🆗 | 👻    |
| HEVC / H.265 | 🆗 / 🆗 | 🆗 / 🛠️ | 🆗 / 🆗 | 👻    |
| AV1          | 🆗 / 🆗 | 🛠️    | ❌ / 🛠️ | 👻    |
| VP9          | 🆗 / 🆗 | 👻    | 👻     | 👻    |


Detail: [`mediaway`](crates/mediaway/README.md) `wgpu` module · `mediaway-encoder::{windows, vulkan}` · `mediaway-decoder::{windows, vulkan}`.

### GPU — by vendor

**Vendor SDKs** (NVENC, AMF, …) — separate from OS + graphics interop. Not the default Auto path. **Video only.**

- **NVIDIA** — `mediaway-encoder::nvenc` ([`mediaway-encoder`](crates/mediaway-encoder/README.md)), **hardware-verified** H.264/HEVC/AV1 CPU-upload encode.
- **Intel** — `mediaway-encoder::quicksync` ([`mediaway-encoder`](crates/mediaway-encoder/README.md)), **hardware-verified** H.264/HEVC encode; AV1 is ❌ (no hardware support on this iGPU generation).
- **AMD** — AMF backend 🛠️ deferred (binding/dependency blockers, not an AMD capability gap).

| Codec        | NVIDIA | AMD | Intel | Apple | Qualcomm |
| ------------ | ------ | --- | ----- | ----- | -------- |
| H.264 / AVC  | 🆗     | 🛠️ | 🆗   | 👻    | 👻       |
| HEVC / H.265 | 🆗    | 🛠️ | 🆗   | 👻    | 👻       |
| AV1          | 🆗    | 🛠️ | ❌   | 👻    | 👻       |
| VP9          | 👻     | 👻  | 👻    | 👻    | 👻       |


### CPU / SW

**Pure Rust sans-io** software codecs — no C codec FFI (`OpenH264`, `libvpx`, …). Opt-in only; never a silent HW fallback. Detail: [`mediaway-sw`](crates/mediaway-sw/README.md) (H.264 decode, AV1/Opus encode, PCM, audio processing).


| Codec                                          | Status |
| ------------------------------------------------ | ------ |
| [H.264 / AVC](crates/mediaway-sw/README.md)       | 🆗     |
| HEVC / H.265                                      | 👻     |
| [AV1](crates/mediaway-sw/README.md)               | 🆗     |
| VP9                                                | 👻     |
| AAC                                                | 👻     |
| [Opus](crates/mediaway-sw/README.md)         | 🆗     |
| [PCM / raw](crates/mediaway-sw/README.md)         | 🆗     |

<!-- ANCHOR_END: codec-support -->

## Container support

<!-- ANCHOR: container-support -->

Freestanding mux/demux cores plus the `mediaway-container` facade (wraps all eight as Mediaway-typed `StreamInfo`/`Packet` modules: `mp4`, `webm`, `wav`, `adts`, `mp3`, `ogg`, `flv`, `ts`). Same marks as § Codec support — not every 🆗-mux implements the shared `Mux` trait; see [`mediaway-container`](crates/mediaway-container/README.md) for which.

| Format                                          | Mux | Demux |
| ------------------------------------------------ | --- | ----- |
| [MP4 / fMP4](crates/iso-bmff/README.md)           | 🆗  | 🆗    |
| [WebM](crates/ebml-webm/README.md)                | 🆗  | 🆗    |
| [WAV / RIFF (PCM)](crates/riff-wave-core/README.md)    | 🆗  | 🆗    |
| [ADTS (raw AAC)](crates/adts-core/README.md)           | 🆗  | 🆗    |
| [MP3 (MPEG Layer III)](crates/mpeg-audio/README.md) | 🆗  | 🆗    |
| [Ogg](crates/ogg-core/README.md)                       | 🆗  | 🆗    |
| [FLV](crates/flv-core/README.md)                       | 🆗  | 🆗    |
| [MPEG-TS](crates/mpeg-ts-core/README.md)               | 🆗  | 🆗    |

<!-- ANCHOR_END: container-support -->

## Device

<!-- ANCHOR: device-capture -->

What `mediaway-device` backends target (camera, mic, **screen**, **window**). Same marks as § Codec support. **⚡** = Zero-Copy out (GPU `GpuBufferHandle` **or** shared CPU PCM without payload copy); cell = **CPU capture / GPU surface** where both apply (`🆗 / ⚡`).


| Source           | [Windows](crates/mediaway-device/README.md) | [Web](crates/mediaway-device/README.md) | [Linux](crates/mediaway-device/README.md) | Apple | Android |
| ---------------- | ------- | --- | ----- | ----- | ------- |
| Camera (video)   | 🆗      | 🆗  | 👻   | 👻    | 👻      |
| Microphone       | 🆗      | 🆗  | 👻   | 👻    | 👻      |
| Screen / display | ⚡       | 🆗  | 👻   | 👻    | 👻      |
| Window           | 🆗      | 🆗  | 👻   | 👻    | 👻      |

<!-- ANCHOR_END: device-capture -->

## Crates

<!-- ANCHOR: crates -->

| Crate | Role |
| -------------------------- | ----------------------------------------------------------- |
| `mediaway-common`          | Shared types (`Rational`, formats, `GpuBufferHandle`, packets/frames) |
| `iso-bmff`                 | MP4 / ISOBMFF mux + demux core (fMP4, ClearKey CENC) |
| `iso-cenc`                 | ClearKey CENC sample crypto (AES-128-CTR) |
| `ebml-webm`                | EBML / WebM mux + demux core |
| `riff-wave-core`           | WAV / RIFF PCM mux + demux core |
| `adts-core`                | ADTS (raw AAC) mux + demux core |
| `mpeg-audio`               | MP3 (MPEG Layer III) mux + demux core |
| `ogg-core`                 | Ogg page/packet mux + demux core |
| `flv-core`                 | FLV tag mux + demux core |
| `mpeg-ts-core`             | MPEG-2 Transport Stream mux + demux core |
| `rtp-core`                 | RTP payloadization for H.264/HEVC (RFC 3550/6184/7798) |
| `rtmp`                     | RTMP publish-client handshake + chunk stream + AMF0 command mux |
| `mediaway-container`       | Container facade: shared traits + typed `mp4`/`webm`/`wav`/`adts`/`mp3`/`ogg`/`flv`/`ts` |
| `mediaway-encoder`         | Encode traits + `auto` selection; Windows WMF / NVENC / QuickSync / Vulkan / WebCodecs / VA-API backends |
| `mediaway-decoder`         | Decode traits; Windows WMF (HW, DX11 Zero-Copy) / Vulkan / WebCodecs backends |
| `mediaway-device`          | Capture + playback traits; Windows DXGI/WGC/WASAPI, Linux portal+PipeWire+V4L2, Web getUserMedia/getDisplayMedia |
| `mediaway`                 | Convenience pipeline (`EncodeSession` + `platform` auto-dispatch + wgpu interop) |
| `mediaway-sw`              | Pure Rust sans-io software codecs (H.264 decode, AV1/Opus encode, PCM, audio processing) |
| `vpl-sys`                  | oneVPL FFI bindings (runtime-loaded; no build-time link) |
| `iso-bmff-wasm`            | WASM bindings for `iso-bmff` (browser) |
| `mediaway-ffi`             | Single C ABI facade (container / device / pipeline) |
| `mediaway-test-media`      | Rust-generated test fixtures (local cache only) |
| `mediaway-avcli`           | AV CLI (mux; not affiliated with FFmpeg) |
| `mediaway-avprobe`         | Media probe CLI (not affiliated with FFmpeg) |

<!-- ANCHOR_END: crates -->


OS backends live as `#[cfg]`-gated modules inside the facade crates
(`mediaway-encoder`, `mediaway-decoder`, `mediaway-device`).

## Dev setup

```bash
# Toolchain (rust-toolchain.toml pins stable)
rustup show

# Hooks
cargo install lefthook cargo-deny
lefthook install

# CI (GitHub Actions) runs the same gates on push/PR to main — see docs/conventions/hooks.md

# Optional
cargo install cargo-nextest gitleaks   # or scoop/brew for gitleaks
```

## Docs

- [CONTRIBUTING.md](CONTRIBUTING.md) — how to contribute
- [docs/contributing/pull-requests.md](docs/contributing/pull-requests.md) — PR checklist (doc sync, quality gates, …)
- [docs/contributing/for-agents.md](docs/contributing/for-agents.md) — for AI assistants helping contributors
- [docs/contributing/](docs/contributing/) — getting started, docs map, PRs
- [docs/spec/](docs/spec/) — product vision and design
- [docs/conventions/](docs/conventions/) — commits, hooks, style, license, testing
- [docs/roadmap.md](docs/roadmap.md) — platform order and crate roadmap index
- Codec support tables — this README (§ Codec support)
- Container support table — this README (§ Container support)
- Device table — this README (§ Device)
- Per crate: `crates/<name>/README.md`, `crates/<name>/docs/roadmap.md`, `crates/<name>/adr/`

## License & dependencies

- **License:** MIT OR Apache-2.0 — [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE).
- **Cargo graph:** no GPL/LGPL (etc.) deps; no linking `libav`* / FFmpeg libraries in shipped crates. See [docs/spec/vision.md](docs/spec/vision.md) § License & dependency boundary.
- System `ffmpeg` / `ffprobe` on `PATH` are optional **test/dev oracles** only ([ADR-0002](docs/adr/0002-system-oracle.md)).
