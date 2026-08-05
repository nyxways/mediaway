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
- [ ] **Multi-track `EncodeSession`**: native two-track (video + audio) support landed
      (`EncodeSession::open_with_audio` / `write_audio_frame`, `crates/mediaway/src/session.rs`);
      remaining gap is migrating `tests/screen_mic_av_smoke.rs` off its hand-rolled second-track
      muxing onto the native API.
- [ ] **C ABI facade**: container + device C ABI mature and hardware-verified. `mediaway-ffi`'s
      pipeline module now covers encode, decode (`AutoDecoder`,
      `adr/pipeline/0004-auto-decode-c-abi.md`), and a capture-to-encode convenience bridge
      (`adr/pipeline/0005-capture-encode-bridge-c-abi.md`, hardware-verified with a real USB
      camera). Decode's C ABI is implemented and compiles/clippy clean but its own integration
      test is `#[ignore]`d — blocked on a real, pre-existing `WindowsVideoDecoder` CPU-decode
      bug found while adding it (`docs/roadmap.md` § Windows CPU Decode Bug), not a defect in
      the FFI wrapper itself. Shared header types are consolidated into
      `include/mediaway/common.h` (`adr/common/0001-shared-header-consolidation.md`). `cbindgen`
      tooling adopted crate-wide (`docs/adr/0016-cbindgen-ffi-headers.md`) — generates a
      clean-compiling header for the whole crate; the three real `include/mediaway/*.h` headers
      are still hand-written, per-header migration tracked separately (see
      `crates/mediaway-ffi/docs/*/roadmap.md`).
- [ ] **Game Engine & Seamless DX Wrappers**: `mediaway::wgpu` exists (Windows DX12 only) — encode
      bridge hardware-verified, decode bridge construction-only (no pixel round trip yet); no
      `Three.js`/WebGPU or Godot wrapper exists.
- [ ] **Multi-language binding wrappers**: C, C++, C# (.NET / Unity), Python, Node.js, and Browser
      (WASM) are verified (`bindings/`); Go, Swift, and Kotlin have not been started.

### 2. Codecs, Hardware Acceleration & OS Backends
- [ ] **Linux Hardware Verification**: the VA-API backends (`mediaway-encoder`/`mediaway-decoder`
      `linux` modules) are compile-verified via WSL2 only — no run against physical `/dev/dri`
      hardware yet.
- [ ] **Windows D3D12 Decoder Hang Investigation**: still reproduces `DXGI_ERROR_DEVICE_HUNG`
      after 3 real bugs already fixed (readback pitch, NV12 chroma-plane barrier, RBSP bit
      offset); root cause narrowed to the opaque DXVA picture-parameter blob, unresolved
      (`mediaway-decoder/adr/windows/0002-d3d12-native-video-decode.md`).
- [ ] **Vulkan Video Decoder/Encoder Refinements**: HEVC GPU decode still reads back all-zero
      pixels (root cause not found); AV1 encode is structurally hardware-verified but every
      frame's OBU output is invalid — confirmed driver-maturity limitation, not a Mediaway bug;
      AV1 decode has not been started.
- [ ] **Windows CPU Decode Bug**: `WindowsVideoDecoder`'s `CpuFramesOk` H.264 path produces no
      frames (single-packet input) or aborts the process on a Rust std UB check (multi-frame
      muxed/demuxed input) — found 2026-08-05 while adding `mediaway-ffi`'s decode C ABI; root
      cause not found (`docs/ai/wiki/platform/windows-decode.md` § CPU decode bug).
- [x] **Opus Audio Codec Integration (encode)**: `WindowsAudioEncoder` dispatches `CodecKind::Opus`
      to `mediaway-sw`'s `SwOpusAudioEncoder` as a real `AudioEncoder` backend
      (`crates/mediaway-encoder/src/windows/mod.rs`).
- [x] **Opus Audio Codec Integration (decode)**: `mediaway-decoder` gained an `AudioDecoder`
      trait mirroring `VideoDecoder` (ADR-0003), implemented for `WmfOpusDecoder` (Windows) and
      `mediaway-sw`'s `SwOpusAudioDecoder` (cross-platform). No audio `auto`-dispatch
      (`WindowsAudioDecoder`-style backend switcher) exists yet — same follow-up gap as video's
      D3D12 decode integration.
- [ ] **Pure Rust SW Codec Extensions**: Add CABAC, P-slice, and B-slice decoding to `mediaway-sw` H.264 decoder (currently Baseline CAVLC I-slice only).

### 3. Media Containers, Protocols & Image Formats
- [ ] **Static Image Containers & Codecs**: Expand facade traits and container cores to support image formats (**AVIF**, **HEIC**, **WebP**, **PNG**, **JPEG**, **GIF**).
- [ ] **RTMP Server Verification**: handshake digest math is cross-checked against 3 reference
      implementations (FFmpeg/librtmp/SRS) only — a live handshake/connect/publish smoke test
      against a real server (NGINX-RTMP, YouTube Live, Twitch) is still open.
- [x] **Matroska / WebM Extensions**: VP8 `CodecKind` mapped, lacing (Xiph/fixed/EBML) and
      `Cluster` lookahead closed (`ebml-webm` adr/0004, 2026-08-05).

### 4. Device Capture & Audio DSP
- [x] **Windows Camera Public Integration**: `IMFSourceReader` camera capture wired into the
      `mediaway-device` facade (`camera` module), hardware-verified against a real USB webcam.
- [x] **Single-Shot Zero-Copy Capture (`capture_once`)**: `capture_video_once` implemented
      (`mediaway-device::desktop::video`, `::camera::capture`, ADR-0006).
- [x] **Windows Hotplug Fix**: `close()` crash fixed 2026-07-31 — `HotplugSession` now owns its
      `ComGuard` for the object's whole lifetime instead of two independent short-lived scopes.
- [ ] **Linux Capture Hardware Verification**: `xdg-desktop-portal` ScreenCast, PipeWire mic, and
      V4L2 camera capture are compile-verified via WSL2 only — zero runtime verification on a
      real Linux desktop yet.

## Workspace bootstrap

- [x] Tooling, conventions, AGENTS.md (ADR-0001)
- [x] Crate scaffolds + per-crate `docs/` / `adr/` / `docs/roadmap.md`
- [x] Crate packaging policy (ADR-0003)
- [x] C-FFI policy (ADR-0004) — per-capability `*-ffi` when Rust MVP is wrappable
- [x] GPU interop policy (ADR-0005) — wgpu / WebGPU / Dawn analogs
- [x] Caveats + code clarity policy (ADR-0006)
- [x] Maturity bar (what a greenfield stack must earn) — [`docs/spec/maturity-bar.md`](spec/maturity-bar.md)
- [ ] Keep this index in sync when crate stages change
