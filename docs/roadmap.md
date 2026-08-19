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
      decode has not been started on this backend. (VA-API/Linux AV1 `KEY_FRAME`-only decode
      landed separately, compile+test-verified, zero real-hardware verification — see
      `mediaway-decoder/adr/linux/0005-vaapi-av1-key-frame-decode.md`.)
- [x] **VA-API/Linux VP9 Encode/Decode**: implemented, WSL2 compile+clippy+test-verified —
      encode is **not** blocked (unlike AV1: `cros-libva` VP9 encode structs are plain field
      bags, the driver synthesizes headers itself, confirmed via `FFmpeg`'s own
      `vaapi_encode_vp9.c`), but real-world VP9 VA-API *encode* driver support is narrow (i965
      only, per that same `FFmpeg` comment) — a compile/test-verified-only addition. Decode
      scope is `KEY_FRAME` + general `INTER_FRAME` (no artificial reference-count restriction —
      VP9's `reference_frames[8]` array is always fully populated regardless of
      active-reference count), a spec-derived parser cross-checked against the real primary VP9
      spec text (`pdftotext`-extracted this session, correcting an earlier "`su(n)`" assumption
      to the real `s(n)` shape). **Zero real-hardware verification** on either side. See
      `mediaway-encoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md` and
      `mediaway-decoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md`.
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
- [x] **Apple Device capture (camera + mic + screen, one vertical slice)**:
      `mediaway-device::apple` — `AVCaptureSession` + a `define_class!` delegate for camera (this
      workspace's first Objective-C delegate-class pattern, unlike Android's C-callback or the
      encoder's C-function-pointer designs), `AVAudioEngine` input tap for mic (found a real
      planar-vs-interleaved PCM mismatch before any code was written), `ScreenCaptureKit` for
      macOS screen (this crate's first genuinely-async `open()`, bridging two real
      completion-handler calls), and `ReplayKit` for iOS screen — both an in-app
      `AppleScreenCapture` (video + app audio + mic audio) and a push-in/pull-out
      `AppleBroadcastExtensionCapture` sink for a host project's own Broadcast Upload Extension
      `.appex` target (this crate cannot build that target itself; the host-extension contract
      is documented in `mediaway-device/adr/apple/0004`, mirroring how Android's `MediaProjection`
      ADR documented its own host-`Activity` contract). **Zero compile verification as
      authored** (no macOS/Xcode in this dev environment); `apple-macos`/`apple-ios` CI jobs
      extended with a `mediaway-device` lint step in the same PR. All 4 ADRs
      (`mediaway-device/adr/apple/0001-0004`) Accepted.
- [x] **Apple multicodec (HEVC encode/decode, VP9/AV1 decode)**: `mediaway-encoder::apple` gains
      HEVC encode (`kVTProfileLevel_HEVC_Main_AutoLevel`, `hvcC` extradata via a new
      `iso_bmff::bitstream::hevc` module mirroring `avc.rs`); VP9/AV1 encode found to be a
      **permanent** `VideoToolbox` API gap (zero compression-profile constants for either codec
      anywhere in the `objc2-video-toolbox` bindings), not a deferred stage. `mediaway-decoder::apple`
      gains HEVC (`CMVideoFormatDescriptionCreateFromHEVCParameterSets`, same shape as H.264) and
      VP9/AV1 decode (generic `CMVideoFormatDescriptionCreate` + `SampleDescriptionExtensionAtoms`
      extension atom — no bitstream parsing, requires a container-supplied `vpcC`/`av1C` config
      record up front, unlike H.264/HEVC's in-band discovery) — also newly **wired into
      `mediaway::platform`'s `AutoEncoder`/`AutoDecoder`/`decoder_support`** (previously unwired for
      every codec). **Zero compile verification as authored** (same posture as every other Apple
      backend); see `mediaway-encoder/adr/apple/0002-videotoolbox-hevc-encode.md` and
      `mediaway-decoder/adr/apple/0002-videotoolbox-hevc-vp9-av1-decode.md`.
