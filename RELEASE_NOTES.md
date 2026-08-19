# Mediaway release notes

<!-- Dev changes accumulate under ## Unreleased (AGENTS.md § 10). Finalize
     with `/release-notes <version>`; reset this template with
     `/release-notes reset`. See docs/ai/wiki/meta/release-notes.md. -->

## Unreleased

### Added

- `mediaway-encoder::apple`: AAC-LC encode via `AudioToolbox`'s `AudioConverter`
  (`AppleAudioEncoder`, `Aac`/`Opus` dispatch, mirrors `WindowsAudioEncoder`'s shape). Float32
  PCM input only, no conversion needed (unlike the Windows WMF backend, which downconverts to
  S16). `mediaway-decoder::apple`: AAC-LC decode via the same `AudioConverter` API — **the first
  AAC decoder in this workspace at all**, requires a container-supplied `AudioSpecificConfig` at
  `open()`. Neither wired into `mediaway::platform` (matches existing Windows audio-backend
  scope). **Zero compile verification as authored**. See
  `crates/mediaway-encoder/adr/apple/0004-audiotoolbox-aac-encode.md` and
  `crates/mediaway-decoder/adr/apple/0004-audiotoolbox-aac-decode.md`.
- `mediaway-encoder::apple`/`mediaway-decoder::apple`: real Zero-Copy `GpuBufferHandle::Metal`
  support. Encode input (`VideoInputPreference::ZeroCopyGpu`) borrows the caller's `CVPixelBuffer`
  directly for one `encode_frame` call — no retain/release. Decode output
  (`VideoOutputPreference::ZeroCopyGpu`) takes a new, independent `CFRetained::retain` per decoded
  frame; unlike this workspace's VA-API DMA-BUF Zero-Copy, `VTDecompressionSession`'s
  `CVPixelBufferPool` grows on demand rather than reusing a fixed slot, so no DPB-style
  `outstanding` tracking is needed — the decoder holds the last-`poll_frame`-returned handle and
  releases it on the next `push_packet`/`poll_frame`/`flush` call, matching this crate's existing
  "valid until next call" GPU-handle contract. **Zero compile verification as authored**. See
  `crates/mediaway-encoder/adr/apple/0003-videotoolbox-metal-zero-copy-encode.md` and
  `crates/mediaway-decoder/adr/apple/0003-videotoolbox-metal-zero-copy-decode.md`.
- `mediaway::platform`: wired Apple into `encoder_support`/`decoder_support` (Opus — see the
  native `AudioConverter` bullet below) and into `ScreenCapture::open`/`Microphone::open`/
  `device_support`/`request_device_permission` (`mediaway-device::apple`'s Camera/Microphone/
  Screen backends were already implemented but unreachable through this cross-platform facade).
- `mediaway-encoder::apple`/`mediaway-decoder::apple`: native Opus encode/decode via the same
  `AudioToolbox` `AudioConverter` API AAC uses (`audiotoolbox::{OpusEncoder, OpusDecoder}`) —
  replaces the cross-platform `SwOpusAudioEncoder`/`SwOpusAudioDecoder` as `AppleAudioEncoder`'s
  Apple-side default (both remain directly usable elsewhere). No new Cargo dependency —
  `kAudioFormatOpus` is a plain format ID already reachable through the AAC work's existing
  bindings. Frame duration is converter-chosen (queried via
  `kAudioConverterCurrentOutputStreamDescription`/`CurrentInputStreamDescription`), not
  caller-selectable like `SwOpusAudioEncoder`'s `time_base`-as-duration contract — a real,
  disclosed gap. No magic cookie needed either direction (Opus is self-describing per-packet).
  Wired live into `mediaway::platform`'s Apple `encoder_support`/`decoder_support` Opus probes.
  **Zero compile verification as authored**. See
  `crates/mediaway-encoder/adr/apple/0005-audiotoolbox-opus-encode.md` and
  `crates/mediaway-decoder/adr/apple/0005-audiotoolbox-opus-decode.md`.

- `mediaway-encoder::apple`: HEVC encode (`kVTProfileLevel_HEVC_Main_AutoLevel`, CPU NV12
  upload, `hvcC` extradata via a new `iso_bmff::bitstream::hevc` module mirroring the existing
  H.264 `avc.rs` shape). VP9/AV1 encode confirmed to be a **permanent** `VideoToolbox` API gap —
  zero compression-profile constants for either codec exist anywhere in the generated
  `objc2-video-toolbox` bindings — not a deferred stage. **Zero compile verification as authored**
  (no Apple SDK in this dev environment, same posture as this backend's existing H.264 path). See
  `crates/mediaway-encoder/adr/apple/0002-videotoolbox-hevc-encode.md`.
- `mediaway-decoder::apple`: HEVC decode (`CMVideoFormatDescriptionCreateFromHEVCParameterSets`,
  same general-GOP black-box-DPB shape as H.264) and VP9/AV1 decode (generic
  `CMVideoFormatDescriptionCreate` + a `SampleDescriptionExtensionAtoms` extension atom wrapping a
  container-supplied `vpcC`/`av1C` config record — no bitstream parsing, `extra_data` required at
  `open()`). Also newly wired into `mediaway::platform`'s `AutoEncoder`/`AutoDecoder`/
  `decoder_support` (previously unwired for every Apple codec). **Zero compile verification as
  authored**. See `crates/mediaway-decoder/adr/apple/0002-videotoolbox-hevc-vp9-av1-decode.md`.

- `mediaway-decoder::vulkan`: AV1 decode via `VK_KHR_video_decode_av1`, scoped to
  `frame_type == KEY_FRAME`/`show_frame == 1`/single-tile pictures (general-GOP AV1 decode
  remains a follow-up). Real sans-io OBU/sequence-header/`KEY_FRAME`-frame-header parsing
  (segmentation, quantization, loop filter, CDEF, loop restoration, tile info all real, not
  stubs) plus a real Vulkan Video session/decode path — **hardware-verified on the RTX 4090
  reference machine on the first attempt**, decoding a real `mediaway-sw::av1::Av1Encoder`
  (`rav1e`-backed) `KEY_FRAME` with hard content assertions; does not share this workspace's
  confirmed AV1 Vulkan *encode* driver-maturity limitation. See
  `crates/mediaway-decoder/adr/vulkan/0002-av1-decode-keyframe-first.md`.
- `mediaway-decoder::android`: first Android decode backend (NDK `AMediaCodec` via the `ndk`
  crate), H.264 CPU NV12 output only, `COLOR_FormatYUV420SemiPlanar` only (reject-not-guess on
  any other reported output color format), general GOP (not IDR-only — the device manages its
  own DPB). Zero compile verification and zero runtime verification as authored (no Android NDK
  or device/emulator in the dev environment); not wired into `auto`/`capability` yet. See
  `crates/mediaway-decoder/adr/android/0001-ndk-amediacodec-h264-cpu-out.md`.
- `mediaway-encoder::amf`: AMD AMF video encode backend (`shiguredo_amf`), H.264 CPU-upload
  encode only, Linux `x86_64` only (the crate's own platform limit). Compile-verified on real
  Linux `x86_64` via WSL2 (including the `AMF_PLANE_TYPE`/`amf_pts`/`amf_size` types confirmed
  against real crate source) — **zero real AMD GPU/driver hardware verification** (none
  available in this workspace). Not wired into `auto`/`capability` yet. See
  `crates/mediaway-encoder/adr/amf/0002-amf-linux-shiguredo-amf-h264-cpu-upload.md`.
- `mediaway-encoder::amf`: extended the H.264-only backend above to also accept HEVC and AV1 —
  `shiguredo_amf`'s own `CodecConfig` already has first-class `Hevc`/`Av1` variants, so this is
  a codec-dispatch widening, not new plumbing. VP9 stays unsupported (`shiguredo_amf` has no
  VP9 `CodecConfig` variant — the dependency's real ceiling, not a Mediaway restriction). Also
  fixes a latent `stream_info_from` bug that would have mislabeled HEVC/AV1 streams as H.264.
  Same WSL2 compile/clippy/test verification, **zero real AMD GPU/driver hardware
  verification** as the H.264 path above. See
  `crates/mediaway-encoder/adr/amf/0003-amf-linux-hevc-av1-codec-dispatch.md`.
- `mediaway-encoder::android`: first Android backend (NDK `AMediaCodec` via the `ndk` crate),
  H.264 CPU-upload encode only. Zero compile verification as authored (no Android NDK in the
  dev environment) — a new CI job compiles/lints it against a real NDK before it is trusted;
  not wired into `auto`/`capability` yet. See
  `crates/mediaway-encoder/adr/android/0001-ndk-amediacodec-h264-cpu-upload.md`.
- `mediaway-decoder::apple`: first Apple decode backend (`VideoToolbox`
  `VTDecompressionSession` via `objc2-*`), H.264 CPU NV12 (`VideoRange`) readback decode only,
  one module for both macOS and iOS. General GOP (P/B frames) — VideoToolbox owns the DPB and
  P/B-frame reordering internally via `kVTDecodeFrame_EnableTemporalProcessing`; this crate
  builds no reference-picture list itself. Scope this stage: exactly one SPS + one PPS, 4-byte
  AVCC length-prefix size only. Zero compile verification as authored (this dev environment
  cannot cross-compile Apple code at all outside macOS/Xcode) — new Apple CI jobs compile/lint
  it against real Apple SDKs before it is trusted; not wired into `auto`/`capability` yet. See
  `crates/mediaway-decoder/adr/apple/0001-videotoolbox-h264-cpu-out.md`.
- `mediaway-encoder::apple`: last "Other" platform encoder backend (`VideoToolbox`
  `VTCompressionSession` via `objc2-*`), H.264 CPU-upload encode only, one module for both
  macOS and iOS. Zero compile verification as authored (this dev environment cannot
  cross-compile Apple code at all outside macOS/Xcode) — new `apple-macos`/`apple-ios` CI jobs
  compile/lint it against real Apple SDKs before it is trusted; not wired into
  `auto`/`capability` yet. Per-packet `is_keyframe` is a documented approximation. See
  `crates/mediaway-encoder/adr/apple/0001-videotoolbox-h264-cpu-upload.md`.
- `VideoEncoderConfig::color_range` (`ColorRange::Video`/`Full`, `mediaway-common`): configurable
  YUV sample range for encoder input. Only the Apple backend honors it so far; other backends
  accept the field without yet branching on it (documented capability-gated fallback, same
  convention as `gop_size`).
- `mediaway-device::android`: first Android device-capture backend — camera (Camera2 NDK raw
  FFI), microphone (AAudio blocking read), and screen (`MediaProjection` + JNI, with a
  documented host-app consent-flow contract). minSdk 26 (differs from
  `mediaway-encoder::android`'s 21). Zero compile verification as authored (no Android NDK in
  the dev environment) — the `android` CI job now also lints `mediaway-device` against a real
  NDK before it is trusted; not wired into any cross-platform capture-selection API yet. See
  `crates/mediaway-device/adr/android/0001-camera2-ndk-native-camera-capture.md`,
  `0002-aaudio-microphone-capture.md`, `0003-mediaprojection-jni-screen-capture.md`.
- `mediaway-device::apple`: first Apple device-capture backend — camera (`AVCaptureSession` +
  an `objc2` `define_class!` delegate), microphone (`AVAudioEngine` input tap), macOS screen
  (`ScreenCaptureKit`), and iOS screen (`ReplayKit` in-app capture, plus a push-in/pull-out
  `AppleBroadcastExtensionCapture` sink for a host project's own Broadcast Upload Extension
  target — this crate cannot build that `.appex` target itself; see the host-extension contract
  in `crates/mediaway-device/adr/apple/0004-replaykit-ios-inapp-screen-capture.md`). Zero
  compile verification as authored (no macOS/Xcode in the dev environment) — the
  `apple-macos`/`apple-ios` CI jobs now also lint `mediaway-device`; not wired into any
  cross-platform capture-selection API yet. See
  `crates/mediaway-device/adr/apple/0001-avfoundation-camera-capture.md`,
  `0002-avaudioengine-microphone-capture.md`, `0003-screencapturekit-macos-screen-capture.md`,
  `0004-replaykit-ios-inapp-screen-capture.md`.
- `mediaway-encoder::linux` (`linux::vaapi`) HEVC encode: HEVC Main profile
  single-forward-reference P-frame GOP alongside the existing H.264 path, dispatched behind a
  new `VaapiVideoEncoder` enum (no `Box<dyn>`). `hevc_gop.rs`'s `GopState` is a verbatim port of
  `mediaway-encoder::vulkan::hevc_gop::GopState`; `EncSequenceParameterBufferHEVC`/
  `EncPictureParameterBufferHEVC`/`EncSliceParameterBufferHEVC` construction is fresh (VA-API's
  own HEVC encode buffers carry no `StdVideoH265*`-equivalent field set — the driver synthesizes
  VPS/SPS/PPS itself), grounded in FFmpeg's real `vaapi_encode_h265.c` conventions. SAO and
  temporal-MVP are deliberately disabled in the emitted SPS to keep this encoder's output the
  simplest possible shape for the sibling VA-API HEVC decoder to round-trip. Compile- and
  test-verified on real Linux (WSL2 Ubuntu, real `libva-dev` headers/bindgen output) — **zero
  real VA-API hardware verification**. See
  `crates/mediaway-encoder/adr/linux/0003-vaapi-hevc-p-frame-gop.md`.
- `mediaway-decoder::linux` (`linux::vaapi`) HEVC decode: HEVC Main profile IDR I-slices and
  single-forward-reference P-slices alongside the existing H.264 path, dispatched behind a new
  `VaapiVideoDecoder` enum (no `Box<dyn>`). No hardware-verified porting source existed for this
  path (Vulkan's own HEVC decode is IDR-only), so the single-slot `HevcDpb` (`hevc_dpb.rs`) and
  the slice-header parser (`hevc_slice.rs`, extended well past `vulkan::hevc_slice.rs`'s own
  stopping point — SAO, temporal-MVP, merge-cand count, QP deltas) are fresh designs grounded in
  ITU-T H.265 and FFmpeg's real `vaapi_hevc.c`. Any short-term RPS shape other than exactly one
  immediately-preceding reference is rejected as `Unsupported`; CRA/random-access pictures are a
  permanent scope cut. Compile- and test-verified on real Linux (WSL2 Ubuntu, real `libva-dev`
  headers/bindgen output) — **zero real VA-API hardware verification**. See
  `crates/mediaway-decoder/adr/linux/0003-vaapi-hevc-p-slice-dpb.md`.
- `mediaway-decoder::linux`: AV1 `KEY_FRAME`-only VA-API decode (`VAProfileAV1Profile0`,
  `VAEntrypointVLD`), single tile, Main profile, every optional coding tool (segmentation, film
  grain, CDEF, loop restoration, superres, warped motion) rejected as `Unsupported` if signaled.
  A spec-derived OBU/sequence-header/frame-header parser — no AV1 decode existed anywhere in
  this workspace to port from. Dispatched alongside the existing H.264 VA-API decoder via a new
  `VaapiVideoDecoder` enum. Compile + clippy + test-verified on real WSL2 Linux — **zero
  real-hardware verification** (no VA-API device available this session), same standing caveat
  as this backend's H.264 path. See
  `crates/mediaway-decoder/adr/linux/0005-vaapi-av1-key-frame-decode.md`.
- `mediaway-encoder::linux`: VP9 `KEY_FRAME` baseline + single-forward-reference `INTER_FRAME`
  GOP VA-API encode (`VAProfileVP9Profile0`, plain `cros-libva`
  `EncSequenceParameterBufferVP9`/`EncPictureParameterBufferVP9` field bags — no packed-header
  buffer needed, unlike this crate's still-blocked AV1 encode design). New `vp9_gop::GopState`
  2-slot physical ping-pong state machine and this backend's first multi-codec **encoder**
  dispatch enum (`VaapiVideoEncoder`, alongside the existing H.264 encoder). Entrypoint probe is
  a real 3-step ladder (`VAEntrypointEncSlice` → `VAEntrypointEncPicture` →
  `VAEntrypointEncSliceLP`) matching FFmpeg's own generic VA-API encode probe order. **Real
  driver-support caveat**: FFmpeg's own source names only the older i965 driver as a working VP9
  VA-API encode target — meaningfully narrower than VP9 *decode*'s broad support, so this is a
  compile/test-verified-only addition. Compile + clippy + test-verified on real WSL2 Linux —
  **zero real-hardware verification** (no VA-API device available this session). See
  `crates/mediaway-encoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md`.
- `mediaway-decoder::linux`: VP9 `KEY_FRAME` + general single-tile `INTER_FRAME` VA-API decode
  (`VAProfileVP9Profile0`, `VAEntrypointVLD`), including compound prediction with no artificial
  reference-count restriction (VA-API's `reference_frames[8]` array is always fully populated
  regardless of active-reference count — a real structural finding, confirmed against FFmpeg's
  `vaapi_vp9.c`) — a broader real-world-stream-compatible scope than this crate's own AV1
  sibling reached. A spec-derived `uncompressed_header()` parser copied verbatim from the real
  primary VP9 specification text (`pdftotext`-extracted this session) and a new persistent
  8-logical-slot reference shadow table (`vp9::ref_table`, 2 fields/slot — width/height — versus
  AV1's 12-field-per-slot state) backed by a 9-physical-surface pool with a pigeonhole-guaranteed
  free-index allocator. Dispatched alongside the existing H.264/AV1 VA-API decoders via the
  `VaapiVideoDecoder` enum. Compile + clippy + test-verified on real WSL2 Linux (100+ new
  hand-constructed bitstream-fixture unit tests) — **zero real-hardware verification** (no
  VA-API device available this session). See
  `crates/mediaway-decoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md`.
- `mediaway-decoder::windows`: D3D12 native HEVC decode (`d3d12_video_decode` module, still
  unregistered), single-forward-reference P-slice + I/IDR, Main profile, 8-bit 4:2:0, parallel
  to the existing H.264 path (new `hevc*.rs` files only, no edits to H.264's own still-unresolved
  GPU-hang baseline). Sans-io-verified only — 42 new unit tests plus `cargo check`/`clippy`
  clean; **zero real GPU hardware verification, deliberately**, given a confirmed, repeatedly
  reproduced D3D12 decode TDR on this workspace's own H.264 path. See
  `crates/mediaway-decoder/adr/windows/0004-d3d12-hevc-single-forward-ref-p-slice-decode.md`.
- `mediaway-decoder::windows`: D3D12 native AV1 decode (`d3d12_video_decode` module, still
  unregistered), `KEY_FRAME`-only, Main profile, 8-bit 4:2:0, single-tile, parallel to the
  existing H.264/HEVC paths (new `av1*.rs` files only). Sans-io-verified only — 43 new unit
  tests plus `cargo check`/`clippy`/`fmt` clean; **zero real GPU hardware verification,
  deliberately**, same TDR-avoidance reasoning as HEVC, plus an open question of whether this
  crate's own D3D12 AV1 encoder output is decodable at all. See
  `crates/mediaway-decoder/adr/windows/0005-d3d12-av1-key-frame-decode.md`.
- `mediaway-decoder::linux`: VA-API DMA-BUF Zero-Copy decode output
  (`VideoOutputPreference::ZeroCopyGpu`, `vaExportSurfaceHandle` via `cros-libva`'s
  `Surface::export_prime()`), new codec-agnostic `vaapi/dmabuf.rs` and a new
  `mediaway_common::GpuBufferHandle::DmaBuf(Box<DmaBufDescriptor>)` variant (boxed — drops
  `Copy` from the whole enum). DPB slot recycling now tracks outstanding Zero-Copy handles
  (`Dpb::mark_outstanding`/`clear_outstanding`), refusing to recycle a slot a caller still holds
  a handle into. WSL2 + Windows compile/clippy/test-verified; zero real VA-API hardware
  verification (no device available). See
  `crates/mediaway-decoder/adr/linux/0006-vaapi-dmabuf-zero-copy-output.md`.
- `mediaway-encoder::linux`: VA-API DMA-BUF Zero-Copy encode input
  (`VideoInputPreference::ZeroCopyGpu`, `vaCreateSurfaces` import via `cros-libva`'s
  `ExternalBufferDescriptor`), new codec-agnostic `vaapi/dmabuf.rs`, reusing the decoder's
  `GpuBufferHandle::DmaBuf`. A single-use imported surface flows through the existing
  `Picture<S, T>` typestate chain alongside the pooled CPU-upload reference surfaces, with no
  pool restructuring; forces all-IDR encode for `ZeroCopyGpu` sessions. WSL2 + Windows
  compile/clippy/test-verified; zero real VA-API hardware verification. See
  `crates/mediaway-encoder/adr/linux/0006-vaapi-dmabuf-zero-copy-input.md`.
- `mediaway-encoder::web`: generalized the AAC-hardcoded audio smoke/probe functions into a
  codec-parameterized surface — `is_webcodecs_audio_codec_supported(codec, channels,
  sample_rate)` and `encode_audio_buffer(codec, channels, sample_rate, bitrate_bps,
  frame_count) -> EncodedAudioChunks` (new type), mirroring the video side's existing
  codec-parameterized shape. Opus is exercised as the second codec through this generalized
  surface (no Opus-only functions, no `OpusEncoderConfig` knobs — no reachable `web-sys`
  binding — no Opus fMP4 mux claim, `EncodedAudioChunk`-level validation only). `wasm32`
  compile-verified only, no real browser runtime available in this environment. See
  `crates/mediaway-encoder/adr/web/0001-webcodecs-opus-audio-encode.md`.
- `mediaway-decoder::web`: first audio decode surface in this module (previously video-only) —
  `is_webcodecs_audio_decode_supported(codec, channels, sample_rate)` and
  `decode_audio_chunks(...) -> DecodedAudioData` (new type), codec-parameterized from the
  start and exercised via Opus (AAC decode is reachable through the same function). Sample
  readback trusts the browser's reported `AudioData` format (planar or interleaved) rather
  than forcing a conversion. `wasm32` compile-verified only, no real browser runtime available
  in this environment — the exact planar/interleaved readback byte layout is unverified
  against real Chrome. See
  `crates/mediaway-decoder/adr/web/0001-webcodecs-opus-audio-decode.md`.
- `mediaway-encoder::web`: generalized the WebGPU-canvas GPU-surface encode path from
  hardcoded H.264 to HEVC/AV1/VP9 (`is_webgpu_video_codec_supported`,
  `encode_video_frame_from_webgpu_canvas`, `webcodecs_gpu_video_fmp4_smoke_with_codec`), plus a
  new WebCodecs-codec-string → `iso_bmff::Codec` mapper for fMP4 muxing. Existing zero-arg
  H.264 entry points (`is_webgpu_video_frame_supported`, `webcodecs_gpu_video_fmp4_smoke`) are
  kept as thin wrappers. `wasm32` compile-verified only — no real browser runtime available in
  this environment; HEVC's Annex-B-vs-length-prefixed NAL framing for `iso-bmff`'s `hvc1`
  sample entry is explicitly unverified. See
  `crates/mediaway-encoder/adr/web/0001-webgpu-multi-codec-video-encode.md`.

### Changed

- `mediaway-decoder::linux` (`linux::vaapi`) H.264 decode: extended from IDR-only to real GOP
  (IPPP...) decode — single-forward-reference P-slices and non-IDR I-slices now route through a
  shared per-picture pipeline with a sliding-window DPB ported from `mediaway-decoder::vulkan`'s
  hardware-verified DPB/POC arithmetic. No B-slices, reference-list reordering, long-term
  references, weighted prediction, CABAC P-slices, or multi-reference decode this round (all
  rejected honestly, not misparsed). Compile- and test-verified on real Linux (WSL2 Ubuntu, real
  `libva-dev` headers/bindgen output) — **zero real VA-API hardware verification** (no working
  VA-API device available in this workspace). See
  `crates/mediaway-decoder/adr/linux/0002-vaapi-h264-p-slice-dpb.md`.
- `mediaway-encoder::linux` (`linux::vaapi`) H.264 encode: extended from all-IDR to real
  single-forward-reference P-frame GOP (IPPP...) encode — `VideoEncoderConfig::gop_size` finally
  read by this backend, real `frame_num`/reference-picture-list wiring ported from
  `mediaway-encoder::vulkan::h264_gop::GopState`'s hardware-verified decision state machine.
  Capability-gated on `VAConfigAttribEncMaxRefFrames` (queried via `Display::get_config_attributes`
  at session-open time); `gop_size <= 1` or an unsupporting driver both fall back to all-IDR
  encode, byte-identical to the previous output. No B-frames, multi-reference, reference-list
  reordering, long-term references, or rate control this round (all deliberately deferred, not
  silently dropped). Compile- and test-verified on real Linux (WSL2 Ubuntu, real `libva-dev`
  headers/bindgen output) — **zero real VA-API hardware verification** (no working VA-API device
  available in this workspace). See `crates/mediaway-encoder/adr/linux/0002-vaapi-h264-p-frame-gop.md`.
- `mediaway`'s `wgpu` dependency bumped from 26.x to 30.x (workspace MSRV now 1.96 clears
  30.x's rustc floor). Fixed six real breaking-API changes in the DX12 HAL escape-hatch bridges
  (`create_texture_from_hal`'s new `initial_state` parameter, `PollType::Wait`'s new struct
  shape, `Instance::new`/`InstanceDescriptor`/`enumerate_adapters` signature changes) and
  removed the `windows-hal-interop` 0.58 straddle dependency entirely, since `wgpu-hal` 30.x now
  pins the same `windows` 0.62 line this workspace already uses. Real-hardware re-verified
  (RTX 4090): the DX12→D3D11 decode-import bridge tests actually ran (not skipped), including a
  byte-exact NV12 pixel round trip. See `crates/mediaway/adr/wgpu/0004-wgpu-30-upgrade.md`.
- Windows `WindowsScreenCapture`: shared (multi-consumer) sessions now fan out each frame via a
  fixed-depth ring of GPU textures any number of caught-up consumers share through cheap `Arc`
  clones, replacing the previous one-`CopyResource`-per-attached-consumer design — a straggling
  consumer degrades to its own transient copy only, never blocking the driver thread or other
  consumers. Compiled and linted on real hardware; end-to-end frame delivery through the new
  ring is not yet hardware-verified (see
  `crates/mediaway-device/adr/windows/0007-ring-buffer-shared-desktop-duplication.md`).

### Fixed

- Android `mediaway-encoder::android` backend: `AMediaFormat`'s `i-frame-interval` (seconds
  between key frames) was hardcoded to `0` instead of being computed from
  `VideoEncoderConfig::gop_size`.
- Android `mediaway-encoder::android` backend: `StreamInfo::extra_data` (SPS/PPS `avcC`) was
  never populated — always empty, even though `AMediaCodec` delivers `csd-0`/`csd-1` via a
  `BUFFER_FLAG_CODEC_CONFIG` output buffer before the first frame. Now captured and converted
  to `avcC`.
- Apple `mediaway-encoder::apple` backend: `Packet::is_keyframe` was a `gop_size <= 1 ||
  packet_index == 0` approximation, not real per-sample sync-frame detection. Now reads the
  real `kCMSampleAttachmentKey_NotSync` attachment VideoToolbox sets on each encoded sample.
- Linux `mediaway-device::linux` microphone capture: `Select` only ever accepted `Default` —
  a non-default `PipeWire` source could not be targeted at all. `DeviceId` gained a
  `PipeWire(String)` (`node.name`) variant; `Select::Id(DeviceId::from_pipewire_node_name(..))`
  now sets `PW_KEY_TARGET_OBJECT` on the capture stream. Real-hardware (real `libpipewire` link)
  compile and unit-test verified via WSL2.
- Windows `mediaway-encoder-windows`: WMF AV1 encode's `refresh_extradata` was codec-blind and
  ran every codec's `MF_MT_MPEG_SEQUENCE_HEADER` blob through the H.264-Annex-B-specific
  `avc::to_avcc`, silently writing a non-conformant raw blob into the MP4 `av1C` box for AV1
  output. New `iso_bmff::bitstream::av1::to_av1c` builds a real `AV1CodecConfigurationRecord`
  from the Sequence Header OBU; `refresh_extradata` is now codec-aware. Also adds a real
  `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, …)` diagnostic
  (`wmf::video::tests::list_encoder_mfts_for_each_codec`) mirroring the decode-side probe —
  real finding on the verification host: an AV1 encoder MFT (NVIDIA + Intel) is registered
  under `MFT_ENUM_FLAG_HARDWARE`, but not reachable through either the CPU-upload or DX11
  Zero-Copy path yet on that host (see `crates/mediaway-encoder/docs/roadmap.md`). See
  `crates/mediaway-encoder/adr/windows/0010-wmf-av1-encode-config-record-and-mft-probe.md`.

### Removed

### Deprecated

### Breaking
