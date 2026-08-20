# Mediaway v0.1.8

## What's new

### Added

- **Multi-platform native binding distribution** (ADR-0024): every published binding package —
  npm (`@mediaway/*`), NuGet (`Mediaway.*`), PyPI (`mediaway`), and the C/CPack archive — now
  ships native libs for Linux x86_64 and macOS (x86_64 + arm64) alongside the existing Windows
  x64 build, not Windows-only. `release.yml` gained `native-assets-linux`/`native-assets-macos`
  build jobs and matching `bindings-tests-linux`/`bindings-tests-macos` RC-gate jobs (container
  round-trip on Linux; container + real hardware `VideoToolbox` pipeline round-trip on macOS,
  since `macos-14` runners are real Apple Silicon). NuGet gained `runtimes/linux-x64`,
  `osx-x64`, `osx-arm64` alongside `win-x64`; PyPI ships one wheel per platform
  (`manylinux_2_39_x86_64`, `macosx_11_0_x86_64`, `macosx_11_0_arm64`); CPack produces one
  archive per platform from a single job (the C++ wrapper is a header-only `INTERFACE` target
  over a prebuilt lib, so no cross-platform runner is needed just to package it).
  `@mediaway/ffi` bundles every platform's lib directly (measured ~1.9-3.1 MB per platform
  release build — small enough that a per-platform `optionalDependencies` split wasn't worth
  the complexity). **Linux real-verified this session** via WSL2: built the release `.so` on
  real Linux, ran the actual C#/Python/Node/C round-trip tests against it, and installed the
  built PyPI wheel into a clean venv — not just a compile check. **macOS `native-assets-macos`
  (the actual compile) is now real-CI-verified** — passed on a real `macos-14` runner after the
  `mediaway-decoder::apple` FFI fixes below; the full `bindings-tests-macos` RC gate (container +
  hardware round-trip) is still being confirmed as of this note. Android AAR/Maven distribution
  is a separate, not-yet-implemented design (ADR-0025, Proposed).
