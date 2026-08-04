# Workspace roadmap

## Platform order (all crates)

**Windows → Web → Linux → other** (Apple, Android, …).

| Priority | Platform | Primary backends |
|----------|----------|------------------|
| 1 | Windows | WMF, DX11/DX12 Zero-Copy |
| 2 | Web | WebCodecs, WebGPU |
| 3 | Linux | VA-API, Vulkan Video |
| 4+ | Other | VideoToolbox, MediaCodec, … |

Codec support matrices (OS / GPU / CPU) and device capture live in the root [`README.md`](../README.md#codec-support).

Detailed work lives in **each crate’s** `docs/roadmap.md`. This file is the index + shared platform policy only.

## Crate roadmaps

Facades (`mediaway-encoder`, `decoder`, `device`) list platform-backend crates in their own roadmaps. Packaging: [`docs/spec/crate-packaging.md`](spec/crate-packaging.md) · ADR-0003.

| Crate | Roadmap |
|-------|---------|
| `mediaway-common` | [`crates/mediaway-common/docs/roadmap.md`](../crates/mediaway-common/docs/roadmap.md) |
| `iso-cenc` | [`crates/iso-cenc/docs/roadmap.md`](../crates/iso-cenc/docs/roadmap.md) |
| `iso-bmff` | [`crates/iso-bmff/docs/roadmap.md`](../crates/iso-bmff/docs/roadmap.md) |
| `ebml-webm` | [`crates/ebml-webm/docs/roadmap.md`](../crates/ebml-webm/docs/roadmap.md) |
| `riff-wave-core` | [`crates/riff-wave-core/docs/roadmap.md`](../crates/riff-wave-core/docs/roadmap.md) |
| `adts-core` | [`crates/adts-core/docs/roadmap.md`](../crates/adts-core/docs/roadmap.md) |
| `mpeg-audio` | [`crates/mpeg-audio/docs/roadmap.md`](../crates/mpeg-audio/docs/roadmap.md) |
| `ogg-core` | [`crates/ogg-core/docs/roadmap.md`](../crates/ogg-core/docs/roadmap.md) |
| `flv-core` | [`crates/flv-core/docs/roadmap.md`](../crates/flv-core/docs/roadmap.md) |
| `mpeg-ts-core` | [`crates/mpeg-ts-core/docs/roadmap.md`](../crates/mpeg-ts-core/docs/roadmap.md) |
| `rtmp` | [`crates/rtmp/docs/roadmap.md`](../crates/rtmp/docs/roadmap.md) |
| `mediaway-container` | [`crates/mediaway-container/docs/roadmap.md`](../crates/mediaway-container/docs/roadmap.md) |
| `mediaway-encoder` | [`crates/mediaway-encoder/docs/roadmap.md`](../crates/mediaway-encoder/docs/roadmap.md) |
| `mediaway-decoder` | [`crates/mediaway-decoder/docs/roadmap.md`](../crates/mediaway-decoder/docs/roadmap.md) |
| `mediaway-device` | [`crates/mediaway-device/docs/roadmap.md`](../crates/mediaway-device/docs/roadmap.md) |
| `mediaway` | [`crates/mediaway/docs/roadmap.md`](../crates/mediaway/docs/roadmap.md) |
| `mediaway-sw` | [`crates/mediaway-sw/docs/roadmap.md`](../crates/mediaway-sw/docs/roadmap.md) |
| `vpl-sys` | [`crates/vpl-sys/README.md`](../crates/vpl-sys/README.md) |
| `iso-bmff-wasm` | [`crates/iso-bmff-wasm/README.md`](../crates/iso-bmff-wasm/README.md) |
| `mediaway-ffi` | [`crates/mediaway-ffi/docs/`](../crates/mediaway-ffi/docs/) (per-module roadmaps) |
| `mediaway-test-media` | [`crates/mediaway-test-media/docs/roadmap.md`](../crates/mediaway-test-media/docs/roadmap.md) |
| `mediaway-avcli` | [`tools/mediaway-avcli/docs/roadmap.md`](../tools/mediaway-avcli/docs/roadmap.md) |
| `mediaway-avprobe` | [`tools/mediaway-avprobe/docs/roadmap.md`](../tools/mediaway-avprobe/docs/roadmap.md) |

OS backends live as `#[cfg]`-gated modules inside the facade crates (`mediaway-encoder`,
`mediaway-decoder`, `mediaway-device`) — their stages are tracked in the facade's roadmap.
Platform backends (`mediaway-*-windows`, …) get their own `docs/roadmap.md` when added to the workspace.  
## Active & Planned Work Items (Wiki & Architecture Backlog)

### 1. High-Level Pipeline & FFI Bindings
- [ ] **Multi-track `EncodeSession`**: Extend `EncodeSession` in `mediaway` to support multi-track (video + audio) muxing natively (currently single-track video; two-track is test-level).
- [ ] **C ABI facade**: Complete the `mediaway-ffi` C ABI surface (container / device / pipeline).
- [ ] **Game Engine & Seamless DX Wrappers**: First-class Zero-Copy wrappers for `wgpu`, `Three.js` (WebGPU), `Unity`, and `Godot` (passing native `GpuBufferHandle` / D3D11 / D3D12 / Vulkan pointers without CPU readback).
- [ ] **Multi-language binding wrappers**: Idiomatic bindings for C++, C# (.NET / Unity), Python, Go, Swift, Kotlin, and Node.js.

### 2. Codecs, Hardware Acceleration & OS Backends
- [ ] **Linux Hardware Verification**: Validate the Linux VA-API backends (`mediaway-encoder::linux`, `mediaway-decoder::linux`) against physical `/dev/dri` device hardware.
- [ ] **Windows D3D12 Decoder Hang Investigation**: Debug GPU driver TDR (`DXGI_ERROR_DEVICE_HUNG`) hang during native D3D12 decoding.
- [ ] **Vulkan Video Decoder/Encoder Refinements**: Resolve HEVC GPU decode zero-pixel output and finalize AV1 Vulkan encode/decode driver support.
- [ ] **Opus Audio Codec Integration**: Wire inbox WMF Opus and `mediaway-sw::opus` into the public `AudioEncoder`/`AudioDecoder` traits.
- [ ] **Pure Rust SW Codec Extensions**: Add CABAC, P-slice, and B-slice decoding to `mediaway-sw` H.264 decoder (currently Baseline CAVLC I-slice only).

### 3. Media Containers, Protocols & Image Formats
- [ ] **Static Image Containers & Codecs**: Expand facade traits and container cores to support image formats (**AVIF**, **HEIC**, **WebP**, **PNG**, **JPEG**).
- [ ] **RTMP Server Verification**: Verify `rtmp` client against live servers (NGINX-RTMP, YouTube Live, Twitch).
- [ ] **Matroska / WebM Extensions**: Map VP8 `CodecKind` and expand Matroska demuxing capabilities (`ebml-webm`).

### 4. Device Capture & Audio DSP
- [ ] **Windows Camera Public Integration**: Wire `IMFSourceReader` camera capture into `mediaway-device` facade traits.
- [ ] **Single-Shot Zero-Copy Capture (`capture_once`)**: Implement `capture_video_once` blocking zero-copy frame retrieval (ADR-0006).
- [ ] **Windows Hotplug Fix**: Resolve crash during `close()` in Windows hotplug monitoring (`mediaway-device::windows::WindowsDeviceHotplug`).
- [ ] **Linux Capture Hardware Verification**: Test `xdg-desktop-portal` ScreenCast, PipeWire mic, and V4L2 camera capture on real Linux installations.

## Workspace bootstrap

- [x] Tooling, conventions, AGENTS.md (ADR-0001)
- [x] Crate scaffolds + per-crate `docs/` / `adr/` / `docs/roadmap.md`
- [x] Crate packaging policy (ADR-0003)
- [x] C-FFI policy (ADR-0004) — per-capability `*-ffi` when Rust MVP is wrappable
- [x] GPU interop policy (ADR-0005) — wgpu / WebGPU / Dawn analogs
- [x] Caveats + code clarity policy (ADR-0006)
- [x] Maturity bar (what a greenfield stack must earn) — [`docs/spec/maturity-bar.md`](spec/maturity-bar.md)
- [ ] Keep this index in sync when crate stages change