- [x] **Apple Zero-Copy (`GpuBufferHandle::Metal`)**: `mediaway-encoder::apple` gains
      `VideoInputPreference::ZeroCopyGpu` — a plain borrow of the caller's `CVPixelBuffer` for one
      `encode_frame` call, no retain/release at all. `mediaway-decoder::apple` gains
      `VideoOutputPreference::ZeroCopyGpu` — a new, independent `CFRetained::retain` per decoded
      frame; unlike VA-API's DMA-BUF Zero-Copy, `VTDecompressionSession`'s `CVPixelBufferPool`
      grows on demand rather than reusing a fixed slot, so no DPB-style `outstanding` tracking is
      needed — the decoder just holds the last-returned handle's retain and releases it on the
      next `push_packet`/`poll_frame`/`flush` call, matching this crate's existing "valid until
      next call" GPU-handle contract. Also wires Apple Opus (via the existing cross-platform
      `SwOpusAudioEncoder`/`SwOpusAudioDecoder`) and `mediaway-device::apple`'s already-implemented
      Camera/Microphone/Screen backends into `mediaway::platform` (both were previously
      unreachable through that facade, same class of gap the multicodec wiring above found for
      video). **Zero compile verification as authored**; see
      `mediaway-encoder/adr/apple/0003-videotoolbox-metal-zero-copy-encode.md` and
      `mediaway-decoder/adr/apple/0003-videotoolbox-metal-zero-copy-decode.md`.
- [x] **Apple AAC (`AudioToolbox` `AudioConverter`)**: `mediaway-encoder::apple` gains
      `AacEncoder` (Float32 PCM in, no conversion needed — a real quality win over Windows' own
      F32→S16 downconvert) and `AppleAudioEncoder` (`Aac`/`Opus` dispatch, mirrors
      `WindowsAudioEncoder`'s `AudioBackend` shape). `mediaway-decoder::apple` gains `AacDecoder` —
      **the first AAC decoder in this whole workspace**, ahead of Windows (which only ever had an
      encoder). `AudioConverterFillComplexBuffer` is pull-based and fully synchronous (confirmed
      from its own doc comment) — unlike every `VideoToolbox` backend, no cross-thread
      synchronization is needed anywhere in either backend. Both require raw (non-ADTS)
      `AudioSpecificConfig`-bearing streams; decode requires the ASC supplied at `open()` (no
      in-band discovery, mirroring the VP9/AV1 video-decode precedent). Neither wired into
      `mediaway::platform` (matches `WindowsAudioEncoder`/`WmfOpusDecoder`'s own existing scope,
      not a new gap). **Zero compile verification as authored**; see
      `mediaway-encoder/adr/apple/0004-audiotoolbox-aac-encode.md` and
      `mediaway-decoder/adr/apple/0004-audiotoolbox-aac-decode.md`.
- [x] **Apple Opus, native (`AudioToolbox` `AudioConverter`)**: adds `audiotoolbox::{OpusEncoder,
      OpusDecoder}`, reusing `AacEncoder`/`AacDecoder`'s pull-based `AudioConverter` shape almost
      verbatim — `kAudioFormatOpus` needs no new Cargo dependency/feature. `AppleAudioEncoder`'s
      `AudioBackend::Opus` now dispatches here instead of the cross-platform `SwOpusAudioEncoder`
      (kept, still directly constructible, just no longer the Apple default). The one real
      difference from AAC: Opus's frame size is **converter-chosen**, not spec-fixed — discovered
      via `AudioConverterGetProperty(kAudioConverterCurrent{Output,Input}StreamDescription)` after
      `open()`, since no local `objc2` evidence shows a way to request a specific duration (a real,
      disclosed gap versus `SwOpusAudioEncoder`'s caller-selectable frame duration). No magic
      cookie needed either direction — Opus is self-describing per-packet, matching
      `windows::wmf::opus::WmfOpusDecoder`'s existing "no `extra_data`" precedent. **Wired into
      `mediaway::platform`**: Apple's `encoder_support`/`decoder_support` Opus probes now open this
      native backend live instead of the software one. **Zero compile verification as authored**;
      see `mediaway-encoder/adr/apple/0005-audiotoolbox-opus-encode.md` and
      `mediaway-decoder/adr/apple/0005-audiotoolbox-opus-decode.md`.

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