- Apple (macOS/iOS): first full encode/decode backend via `VideoToolbox` — H.264, HEVC, VP9/AV1
  decode, ProRes encode/decode, plus AAC-LC and Opus audio encode/decode via `AudioToolbox`
  (the workspace's first AAC decoder); real Zero-Copy Metal (`CVPixelBuffer`) paths for encode
  input and decode output; wired into `mediaway::platform` for encoder/decoder support, capture,
  and permissions. **Authored with zero Apple compile verification** — no macOS/Xcode in this
  dev environment; ProRes RAW is permanently out of scope (no `VideoToolbox` API for it)
- Apple device capture: first backend for camera (`AVCaptureSession`), microphone
  (`AVAudioEngine`), and screen (`ScreenCaptureKit` on macOS, `ReplayKit` + a host Broadcast
  Upload Extension contract on iOS). Same zero-compile-verification caveat as above
- Android: first decode backend (NDK `AMediaCodec`, H.264 CPU output) and first device-capture
  backend (Camera2, AAudio microphone, `MediaProjection` screen). **Authored with zero Android
  compile verification** — no NDK/device/emulator in this dev environment
- AMD AMF: new video encode backend (`shiguredo_amf`, Linux x86_64 only) — H.264, HEVC, AV1.
  Compile/test-verified on WSL2; no real AMD GPU available to hardware-verify
- Vulkan: AV1 decode (`VK_KHR_video_decode_av1`), keyframe-only for now — hardware-verified on
  an RTX 4090 on the first attempt
- `mediaway_encoder::auto::Backend::Vulkan` — Vulkan Video is now a first-class `AutoEncoder`
  backend choice on Windows, tried after NVENC/QuickSync; hardware-verified on an RTX 4090
- Linux (VA-API): HEVC encode/decode GOP, VP9 encode/decode, AV1 keyframe-only decode, and
  DMA-BUF Zero-Copy for both encode input and decode output. Compile/test-verified on WSL2 —
  no real VA-API hardware available to verify this cycle
- Windows: D3D12 native HEVC and AV1 (keyframe-only) decode paths, sans-io-verified only —
  deliberately not run on real hardware given a known, reproduced TDR on this workspace's
  existing D3D12 H.264 decode path
- Windows window capture (WGC) confirmed genuinely Zero-Copy end to end on real hardware — no
  `CopyResource`/`memcpy` anywhere in the path. README's first ⚡ Zero-Copy mark
- Windows screen capture: new opt-in `CaptureSharing::Exclusive` mode — true Zero-Copy, no
  driver thread or ring, single-consumer only (DXGI itself rejects a second concurrent `open()`).
  Hardware-verified on an RTX 4090; default sharing behavior is unchanged
- `mediaway::wgpu::WgpuDx12Bridge`: two new Zero-Copy paths — render directly into the bridge's
  own shared texture instead of copying into it, and import a caller-owned externally-shared
  D3D12 resource instead of allocating one. Hardware-verified on an RTX 4090
- Web (WebCodecs): audio encode/decode generalized from AAC-only to also cover Opus; GPU-surface
  video encode generalized from H.264-only to also cover HEVC/AV1/VP9. `wasm32` compile-verified
  only — no real browser runtime in this dev environment
- `VideoEncoderConfig::color_range` (`ColorRange::Video`/`Full`) — configurable YUV sample range;
  honored by the Apple backend so far, other backends accept it without branching on it yet
- `CodecKind`: six new ProRes variants (422 Proxy/LT/422/HQ, 4444, 4444 XQ), synced through to the
  `mediaway-ffi` C ABI (`mediaway_codec_kind_t`)

### Changed

- Linux (VA-API) H.264 decode: extended from IDR-only to real GOP decode (single-forward-reference
  P-slices, non-IDR I-slices)
- Linux (VA-API) H.264 encode: extended from all-IDR to real single-forward-reference P-frame GOP
  encode, now honoring `VideoEncoderConfig::gop_size`; falls back to all-IDR on unsupporting
  drivers or `gop_size <= 1`
- `mediaway`'s `wgpu` dependency bumped 26.x → 30.x (workspace MSRV now 1.96); fixed six breaking
  DX12 HAL API changes and dropped an interim `windows-hal-interop` straddle dependency.
  Re-verified on real hardware (RTX 4090), including a byte-exact NV12 round trip
- Windows shared (multi-consumer) screen capture: replaced one-`CopyResource`-per-consumer with a
  fixed-depth GPU-texture ring shared via `Arc` clones — a straggling consumer now only costs
  itself a transient copy instead of blocking the driver thread or other consumers

### Fixed

- Android encoder: key-frame interval was hardcoded to `0` instead of being derived from
  `VideoEncoderConfig::gop_size`
- Android encoder: SPS/PPS extradata (`avcC`) was never populated despite being available
- Apple encoder: `Packet::is_keyframe` was an approximation (`gop_size <= 1 || index == 0`); now
  reads VideoToolbox's real per-sample sync-frame attachment
- Linux microphone capture: a non-default PipeWire source could not be selected at all — `Select`
  only ever accepted `Default`
- Windows WMF AV1 encode: extradata refresh ran every codec's sequence-header blob through the
  H.264-specific converter, writing a non-conformant `av1C` box for AV1 output
- `mediaway::platform::AutoEncoder::open`: Linux and Apple branches ignored `config.gpu_device`
  and always used CPU-upload input, even though both backends already have a real Zero-Copy GPU
  path — now tries Zero-Copy first when a GPU device is supplied, matching Windows
- CI: the `android`/`apple-macos`/`apple-ios` jobs only ever compiled/linted `mediaway-encoder`
  and `mediaway-device` for those targets — `mediaway-decoder` (this release's Apple HEVC/VP9/
  AV1/ProRes/AAC/Opus decode and Android `AMediaCodec` decode) had never been compiled on any
  platform-specific CI runner at all. Added it to all three jobs' trigger conditions and lint
  steps
- C#/Python/Node.js `CodecKind` mirrors were missing the six new ProRes variants already present
  in the Rust `CodecKind` and the C ABI header, leaving them out of numeric sync with the native
  ABI — added (C++'s `Codec` enum is a narrower, deliberately curated muxable subset that already
  excludes AV1/VP9/WebVTT/Tx3g pre-existing this release; left as is, out of scope here)
- PyPI wheel `package-data` only ever globbed `_native/*.dll` — the Linux/macOS wheels this
  release adds would have shipped with no native lib inside at all (or, worse, a stale one — see
  next bullet). Fixed to include `*.so`/`*.dylib`; caught by actually installing a built Linux
  wheel into a clean venv and running the real round-trip test against it, not a dry build
- `bindings/python`'s wheel build reused `setuptools`' own `build/` staging directory across
  platform iterations without cleaning it — a wheel built for platform N in a multi-platform loop
  (as the new `pypi` release job does) silently carried every earlier platform's native lib
  forward too. `tools/scripts/build-python-package.ts` now clears `build/` before every build
- `bindings/c/tests/run-roundtrip.sh` (the C-ABI RC-gate check) was Windows-only — hardcoded
  `.dll`, `PATH`-based library resolution, and a `win-x64` native dir default. Generalized to
  detect the host OS and use the right lib filename/extension and runtime search-path variable
  (`LD_LIBRARY_PATH`/`DYLD_LIBRARY_PATH`), real-verified running it on Linux via WSL2
- **Real bugs caught while getting v0.1.7/v0.1.8's release run to actually pass** (v0.1.7's
  crates.io publish is already permanent, and v0.1.8's own crates.io publish already succeeded
  too — these are `release.yml`/binding-metadata-only fixes, no crate source changed, so no new
  version bump was needed to retry):
  - `mediaway-decoder::apple`'s `VideoToolbox` FFI genuinely did not compile on real macOS — 21
    errors across `format_desc.rs`/`video.rs`, all the same root cause: the code assumed
    `objc2-core-media`/`objc2-video-toolbox` expose idiomatic associated constructors
    (`CMFormatDescription::new`, `VTDecompressionSession::new`,
    `CVPixelBuffer::lock_base_address`, …), but the real crates expose C-style free functions
    with `NonNull<*const/mut T>` Create-Rule out-parameters instead (confirmed by reading the
    actual `objc2-core-media`/`objc2-core-video`/`objc2-video-toolbox` 0.3.2 source;
    `mediaway-encoder::apple`'s equivalent code already used the correct shape). Rewrote every
    affected call site to the real API — real-CI-verified: `native-assets-macos` now compiles on
    a real `macos-14` runner.
  - `native-assets-linux`'s pinned `ubuntu-22.04` runner (ADR-0024's original choice) turned out
    to be genuinely too old for two of this workspace's real Linux dependencies — `libspa` (a
    `pipewire` dependency) calls `spa_meta_first`/`spa_meta_region_is_valid`, static-inline
    helpers absent from that release's `libspa-0.2-dev`; separately, `cros-libva`'s AV1 encode
    struct bindings need fields that release's libva 2.14 doesn't have. Both are bindgen'd from
    **system** headers/libraries, not pinned Rust crate versions, so this session's own WSL2
    testing (Ubuntu 24.04) never hit either gap. Fixed by moving the pin to **`ubuntu-24.04`**
    instead (glibc floor now ≥ 2.39, PyPI tag `manylinux_2_39_x86_64`) rather than chasing PPA
    backports package-by-package — see ADR-0024's Implementation note for the full correction.
  - The `native-assets-linux`/`native-assets-macos` artifacts never uploaded the real
    `bindings/python/mediaway/_native/` path directly (only the merge-safe `_native-staging/
    <rid>/` copy, to avoid the two macOS architectures colliding on one filename) — the
    `bindings-tests-linux`/`bindings-tests-macos` RC-gate jobs assumed it was already there and
    failed immediately on a missing-directory error before running any real test. Both jobs now
    copy the right platform's staged lib into place themselves first.
  - `Mediaway.Container.Tests.csproj`/`Mediaway.Pipeline.Tests.csproj` (the C# RC-gate tests) only
    ever knew how to stage `mediaway_ffi.dll`/`libmediaway_ffi.so` for a local `dotnet test` run
    — no macOS entry existed at all, so `bindings-tests-macos` failed with `DllNotFoundException`
    on every test. Added `libmediaway_ffi.dylib` staging for both `osx-arm64`/`osx-x64` (selected
    via `RuntimeInformation.OSArchitecture`, since `native-assets-macos` stages both
    architectures side by side and a blind `Exists()` check would collide).
  - `bindings-tests-linux` only installed `build-essential` (for the C round-trip test) — every
    C# test failed with `DllNotFoundException` there too, but for a different reason than macOS's
    missing-entry bug: `libmediaway_ffi.so` *was* found and `dlopen`'d, but its own transitive
    dependency `libva.so.2` (VA-API runtime, dynamically linked even though these tests never
    touch a real GPU) wasn't installed on that job's bare runner — `native-assets-linux` already
    installs `libpipewire-0.3-dev`/`libva-dev` to *build*, but `bindings-tests-linux` never
    installed anything to satisfy the same libraries at *load* time. Added.
  - With the staging bugs fixed, `bindings-tests-macos`'s container tests passed for real (11/11)
    — but `mediaway-ffi::pipeline::audio::open_audio_encoder` (the C ABI's auto-audio-encoder
    dispatch) had **only ever had a Windows branch**; every other platform, including macOS,
    unconditionally returned `EncodeError::NoBackend` even though `mediaway-encoder::apple::
    AppleAudioEncoder` (AAC-LC via `AudioToolbox`) was implemented earlier this same release
    cycle — the C ABI dispatch layer was simply never updated to call it. Added the missing Apple
    branch. `mediaway::platform::AutoEncoder::open` (the video path) already routes to
    `AppleVideoEncoder` correctly, but `bindings-tests-macos`'s hardware `EncodeToMp4Tests`/
    `DecodeRoundtripTests` still fail with a real `EncoderBackendFailure`/`DecoderBackendFailure`
    (an actual `VideoToolbox` OS/API failure, not a missing-dispatch bug) — **unresolved as of
    this note**, under active investigation.

## Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. The workspace ships 11
freestanding, independently versioned core crates (`iso-bmff`, `ebml-webm`,
`flv-core`, `adts-core`, `ogg-core`, `riff-wave-core`, `mpeg-ts-core`,
`mpeg-audio`, `iso-cenc`, `rtmp`, `rtp-core`) plus one `mediaway` umbrella with
five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and
a single C ABI (`mediaway-ffi`).

## Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770. `AutoEncoder` now also tries Vulkan Video as a backend
  option. D3D12 native HEVC/AV1 decode paths exist but are deliberately not
  hardware-run (known TDR on the existing D3D12 H.264 decode path). Window
  capture (WGC) confirmed genuinely Zero-Copy end to end; screen capture
  gained an opt-in true-Zero-Copy `Exclusive` sharing mode.
- Linux: VA-API gained HEVC/VP9 encode+decode, AV1 keyframe-only decode, and
  DMA-BUF Zero-Copy for both encode input and decode output; a new AMD AMF
  encode backend (H.264/HEVC/AV1, x86_64 only). Compile/test-verified on
  WSL2 — no real VA-API or AMD GPU hardware available to verify this cycle.
  All 4 non-Rust bindings (C++/C#/Python/Node.js) now **ship an actual
  Linux native lib in their published package** (ADR-0024), not just a
  dev-tree compile check — container capability real-verified via WSL2
  (built `.so`, ran the real round-trip tests, installed the built PyPI
  wheel into a clean venv); device/pipeline capabilities remain
  Windows-hardware-verified only.
- macOS / iOS: first implementation this release — `VideoToolbox` encode
  (H.264/HEVC/ProRes) and decode (H.264/HEVC/VP9/AV1/ProRes), native
  AAC-LC/Opus audio via `AudioToolbox`, Zero-Copy Metal GPU paths, and device
  capture (camera/microphone/screen). **Authored with zero compile
  verification, then real-CI-verified to actually compile** — `mediaway-
  encoder`/`mediaway-decoder`/`mediaway-device`'s Apple backends all build
  clean on a real `macos-14` runner (`native-assets-macos`) as of this note,
  after fixing 21 real `mediaway-decoder::apple` compile errors this
  session; this is compile confirmation only, not real-hardware behavior
  verification (no macOS device runs any actual encode/decode/capture in
  CI). Every non-Rust binding's published package also ships a macOS
  (x86_64 + arm64) native lib built from this same code (ADR-0024).
- Android: first implementation this release — NDK `AMediaCodec` H.264
  decode and Camera2/AAudio/`MediaProjection` device capture. **Authored
  with zero compile verification** — no Android NDK/device/emulator in this
  dev environment; not yet wired into CI.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`; WebCodecs encode
  and decode now cover Opus audio and HEVC/AV1/VP9 GPU-surface video
  alongside the existing H.264/AAC paths. `wasm32` compile-verified only, no
  real browser runtime available in this environment.

## Codecs

- Encode: H.264 — NVENC, Vulkan Video, QuickSync (VPL), VA-API (GOP), AMF,
  Apple (unverified); HEVC — VA-API (GOP), AMF, Apple (unverified); VP9 —
  VA-API (narrow real-driver support, untested); AV1 — software (rav1e),
  AMF (untested), Vulkan (implemented but driver-blocked on this workspace's
  reference GPU); ProRes — Apple (unverified, ProRes RAW out of scope).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video (hardware-verified),
  VA-API (GOP, untested), D3D12 (HEVC, sans-io only), Apple (unverified);
  VP9 — VA-API (untested), Apple (unverified); AV1 — Vulkan (keyframe-only,
  hardware-verified), VA-API (keyframe-only, untested), D3D12 (keyframe-only,
  sans-io only), Apple (unverified); ProRes — Apple (unverified). Auto video
  decode C ABI (CPU output), reachable from all 4 non-Rust bindings.
- Audio: Opus — Windows decode via Media Foundation, cross-platform software
  encode/decode (`unsafe-libopus`), native Apple encode/decode (unverified);
  AAC — software encode (C# `Mediaway.Pipeline.AudioEncoder`, ABI v2), native
  Apple encode/decode including this workspace's first AAC decoder
  (unverified); audio processing module (sonora). Opus decode C ABI
  reachable from all 4 non-Rust bindings.
- Containers: ISOBMFF/MP4, WebM, FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE, MPEG
  audio — all verified playable in mpv; CENC encryption/decryption; RTMP
  (proposed, unpublished); `rtp-core` for RTP payloadization (H.264/HEVC).

## Bindings

Every package below now ships native libs for Windows x64, Linux x86_64, and macOS
(x86_64 + arm64) — see **Multi-platform native binding distribution** above (ADR-0024).
Linux is real-verified this release; macOS compiles on real CI, full RC-gate confirmation
still pending as of this note. The new
Apple/Android/AMF/VA-API/D3D12 codec *paths* from the Platforms/Codecs sections above are
not yet reachable through any encode/decode C ABI call (enum-level `CodecKind` sync only
this release).

- C: [`mediaway_ffi.h`](https://github.com/nyxways/mediaway/releases/tag/v0.1.8)
  + one CMake/CPack archive per platform (GitHub Release assets) —
  `mediaway_codec_kind_t` gained the six ProRes values.
- C#: [`Mediaway.*`](https://www.nuget.org/packages/Mediaway.Common) packages
  on NuGet (Trusted Publishing, OIDC) — `CodecKind` synced with the six ProRes values;
  each package's `runtimes/` folder now carries every platform's native lib.
- Python: [`mediaway`](https://pypi.org/project/mediaway/) on PyPI (Trusted
  Publishing) — `Codec` synced with the six ProRes values; one wheel per platform now
  instead of a single Windows-only wheel.
- Node: [`@mediaway/ffi`](https://www.npmjs.com/package/@mediaway/ffi),
  [`@mediaway/container`](https://www.npmjs.com/package/@mediaway/container),
  [`@mediaway/device`](https://www.npmjs.com/package/@mediaway/device),
  [`@mediaway/encoder`](https://www.npmjs.com/package/@mediaway/encoder),
  [`@mediaway/decoder`](https://www.npmjs.com/package/@mediaway/decoder) on
  npm (OIDC Trusted Publishing) — `VideoCodec` synced with the six ProRes values;
  `@mediaway/ffi` now bundles every platform's native lib in the one package.
- C++: `bindings/cpp/include/mediaway/` — `Codec` is a narrower, deliberately curated subset
  (already excludes AV1/VP9/WebVTT/Tx3g pre-existing this release) and was not extended here.
- Browser: [`@mediaway/browser`](https://www.npmjs.com/package/@mediaway/browser)
  (wasm, wasm-bindgen) — WebCodecs Opus audio and HEVC/AV1/VP9 GPU-surface
  video are reachable here, compile-verified only.

## Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

## Maturity bar

Not production-ready. This release adds an unusually large amount of
**authored-but-unverified** surface: the entire Apple (macOS/iOS) and
Android platforms ship with **zero compile verification** in this dev
environment (no Apple SDK, no Android NDK), and several Linux VA-API,
AMD AMF, and Windows D3D12 additions are compile/test-verified only, with
**zero real-hardware verification** this cycle. Treat every codec/platform
path in this note without an explicit "hardware-verified" tag as unverified
until a CI job or a real device confirms it. Previously-shipped
hardware-verified paths (NVENC, QuickSync, Vulkan Video H.264/HEVC/AV1-decode
on RTX 4090, WGC window/screen capture) are unaffected by this caveat.
Costly paths (CPU readback, SW fallbacks) are documented at each API
(`docs/spec/caveats-and-clarity.md`). See `docs/spec/status.md`. One narrower update since the
initial v0.1.7 attempt: `mediaway-decoder::apple`'s `VideoToolbox` FFI shape was corrected
against the real `objc2-core-media`/`objc2-core-video`/`objc2-video-toolbox` 0.3.2 API this
session (21 real compile errors, all fixed) and **is now confirmed compiling on a real
`macos-14` CI runner** — this is compile confirmation only, not real-hardware decode
verification (no macOS device runs any actual decode in CI).

Separately: this release's **binding distribution** itself (which platforms each npm/
NuGet/PyPI/CPack package's native lib actually covers) is a different maturity axis from
the codec/platform backend caveat above. Linux binding packaging is real-verified
(built, packed, installed, and round-trip-tested on real Linux via WSL2 this session — the
`native-assets-linux` job's `ubuntu-24.04` fix matches that WSL2 environment exactly but has not
yet been confirmed by an actual CI run of this exact job as of this note). macOS
`native-assets-macos` (compile) is real-CI-verified; the full `bindings-tests-macos` RC gate
(container + hardware round-trip) is still being confirmed as of this note.
