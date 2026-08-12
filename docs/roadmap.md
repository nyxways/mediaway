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
| `rtp-core` | [`crates/rtp-core/docs/roadmap.md`](../crates/rtp-core/docs/roadmap.md) |
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
- [x] **Multi-track `EncodeSession`**: native two-track (video + audio) support landed
      (`EncodeSession::open_with_audio` / `write_audio_frame`, `crates/mediaway/src/session.rs`);
      `tests/screen_mic_av_smoke.rs` migrated onto the native API — its hand-rolled
      second-track muxing is gone (Stage 1b in `crates/mediaway/docs/roadmap.md`).
- [x] **Windows H.264 pipeline C ABI slice**: `mediaway-ffi` covers encode, MP4 mux/demux,
      and decode (`AutoDecoder`, `adr/pipeline/0004-auto-decode-c-abi.md`) in one enabled
      integration test (`tests/decode_smoke.rs`), with decoded pixel-content assertions.
      The same slice is also covered at the Rust API level by the Windows trim/splice
      round-trip. The broader C ABI facade remains open for wider hardware, capture, and
      consumer-language CI coverage. Shared header types are consolidated into
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
      after 4 real bugs already fixed (readback pitch, NV12 chroma-plane barrier, RBSP bit
      offset, `BitOffsetToSliceData`); root cause narrowed to the opaque DXVA picture-parameter
      blob, unresolved after 8 confirmed real hardware hangs — further live re-runs on the
      primary dev machine are paused pending a new lead
      (`mediaway-decoder/adr/windows/0002-d3d12-native-video-decode.md`).
- [x] **Vulkan HEVC Decode**: root cause found and fixed (a missed
      `pps_loop_filter_across_slices_enabled_flag` slice-header bit) — real decode
      hardware-verified on the RTX 4090, hard pixel assertions
      (`mediaway-decoder/adr/vulkan/0001-vulkan-video-decode.md`'s 2026-08-05 addendum).
- [ ] **Vulkan AV1 Encode/Decode**: encode is structurally hardware-verified but every frame's
      OBU output is invalid — confirmed driver-maturity limitation, not a Mediaway bug; AV1
      decode has not been started.
- [x] **Windows CPU Decode Bug**: `WindowsVideoDecoder`'s `CpuFramesOk` H.264 path — both real
      bugs (AVCC/Annex-B framing mismatch, and a test double-free misattributed as a decoder
      abort) found and fixed 2026-08-05
      (`docs/ai/wiki/platform/windows-decode.md` § CPU decode bug).
- [x] **Opus Audio Codec Integration (encode)**: `WindowsAudioEncoder` dispatches `CodecKind::Opus`
      to `mediaway-sw`'s `SwOpusAudioEncoder` as a real `AudioEncoder` backend
      (`crates/mediaway-encoder/src/windows/mod.rs`).
- [x] **Opus Audio Codec Integration (decode)**: `mediaway-decoder` gained an `AudioDecoder`
      trait mirroring `VideoDecoder` (ADR-0003), implemented for `WmfOpusDecoder` (Windows) and
      `mediaway-sw`'s `SwOpusAudioDecoder` (cross-platform). No audio `auto`-dispatch
      (`WindowsAudioDecoder`-style backend switcher) exists yet — same follow-up gap as video's
      D3D12 decode integration.
- [ ] **Pure Rust SW Codec Extensions**: Add CABAC, P-slice, and B-slice decoding to `mediaway-sw` H.264 decoder (currently Baseline CAVLC I-slice only).
- [x] **Android Encoder (first Android backend, first "Other" platform)**: `mediaway-encoder::android`
      (NDK `AMediaCodec` via the `ndk` crate, H.264 CPU-upload only) implemented per
      `mediaway-encoder/adr/android/0001-ndk-amediacodec-h264-cpu-upload.md` — **zero compile
      verification as authored** (this dev environment has no Android NDK, a strictly weaker
      starting point than Linux got via WSL2); a new `android` CI job
      (`nttld/setup-ndk` + `cargo-ndk`, `arm64-v8a`, API 21, compile+clippy only) is the first
      real gate before hardware verification is even attempted.
- [x] **Android Device capture (camera + mic + screen, one vertical slice)**:
      `mediaway-device::android` — Camera2 NDK raw `ndk-sys` FFI camera (a real gap found:
      `ndk-sys` has no `camera2ndk` link directive, closed via a new crate `build.rs`), AAudio
      microphone (blocking `read()`, not the mutex-hostile `data_callback` model), and
      `MediaProjection` + JNI screen capture — the last domain needs a real host-app (Kotlin/
      Java) consent-flow contract documented in
      `mediaway-device/adr/android/0003-mediaprojection-jni-screen-capture.md`, since
      `android-activity`'s stock `AndroidApp` has no `onActivityResult` hook at all (confirmed
      via its real source). minSdk **26** for this crate (AAudio + the native `Surface` bridge
      both need it) — differs from `mediaway-encoder::android`'s 21, a separately scoped
      decision. **Zero compile verification as authored**; `android` CI job extended with a
      `mediaway-device` (`-p 26`) lint step in the same PR. All three ADRs
      (`mediaway-device/adr/android/0001-0003`) Accepted.
- [x] **Apple Encoder (last "Other" platform)**: `mediaway-encoder::apple` (`VTCompressionSession`
      via the `objc2-video-toolbox`/`objc2-core-video`/`objc2-core-media`/`objc2-core-foundation`
      crates, H.264 CPU-upload only) implemented per
      `mediaway-encoder/adr/apple/0001-videotoolbox-h264-cpu-upload.md` (**Accepted**), grounded
      in a locally cloned `objc2` checkout (`local/vendor-ref/objc2/`) rather than web-fetched
      API summaries — caught a real would-be bug (`CreateWithBytes` vs. `CreateWithPlanarBytes`
      for NV12) before any code was written. **Zero compile verification as authored** — this
      dev environment cannot even cross-compile Apple code (no legal path outside macOS/Xcode),
      a harder gap than Android's NDK-only one; `apple-macos`/`apple-ios` CI jobs
      (`.github/workflows/ci.yml`) are the first real gate. Per-packet `is_keyframe` is a
      documented approximation (real `CFArray`/`CFDictionary` sync-attachment reading deferred).
- [x] **`VideoEncoderConfig::color_range` (`ColorRange::Video`/`Full`)**: new shared field
      (`mediaway-common`), threaded through every backend's `VideoEncoderConfig`/
      `AutoVideoEncodeConfig` construction site (~25 call sites across the workspace). Only the
      Apple `VideoToolbox` backend honors it today (`kCVPixelFormatType_420YpCbCr8BiPlanar{Video,
      Full}Range`); Windows/Linux/Android accept the field but don't yet branch on it, same
      capability-gated-fallback convention as `gop_size`. Also fixed a real bug found in the
      same pass: the Android backend's `i-frame-interval` was hardcoded to `0` instead of being
      computed from `VideoEncoderConfig::gop_size`.

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
