# Apple encode (`VideoToolbox` `VTCompressionSession`) — implemented, zero compile verification

- Module: `mediaway-encoder::apple` (`src/apple/`), `cfg(any(target_os = "macos", target_os =
  "ios"))` — **single module for both**, per `docs/spec/crate-packaging.md`'s platform suffix
  table.
- Bindings: [`objc2-video-toolbox`](https://github.com/madsmtm/objc2) + `objc2-core-video` +
  `objc2-core-media` + `objc2-core-foundation`, `"0.3"` (`Zlib OR Apache-2.0 OR MIT`) — plain
  C-API wrappers (`VTCompressionSession`/`CVPixelBuffer`/`CMSampleBuffer` are Core Foundation
  types, **not** Objective-C classes), so `objc2`/`objc2-foundation`/`block2` are **not** needed
  — smaller graph than a naive "add the whole objc2 ecosystem" plan.
- Codec: H.264 (`kCMVideoCodecType_H264`) only, Constrained-Baseline-class profile
  (`kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel`).
- CPU upload: `upload_cpu_nv12` via `CVPixelBufferCreateWithPlanarBytes` (2-plane NV12 — **not**
  `CreateWithBytes`, which only fits packed single-plane formats). One memcpy into an owned
  `Box<Vec<u8>>`, freed via `CVPixelBufferReleasePlanarBytesCallback` once VideoToolbox is done
  with it. The new `VideoEncoderConfig::color_range` field (`ColorRange::Video`/`Full`, see
  [encode/scaffold](../encode/scaffold.md)) selects
  `kCVPixelFormatType_420YpCbCr8BiPlanar{Video,Full}Range`.
- GOP: `kVTCompressionPropertyKey_MaxKeyFrameInterval` set from `config.gop_size` (device-
  dependent, not byte-exact like Linux's raw bitstream). **Per-packet `is_keyframe` is
  approximated** — `gop_size <= 1 || packet_index == 0` — real `kCMSampleAttachmentKey_NotSync`
  attachment reading is deferred (would need a third layer of unverified generic
  `CFArray<CFDictionary<CFString, CFType>>` FFI); see ADR-0001 § Implementation notes.
- Output: `VTCompressionOutputCallback` (async, VideoToolbox-internal thread) pushes into a
  shared `SharedState { pending: Mutex<VecDeque<Packet>>, finalized_info: OnceLock<StreamInfo>,
  .. }` behind `Arc` — push-based, unlike Android's pull-based opportunistic drain. The extra
  `Arc::into_raw` strong count passed as the callback's `refCon` is reclaimed exactly once in
  `Drop`, **after** `complete_frames`+`invalidate()` (defensive ordering — see ADR-0001 §
  Decisions confirmed with the user, the `VTCompressionSessionInvalidate` callback-cutoff
  guarantee is unconfirmed from any source reachable this session).
- Extradata: **in scope**, unlike Android's `csd-0`/`csd-1` deferral — SPS/PPS reachable
  synchronously off the first `CMSampleBuffer` via
  `CMVideoFormatDescriptionGetH264ParameterSetAtIndex`, converted to avcC by reusing
  `iso_bmff::bitstream::avc::to_avcc` (already used by the Windows WMF backend).
- Zero-Copy: **not implemented, deferred** — `GpuBufferHandle::Metal` (`CVPixelBuffer`/
  `IOSurface` token) **already exists** in `mediaway-common::gpu` (predates any Apple backend,
  same situation Android found for `AndroidSurface`).
- ADR: [0001 (`adr/apple/`)](../../../../crates/mediaway-encoder/adr/apple/0001-videotoolbox-h264-cpu-upload.md)
  — **Accepted**. Binding choice, scope, CI plan, and the **zero compile verification as
  authored** caveat.

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
| Real `BUFFER_FLAG_KEY_FRAME` per-packet flag read | `is_keyframe` **approximated** (`gop_size <= 1 \|\| packet_index == 0`) | `CFArray`/`CFDictionary` attachment reading deferred — see ADR-0001 |
| `GpuBufferHandle::AndroidSurface` deferred | `GpuBufferHandle::Metal` deferred | Same "type exists, wiring deferred" shape |
