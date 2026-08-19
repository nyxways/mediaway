# Apple encode (`VideoToolbox` `VTCompressionSession`) — implemented, zero compile verification

- Module: `mediaway-encoder::apple` (`src/apple/`), `cfg(any(target_os = "macos", target_os =
  "ios"))` — **single module for both**, per `docs/spec/crate-packaging.md`'s platform suffix
  table.
- Bindings: [`objc2-video-toolbox`](https://github.com/madsmtm/objc2) + `objc2-core-video` +
  `objc2-core-media` + `objc2-core-foundation`, `"0.3"` (`Zlib OR Apache-2.0 OR MIT`) — plain
  C-API wrappers (`VTCompressionSession`/`CVPixelBuffer`/`CMSampleBuffer` are Core Foundation
  types, **not** Objective-C classes), so `objc2`/`objc2-foundation`/`block2` are **not** needed
  — smaller graph than a naive "add the whole objc2 ecosystem" plan.
- Codec: H.264 (`kCMVideoCodecType_H264`, Constrained-Baseline-class profile) and HEVC
  (`kCMVideoCodecType_HEVC`, `kVTProfileLevel_HEVC_Main_AutoLevel`). **VP9/AV1 encode is a
  permanent platform gap, not deferred** — zero `kVTProfileLevel_{VP9,AV1}` compression
  constants exist anywhere in the generated `objc2-video-toolbox` bindings (confirmed by
  grepping the whole tree); `VideoToolbox` exposes no public compression API for either codec.
  See `adr/apple/0002-videotoolbox-hevc-encode.md`.
- HEVC extradata: `CMVideoFormatDescriptionGetHEVCParameterSetAtIndex` extracts VPS+SPS+PPS,
  built into an `hvcC` via the new `iso_bmff::bitstream::hevc::to_hvcc` (mirrors H.264's
  `to_avcc` reuse below, generalized from 2 parameter-set types to 3). Extraction dispatch lives
  in `src/apple/videotoolbox/extradata.rs` (`extract_h264`/`extract_hevc`), split out of
  `video.rs` to keep it under this workspace's 1000-line source-file limit.
- CPU upload: `upload_cpu_nv12` via `CVPixelBufferCreateWithPlanarBytes` (2-plane NV12 — **not**
  `CreateWithBytes`, which only fits packed single-plane formats). One memcpy into an owned
  `Box<Vec<u8>>`, freed via `CVPixelBufferReleasePlanarBytesCallback` once VideoToolbox is done
  with it. The new `VideoEncoderConfig::color_range` field (`ColorRange::Video`/`Full`, see
  [encode/scaffold](../encode/scaffold.md)) selects
  `kCVPixelFormatType_420YpCbCr8BiPlanar{Video,Full}Range`.
- GOP: `kVTCompressionPropertyKey_MaxKeyFrameInterval` set from `config.gop_size` (device-
  dependent, not byte-exact like Linux's raw bitstream). Per-packet `is_keyframe` is **real**,
  not approximated — `kCMSampleAttachmentKey_NotSync` attachment reading (`is_sync_sample`,
  `CFArray<CFDictionary<CFString, CFType>>` FFI) replaced the original `packet_index == 0`
  heuristic; see ADR-0001 addendum.
- Output: `VTCompressionOutputCallback` (async, VideoToolbox-internal thread) pushes into a
  shared `SharedState { pending: Mutex<VecDeque<Packet>>, finalized_info: OnceLock<StreamInfo>,
  .. }` behind `Arc` — push-based, unlike Android's pull-based opportunistic drain. The extra
  `Arc::into_raw` strong count passed as the callback's `refCon` is reclaimed exactly once in
  `Drop`, **after** `complete_frames`+`invalidate()` (defensive ordering — see ADR-0001 §
  Decisions confirmed with the user, the `VTCompressionSessionInvalidate` callback-cutoff
  guarantee is unconfirmed from any source reachable this session).
- Extradata: **in scope**, unlike Android's `csd-0`/`csd-1` deferral — SPS/PPS(+VPS) reachable
  synchronously off the first `CMSampleBuffer` (see the HEVC extradata bullet above for H.264's
  precedent extended to 3 parameter sets).
- Zero-Copy: **not implemented, deferred** — `GpuBufferHandle::Metal` (`CVPixelBuffer`/
  `IOSurface` token) **already exists** in `mediaway-common::gpu` (predates any Apple backend,
  same situation Android found for `AndroidSurface`).
- ADR: [0001 (`adr/apple/`)](../../../../crates/mediaway-encoder/adr/apple/0001-videotoolbox-h264-cpu-upload.md)
  — **Accepted**. Binding choice, scope, CI plan, and the **zero compile verification as
  authored** caveat. [0002](../../../../crates/mediaway-encoder/adr/apple/0002-videotoolbox-hevc-encode.md)
  — **Accepted**. HEVC addition, VP9/AV1 permanent non-support.

## Status: implemented, zero compile verification until CI runs

Grounded entirely in a **locally cloned `objc2` checkout**
(`local/vendor-ref/objc2/generated/`, `framework-crates/objc2-*/Cargo.toml`) per explicit user
direction, not web-fetched API summaries — the same "read the real source" discipline that caught
a real bug in the Android backend's initial research pass (`CreateWithBytes` vs.
`CreateWithPlanarBytes` for NV12). This dev environment cannot cross-compile Apple targets at all
(no legal path outside macOS/Xcode) — a harder starting gap than Android's "just missing the
NDK". Two new CI jobs (`.github/workflows/ci.yml`: `apple-macos` on a pinned `macos-14` runner,
native; `apple-ios` cross-compiled, compile-only) are the first real gate this code goes through.

## Structural differences vs. Android (`AMediaCodec`)

| Android (`AMediaCodec` / `ndk`) | Apple (`VTCompressionSession` / `objc2-*`) | Note |
|---|---|---|
| Safe wrapper crate (`ndk::media::media_codec`) | Every `objc2-*` call is `unsafe fn` (raw C API) | This module carries real `// SAFETY:` discipline like `src/windows/`, unlike Android/Linux's `forbid(unsafe_code)` |
| Pull-based `dequeue_output_buffer` opportunistic drain | Push-based async callback into a shared queue | Genuinely different concurrency shape, not just naming |
| `csd-0`/`csd-1` extradata **deferred** (separate output event) | SPS/PPS extradata **in scope** (cheap, same-sample-buffer) | Apple's API shape is cheaper here |
| Real `BUFFER_FLAG_KEY_FRAME` per-packet flag read | `is_keyframe` **real**, via `kCMSampleAttachmentKey_NotSync` attachment reading (`CFArray`/`CFDictionary` FFI) | Both real, different mechanism — see ADR-0001 addendum |
| `GpuBufferHandle::AndroidSurface` deferred | `GpuBufferHandle::Metal` deferred | Same "type exists, wiring deferred" shape |
