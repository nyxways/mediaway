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
| `mediaway-ffi` (shared internal helper, rlib-only, no C ABI) | [`crates/mediaway-ffi/docs/roadmap.md`](../crates/mediaway-ffi/docs/roadmap.md) |
| `iso-cenc` (unprefixed ClearKey CENC) | [`crates/iso-cenc/docs/roadmap.md`](../crates/iso-cenc/docs/roadmap.md) |
| `mediaway-container` (facade) | [`crates/mediaway-container/docs/roadmap.md`](../crates/mediaway-container/docs/roadmap.md) |
| `mediaway-ffi` (C ABI, first `*-ffi` crate) | [`crates/mediaway-ffi/docs/roadmap.md`](../crates/mediaway-ffi/docs/roadmap.md) |
| `mediaway` (facade-of-facades) | [`crates/mediaway/docs/roadmap.md`](../crates/mediaway/docs/roadmap.md) |
| `mediaway-ffi` (C ABI, second `*-ffi` crate) | [`crates/mediaway-ffi/docs/roadmap.md`](../crates/mediaway-ffi/docs/roadmap.md) |
| `iso-bmff` (unprefixed ISOBMFF/MP4) | [`crates/iso-bmff/docs/roadmap.md`](../crates/iso-bmff/docs/roadmap.md) |
| `ebml-webm` (unprefixed EBML/WebM+Matroska demux) | [`crates/ebml-webm/docs/roadmap.md`](../crates/ebml-webm/docs/roadmap.md) |
| `riff-wave` (unprefixed WAV/PCM) | [`crates/riff-wave/docs/roadmap.md`](../crates/riff-wave/docs/roadmap.md) |
| `adts` (unprefixed raw AAC) | [`crates/adts/docs/roadmap.md`](../crates/adts/docs/roadmap.md) |
| `mpeg-audio` (unprefixed MP3/Layer III) | [`crates/mpeg-audio/docs/roadmap.md`](../crates/mpeg-audio/docs/roadmap.md) |
| `ogg` (unprefixed Ogg page/packet) | [`crates/ogg/docs/roadmap.md`](../crates/ogg/docs/roadmap.md) |
| `flv` (unprefixed FLV) | [`crates/flv/docs/roadmap.md`](../crates/flv/docs/roadmap.md) |
| `mpeg-ts` (unprefixed MPEG-2 Transport Stream) | [`crates/mpeg-ts/docs/roadmap.md`](../crates/mpeg-ts/docs/roadmap.md) |
| `rtmp` (unprefixed RTMP publish client, handshake/chunk/AMF0 implemented, ADR-0001) | [`crates/rtmp/docs/roadmap.md`](../crates/rtmp/docs/roadmap.md) |
| `mediaway-encoder` (facade) | [`crates/mediaway-encoder/docs/roadmap.md`](../crates/mediaway-encoder/docs/roadmap.md) |
| `mediaway-encoder-windows` | [`crates/mediaway-encoder-windows/docs/roadmap.md`](../crates/mediaway-encoder-windows/docs/roadmap.md) |
| `mediaway-encoder-linux` | [`crates/mediaway-encoder-linux/docs/roadmap.md`](../crates/mediaway-encoder-linux/docs/roadmap.md) |
| `mediaway-encoder-vulkan` | [`crates/mediaway-encoder-vulkan/docs/roadmap.md`](../crates/mediaway-encoder-vulkan/docs/roadmap.md) |
| `mediaway-encoder-nvenc` | [`crates/mediaway-encoder-nvenc/docs/roadmap.md`](../crates/mediaway-encoder-nvenc/docs/roadmap.md) |
| `mediaway-encoder-quicksync` | [`crates/mediaway-encoder-quicksync/docs/roadmap.md`](../crates/mediaway-encoder-quicksync/docs/roadmap.md) |
| `vpl-sys` (unprefixed oneVPL FFI core) | [`crates/vpl-sys/README.md`](../crates/vpl-sys/README.md) |
| `mediaway-wgpu` | [`crates/mediaway-wgpu/docs/roadmap.md`](../crates/mediaway-wgpu/docs/roadmap.md) |
| `mediaway-decoder` (facade) | [`crates/mediaway-decoder/docs/roadmap.md`](../crates/mediaway-decoder/docs/roadmap.md) |
| `mediaway-decoder-windows` | [`crates/mediaway-decoder-windows/docs/roadmap.md`](../crates/mediaway-decoder-windows/docs/roadmap.md) |
| `mediaway-decoder-linux` | [`crates/mediaway-decoder-linux/docs/roadmap.md`](../crates/mediaway-decoder-linux/docs/roadmap.md) |
| `mediaway-decoder-vulkan` | [`crates/mediaway-decoder-vulkan/docs/roadmap.md`](../crates/mediaway-decoder-vulkan/docs/roadmap.md) |
| `mediaway-device` (facade) | [`crates/mediaway-device/docs/roadmap.md`](../crates/mediaway-device/docs/roadmap.md) |
| `mediaway-device-windows` | [`crates/mediaway-device-windows/docs/roadmap.md`](../crates/mediaway-device-windows/docs/roadmap.md) |
| `mediaway-device-linux` | [`crates/mediaway-device-linux/docs/roadmap.md`](../crates/mediaway-device-linux/docs/roadmap.md) |
| `mediaway-ffi` (C ABI, third `*-ffi` crate) | [`crates/mediaway-ffi/docs/roadmap.md`](../crates/mediaway-ffi/docs/roadmap.md) |
| `iso-bmff-wasm` | [`crates/iso-bmff-wasm/README.md`](../crates/iso-bmff-wasm/README.md) |
| `mediaway-encoder-web` | [`crates/mediaway-encoder-web/README.md`](../crates/mediaway-encoder-web/README.md) |
| `mediaway-device-web` | [`crates/mediaway-device-web/README.md`](../crates/mediaway-device-web/README.md) |
| `mediaway-decoder-web` | [`crates/mediaway-decoder-web/README.md`](../crates/mediaway-decoder-web/README.md) |
| `mediaway-sw` | [`crates/mediaway-sw/docs/roadmap.md`](../crates/mediaway-sw/docs/roadmap.md) |
| `mediaway-audio-apm` | [`crates/mediaway-audio-apm/docs/roadmap.md`](../crates/mediaway-audio-apm/docs/roadmap.md) |
| `mediaway-sw-opus` | [`crates/mediaway-sw-opus/docs/roadmap.md`](../crates/mediaway-sw-opus/docs/roadmap.md) |
| `mediaway-test-media` | [`crates/mediaway-test-media/docs/roadmap.md`](../crates/mediaway-test-media/docs/roadmap.md) |
| `mediaway-avcli` | [`tools/mediaway-avcli/docs/roadmap.md`](../tools/mediaway-avcli/docs/roadmap.md) |
| `mediaway-avprobe` | [`tools/mediaway-avprobe/docs/roadmap.md`](../tools/mediaway-avprobe/docs/roadmap.md) |

Platform backends (`mediaway-*-windows`, …) get their own `docs/roadmap.md` when added to the workspace.  
## Active & Planned Work Items (Wiki & Architecture Backlog)

### 1. High-Level Pipeline & FFI Bindings
- [ ] **Multi-track `EncodeSession`**: Extend `EncodeSession` in `mediaway` to support multi-track (video + audio) muxing natively (currently single-track video; two-track is test-level).
- [ ] **Per-capability C-FFI crates**: Complete implementation of `mediaway-ffi`, `mediaway-ffi`, and `mediaway-ffi`.
- [ ] **Game Engine & Seamless DX Wrappers**: First-class Zero-Copy wrappers for `wgpu`, `Three.js` (WebGPU), `Unity`, and `Godot` (passing native `GpuBufferHandle` / D3D11 / D3D12 / Vulkan pointers without CPU readback).
- [ ] **Multi-language binding wrappers**: Idiomatic bindings for C++, C# (.NET / Unity), Python, Go, Swift, Kotlin, and Node.js.

### 2. Codecs, Hardware Acceleration & OS Backends
- [ ] **Linux Hardware Verification**: Validate `mediaway-encoder-linux` and `mediaway-decoder-linux` (VA-API) against physical `/dev/dri` device hardware.
- [ ] **Windows D3D12 Decoder Hang Investigation**: Debug GPU driver TDR (`DXGI_ERROR_DEVICE_HUNG`) hang during native D3D12 decoding.
- [ ] **Vulkan Video Decoder/Encoder Refinements**: Resolve HEVC GPU decode zero-pixel output and finalize AV1 Vulkan encode/decode driver support.
- [ ] **Opus Audio Codec Integration**: Wire inbox WMF Opus and `mediaway-sw-opus` into the public `AudioEncoder`/`AudioDecoder` facade traits.
- [ ] **Pure Rust SW Codec Extensions**: Add CABAC, P-slice, and B-slice decoding to `mediaway-sw` H.264 decoder (currently Baseline CAVLC I-slice only).

### 3. Media Containers, Protocols & Image Formats
- [ ] **Static Image Containers & Codecs**: Expand facade traits and container cores to support image formats (**AVIF**, **HEIC**, **WebP**, **PNG**, **JPEG**).
- [ ] **RTMP Server Verification**: Verify `rtmp` client against live servers (NGINX-RTMP, YouTube Live, Twitch).
- [ ] **Matroska / WebM Extensions**: Map VP8 `CodecKind` and expand Matroska demuxing capabilities.

### 4. Device Capture & Audio DSP
- [ ] **Windows Camera Public Integration**: Wire `IMFSourceReader` camera capture into `mediaway-device` facade traits.
- [ ] **Single-Shot Zero-Copy Capture (`capture_once`)**: Implement `capture_video_once` blocking zero-copy frame retrieval (ADR-0006).
- [ ] **Windows Hotplug Fix**: Resolve crash during `close()` in `mediaway-ffi` hotplug monitoring.
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
