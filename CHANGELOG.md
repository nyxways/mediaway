# Changelog

All notable changes to Mediaway, grouped by release. Development changes
accumulate under `## Unreleased` in `RELEASE_NOTES.md` and are finalized here
at release time (`/release-notes <version>`). The most recent section is also
the skeleton source for the next release note (Overview / Platforms / Codecs /
Bindings / Maturity bar).

## [0.1.8] - 2026-08-20

### Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. The workspace ships 11
freestanding, independently versioned core crates (`iso-bmff`, `ebml-webm`,
`flv-core`, `adts-core`, `ogg-core`, `riff-wave-core`, `mpeg-ts-core`,
`mpeg-audio`, `iso-cenc`, `rtmp`, `rtp-core`) plus one `mediaway` umbrella with
five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and
a single C ABI (`mediaway-ffi`).

### Platforms

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

### Codecs

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

### Bindings

Every package below now ships native libs for Windows x64, Linux x86_64, and macOS
(x86_64 + arm64) — see **Multi-platform native binding distribution** above (ADR-0024).
Linux is real-verified this release; macOS compiles on real CI, full RC-gate confirmation
still pending as of this note. The new Apple/Android/AMF/VA-API/D3D12 codec *paths* from the
Platforms/Codecs sections above are not yet reachable through any encode/decode C ABI call
(enum-level `CodecKind` sync only this release).

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

### Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

### Maturity bar

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

### What's new since 0.1.7

`v0.1.7`'s first real release attempt only got as far as `crates.io` (already published,
irreversible) before `native-assets-linux`/`native-assets-macos` both failed. Fixing those
surfaced a second and third real Linux build failure and a real `release.yml` artifact-wiring
bug across the following attempts — no crate source changes were needed for any of it beyond the
original `mediaway-decoder::apple` fix, so all of this shipped under the one v0.1.8 version
rather than bumping again for CI-only fixes. Everything else in the "What's new since 0.1.6"
section below is unchanged content, now actually able to ship.

#### Fixed

- `mediaway-decoder::apple`'s `VideoToolbox` FFI genuinely did not compile on real macOS — 21
  errors across `format_desc.rs`/`video.rs`, all the same root cause: the code assumed
  `objc2-core-media`/`objc2-video-toolbox` expose idiomatic associated constructors
  (`CMFormatDescription::new`, `VTDecompressionSession::new`, `CVPixelBuffer::lock_base_address`,
  …), but the real crates expose C-style free functions with `NonNull<*const/mut T>` Create-Rule
  out-parameters instead (confirmed by reading the actual `objc2-core-media`/`objc2-core-video`/
  `objc2-video-toolbox` 0.3.2 source; `mediaway-encoder::apple`'s equivalent code already used the
  correct shape). Rewrote every affected call site to the real API — real-CI-verified:
  `native-assets-macos` now compiles clean on a real `macos-14` runner.
- `native-assets-linux`'s pinned `ubuntu-22.04` runner (ADR-0024's original choice) turned out to
  be genuinely too old for two of this workspace's real Linux dependencies, found one at a time
  across two failed CI attempts — `libspa` (a `pipewire` dependency) calls `spa_meta_first`/
  `spa_meta_region_is_valid`, static-inline helpers absent from that release's `libspa-0.2-dev`;
  separately, `cros-libva`'s AV1 encode struct bindings need fields that release's libva 2.14
  doesn't have. Both are bindgen'd from **system** headers/libraries, not pinned Rust crate
  versions, so this session's own WSL2 testing (Ubuntu 24.04) never hit either gap. Fixed by
  moving the pin to **`ubuntu-24.04`** instead (glibc floor now ≥ 2.39, PyPI tag
  `manylinux_2_39_x86_64`) rather than chasing PPA backports package-by-package — see ADR-0024's
  Implementation note for the full correction.
- The `native-assets-linux`/`native-assets-macos` artifacts never uploaded the real
  `bindings/python/mediaway/_native/` path directly (only the merge-safe `_native-staging/<rid>/`
  copy, to avoid the two macOS architectures colliding on one filename) — the `bindings-tests-
  linux`/`bindings-tests-macos` RC-gate jobs assumed it was already there and failed immediately
  on a missing-directory error before running any real test. Both jobs now copy the right
  platform's staged lib into place themselves first.
- `Mediaway.Container.Tests.csproj`/`Mediaway.Pipeline.Tests.csproj` (the C# RC-gate tests) only
  ever knew how to stage `mediaway_ffi.dll`/`libmediaway_ffi.so` for a local `dotnet test` run —
  no macOS entry existed at all, so `bindings-tests-macos` failed with `DllNotFoundException` on
  every test. Added `libmediaway_ffi.dylib` staging for both `osx-arm64`/`osx-x64` (selected via
  `RuntimeInformation.OSArchitecture`, since `native-assets-macos` stages both architectures side
  by side and a blind `Exists()` check would collide).
- `bindings-tests-linux` only installed `build-essential` (for the C round-trip test) — every C#
  test failed with `DllNotFoundException` there too, but for a different reason than macOS's
  missing-entry bug: `libmediaway_ffi.so` *was* found and `dlopen`'d, but its own transitive
  dependency `libva.so.2` (VA-API runtime, dynamically linked even though these tests never touch
  a real GPU) wasn't installed on that job's bare runner — `native-assets-linux` already installs
  `libpipewire-0.3-dev`/`libva-dev` to *build*, but `bindings-tests-linux` never installed
  anything to satisfy the same libraries at *load* time. Added.
- With the staging bugs fixed, `bindings-tests-macos`'s container tests passed for real (11/11)
  — but `mediaway-ffi::pipeline::audio::open_audio_encoder` (the C ABI's auto-audio-encoder
  dispatch) had **only ever had a Windows branch**; every other platform, including macOS,
  unconditionally returned `EncodeError::NoBackend` even though `mediaway-encoder::apple::
  AppleAudioEncoder` (AAC-LC via `AudioToolbox`) was implemented earlier this same release cycle
  — the C ABI dispatch layer was simply never updated to call it. Added the missing Apple branch.
  `mediaway::platform::AutoEncoder::open` (the video path) already routes to `AppleVideoEncoder`
  correctly, but `bindings-tests-macos`'s hardware `EncodeToMp4Tests`/`DecodeRoundtripTests`
  still fail with a real `EncoderBackendFailure`/`DecoderBackendFailure` (an actual
  `VideoToolbox` OS/API failure, not a missing-dispatch bug) — **unresolved as of this note**,
  under active investigation.

### What's new since 0.1.6

#### Added

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
  (the actual compile) is real-CI-verified** — passed on a real `macos-14` runner after the
  `mediaway-decoder::apple` FFI fixes above; the full `bindings-tests-macos` RC gate (container +
  hardware round-trip) is still being confirmed as of this note. Android AAR/Maven
  distribution is a separate, not-yet-implemented design (ADR-0025, Proposed).
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

#### Changed

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

#### Fixed

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

## [0.1.7] - 2026-08-20

### What's new since 0.1.6

#### Added

- **Multi-platform native binding distribution** (ADR-0024): every published binding package —
  npm (`@mediaway/*`), NuGet (`Mediaway.*`), PyPI (`mediaway`), and the C/CPack archive — now
  ships native libs for Linux x86_64 and macOS (x86_64 + arm64) alongside the existing Windows
  x64 build, not Windows-only. `release.yml` gained `native-assets-linux`/`native-assets-macos`
  build jobs and matching `bindings-tests-linux`/`bindings-tests-macos` RC-gate jobs (container
  round-trip on Linux; container + real hardware `VideoToolbox` pipeline round-trip on macOS,
  since `macos-14` runners are real Apple Silicon). NuGet gained `runtimes/linux-x64`,
  `osx-x64`, `osx-arm64` alongside `win-x64`; PyPI ships one wheel per platform
  (`manylinux_2_35_x86_64`, `macosx_11_0_x86_64`, `macosx_11_0_arm64`); CPack produces one
  archive per platform from a single job (the C++ wrapper is a header-only `INTERFACE` target
  over a prebuilt lib, so no cross-platform runner is needed just to package it).
  `@mediaway/ffi` bundles every platform's lib directly (measured ~1.9-3.1 MB per platform
  release build — small enough that a per-platform `optionalDependencies` split wasn't worth
  the complexity). **Linux real-verified this session** via WSL2: built the release `.so` on
  real Linux, ran the actual C#/Python/Node/C round-trip tests against it, and installed the
  built PyPI wheel into a clean venv — not just a compile check. **macOS is authored but not
  yet CI-verified** — the release branch had not been pushed as of this note; treat it as
  unverified until a real `macos-14` GitHub Actions run confirms it. Android AAR/Maven
  distribution is a separate, not-yet-implemented design (ADR-0025, Proposed).
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

#### Changed

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

#### Fixed

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

### Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. The workspace ships 11
freestanding, independently versioned core crates (`iso-bmff`, `ebml-webm`,
`flv-core`, `adts-core`, `ogg-core`, `riff-wave-core`, `mpeg-ts-core`,
`mpeg-audio`, `iso-cenc`, `rtmp`, `rtp-core`) plus one `mediaway` umbrella with
five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and
a single C ABI (`mediaway-ffi`).

### Platforms

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
  verification** — no macOS/Xcode in this dev environment. Every non-Rust
  binding's published package is now *wired* to ship a macOS (x86_64 +
  arm64) native lib too (ADR-0024, `release.yml`'s new
  `native-assets-macos`/`bindings-tests-macos` jobs), but that path has not
  yet run on a real macOS CI runner — treat macOS as authored, not verified.
- Android: first implementation this release — NDK `AMediaCodec` H.264
  decode and Camera2/AAudio/`MediaProjection` device capture. **Authored
  with zero compile verification** — no Android NDK/device/emulator in this
  dev environment; not yet wired into CI.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`; WebCodecs encode
  and decode now cover Opus audio and HEVC/AV1/VP9 GPU-surface video
  alongside the existing H.264/AAC paths. `wasm32` compile-verified only, no
  real browser runtime available in this environment.

### Codecs

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

### Bindings

Every package below now ships native libs for Windows x64, Linux x86_64, and macOS
(x86_64 + arm64) — see **Multi-platform native binding distribution** above (ADR-0024).
Linux is real-verified this release; macOS is authored but not yet CI-run. The new
Apple/Android/AMF/VA-API/D3D12 codec *paths* from the Platforms/Codecs sections above are
not yet reachable through any encode/decode C ABI call (enum-level `CodecKind` sync only
this release).

- C: [`mediaway_ffi.h`](https://github.com/nyxways/mediaway/releases/tag/v0.1.7)
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

### Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

### Maturity bar

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
(`docs/spec/caveats-and-clarity.md`). See `docs/spec/status.md`.

Separately: this release's **binding distribution** itself (which platforms each npm/
NuGet/PyPI/CPack package's native lib actually covers) is a different maturity axis from
the codec/platform backend caveat above. Linux binding packaging is real-verified
(built, packed, installed, and round-trip-tested on real Linux via WSL2 this session).
macOS binding packaging is wired into `release.yml` but has not yet run on a real
`macos-14` GitHub Actions runner — do not claim macOS binding support as verified until
that first run succeeds.

## [0.1.6] - 2026-08-11

### What's new since 0.1.5

#### Added

- `mediaway-device`: GPU adapter enumeration (`windows::enumerate_gpu_adapters`) and a
  configurable DirectX11 device factory (`windows::GpuDevice`) — the first reusable way
  for a caller without a pre-existing GPU device to get a real `GpuDeviceHandle` for
  Zero-Copy capture/encode/decode paths
- `mediaway-ffi`: C ABI for the GPU device factory (`mediaway_gpu_adapter_list`,
  `mediaway_gpu_device_create`/`_handle`/`_close`) — the first way for a non-Rust
  caller to create a real GPU device and drive Screen capture / GPU-input encode from
  outside Rust; `bindings/c/examples/device/capture_screen.c` and
  `bindings/c/examples/pipeline/screen_record.c` now link+run-verify real Screen
  capture on real hardware instead of only demonstrating the gap
- `@mediaway/decoder` (Node.js): new npm package — video decode + Opus audio decode,
  split out of `@mediaway/encoder`'s previously undiscoverable `decode.ts`
- `@mediaway/device` (Node.js): the GPU device factory (`listGpuAdapters`,
  `GpuDevice`) and real Screen capture — `openScreenCapture()` creates a GPU device
  internally (or accepts a caller-supplied one) instead of always throwing
  `CaptureUnavailableError`
- `@mediaway/encoder` (Node.js): the capture-to-encode bridge
  (`EncodeSession.writeFrameFromCameraCapture`/`writeFrameFromDesktopCapture`) and
  `AutoVideoEncodeConfig.gpuDevice` for Zero-Copy GPU input — `examples/device/capture-screen.ts`
  and `examples/pipeline/screen-record.ts` now run-verify real Screen capture on real
  hardware instead of only demonstrating the gap
- `Mediaway.Device` (C#): the GPU device factory (`GpuDevice.ListAdapters`/`Create`/
  `TryCreate`) — the first way for a C# caller to construct a real `ID3D11Device` without
  raw COM interop
- `Mediaway.Pipeline` (C#): the capture-to-encode bridge
  (`EncodeSession.WriteFrameFromCameraCapture`/`WriteFrameFromDesktopCapture`) —
  `ScreenRecord.cs` now builds its GPU device via the new factory and streams through the
  bridge instead of a `NotImplementedException` placeholder, and
  `Mediaway.Device.Tests`/`Mediaway.Pipeline.Tests` hardware-verify real Screen capture and
  the bridge on real hardware instead of a hand-rolled test-only `D3D11CreateDevice`
- `mediaway` (Python): `GpuDevice.list_adapters()`/`create()` and the capture-to-encode
  bridge (`EncodeSession.write_frame_from_camera_capture`/`write_frame_from_desktop_capture`)
  — `VideoCapture.open(source="screen")` now opens real GPU-backed Screen capture instead
  of always raising `CaptureUnsupportedError`; `examples/device/capture_screen.py` and
  `examples/pipeline/screen_record.py` now run-verify real Screen capture on real hardware
  instead of only demonstrating the gap
- `mediaway::device::GpuDevice` (C++): `listAdapters()`/`create()` — the first way for a
  C++ caller to construct a real `ID3D11Device` without raw COM interop.
  `mediaway::device::ScreenCapture::open()` now opens real Zero-Copy Screen capture
  (`ScreenCaptureConfig::gpuDevice`) instead of always throwing `Status::Unsupported`
- `mediaway::encoder::EncodeSession` (C++): the capture-to-encode bridge
  (`writeFrameFromCameraCapture`/`writeFrameFromDesktopCapture`) —
  `examples/device/capture_screen.cpp` and `examples/pipeline/screen_record.cpp` now
  link+run-verify real Screen capture and the bridge on real hardware instead of only
  demonstrating the gap. This completes GPU device factory + Screen capture +
  capture-to-encode bridge parity across every planned binding (C, Node.js, C#, Python, C++)

#### Fixed

- `mediaway-ffi`: `mediaway_pipeline_ffi_abi_version()` was still returning `5` while
  `include/mediaway/pipeline.h`'s `MEDIAWAY_PIPELINE_FFI_ABI_VERSION` had already been
  bumped to `6` — every C caller's own ABI-version self-check was silently failing
- `bindings/nodejs`: `bun install`'s default per-package/isolated workspace linking
  did not hoist `@mediaway/*` into the root `node_modules`, breaking root-level
  `tsc --noEmit`/`tsx` resolution for `test/*.ts` (pre-existing, reproduced even on
  files untouched by this change) — fixed via `bunfig.toml`'s `[install] linker = "hoisted"`
- `bindings/csharp`: `ScreenRecord.cs`'s `DrainAudioAsync` had an ambiguous bare
  `AudioFrame` reference between `Mediaway.Device.Audio.AudioFrame` and
  `Mediaway.Pipeline.AudioFrame` (pre-existing, only surfaced once `Mediaway.Pipeline`
  referenced `Mediaway.Device.Camera`/`Mediaway.Device.Desktop` for the bridge above) —
  fixed by fully qualifying the type
- `bindings/python`: `examples/pipeline/screen_record.py`'s header claimed "no audio
  encoder exists in the ABI" — stale; `AudioEncoder` (ABI v2) already shipped and
  `camera_record.py`'s own header already documents that gap as closed
- `bindings/cpp`: `device::ScreenCapture::pollFrame()` unconditionally threw on a
  GPU-storage frame — dead code that only surfaced once `ScreenCapture::open()` could
  actually succeed, since GPU storage is the only real case for Screen (no CPU
  fallback). Also never queried negotiated geometry (`info()` stayed `{0,0,...}`
  forever) and had no `releaseFrame()` at all. `examples/pipeline/screen_record.cpp`'s
  header carried the same stale "no audio encoder exists in the ABI" claim as Python's

### Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. The workspace ships 11
freestanding, independently versioned core crates (`iso-bmff`, `ebml-webm`,
`flv-core`, `adts-core`, `ogg-core`, `riff-wave-core`, `mpeg-ts-core`,
`mpeg-audio`, `iso-cenc`, `rtmp`, `rtp-core`) plus one `mediaway` umbrella with
five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and
a single C ABI (`mediaway-ffi`).

### Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770. GPU adapter enumeration and a configurable DirectX11 device
  factory (`mediaway-device::windows::GpuDevice`) now give every binding a way
  to construct a real GPU device without raw COM interop.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified. All 4 C-ABI bindings (C++/C#/Python/Node.js) verified
  for the container capability on Linux x64 (pure CPU); device/pipeline
  capabilities remain Windows-hardware-verified only.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`, WebCodecs encode
  and decode (`DecodeSession`); encoder/decoder/device crates build for
  wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

### Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`, with
  multi-frame GOP + CBR rate control and live `set_bitrate`), QuickSync (VPL);
  AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video, both
  hardware-verified for GPU decode; AAC — software (ADTS). Auto video
  decode C ABI (CPU output), reachable from all 4 non-Rust bindings.
- Audio: Opus — Windows decode via Media Foundation and a cross-platform
  software decoder (both behind the `AudioDecoder` trait), software
  encode/decode (`unsafe-libopus`); audio processing module (sonora); AAC —
  software encode (C# `Mediaway.Pipeline.AudioEncoder`, ABI v2). Opus decode
  C ABI reachable from all 4 non-Rust bindings.
- Containers: ISOBMFF/MP4, WebM, FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE, MPEG
  audio — all verified playable in mpv; CENC encryption/decryption; RTMP
  (proposed, unpublished); `rtp-core` for RTP payloadization (H.264/HEVC).

### Bindings

- C: [`mediaway_ffi.h`](https://github.com/nyxways/mediaway/releases/tag/v0.1.6)
  + CMake/CPack archives (GitHub Release assets) — the GPU device factory
  (`mediaway_gpu_adapter_list`, `mediaway_gpu_device_create`/`_handle`/`_close`)
  now gives a non-Rust caller a real GPU device to drive Screen capture and
  GPU-input encode without any pre-existing device.
- C#: [`Mediaway.*`](https://www.nuget.org/packages/Mediaway.Common) packages
  on NuGet (Trusted Publishing, OIDC) — `Mediaway.Device.GpuDevice` and the
  `Mediaway.Pipeline` capture-to-encode bridge (`WriteFrameFromCameraCapture`/
  `WriteFrameFromDesktopCapture`) now hardware-verify real Screen capture.
- Python: [`mediaway`](https://pypi.org/project/mediaway/) on PyPI (Trusted
  Publishing) — `GpuDevice.list_adapters()`/`create()` and the capture-to-encode
  bridge now hardware-verify real Screen capture via `VideoCapture.open(source="screen")`.
- Node: [`@mediaway/ffi`](https://www.npmjs.com/package/@mediaway/ffi),
  [`@mediaway/container`](https://www.npmjs.com/package/@mediaway/container),
  [`@mediaway/device`](https://www.npmjs.com/package/@mediaway/device),
  [`@mediaway/encoder`](https://www.npmjs.com/package/@mediaway/encoder),
  [`@mediaway/decoder`](https://www.npmjs.com/package/@mediaway/decoder) on
  npm (OIDC Trusted Publishing) — `@mediaway/decoder` is a new package split
  out of `@mediaway/encoder`'s decode surface; `@mediaway/device`'s
  `listGpuAdapters`/`GpuDevice` and `@mediaway/encoder`'s capture-to-encode
  bridge now hardware-verify real Screen capture.
- C++: `mediaway::device::GpuDevice::listAdapters()`/`create()` and
  `EncodeSession`'s capture-to-encode bridge wired into
  `bindings/cpp/include/mediaway/`; completes GPU device factory + Screen
  capture + capture-to-encode bridge parity across every planned binding
  (C, Node.js, C#, Python, C++).
- Browser: [`@mediaway/browser`](https://www.npmjs.com/package/@mediaway/browser)
  (wasm, wasm-bindgen).

### Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

### Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines for
every backend. GPU device factory + Screen capture + capture-to-encode bridge
now reach parity across every planned binding (C, Node.js, C#, Python, C++),
each hardware-verified on real Windows GPUs; device and pipeline
(encode/decode) capabilities remain Windows-hardware-verified only, while
container mux/demux and video/Opus-audio decode also verify on Linux x64
(pure CPU). Costly paths (CPU readback, SW fallbacks) are documented at each
API (`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.

## [0.1.5] - 2026-08-08

### What's new since 0.1.4

#### Added

- `mediaway-ffi`: WebM reaches the container C ABI (`mediaway_muxer_create_for_format`/`mediaway_demuxer_create_for_format`, ABI v1, `adr/container/0003-multi-format-c-abi.md`) — previously the C ABI (and every non-Rust binding) could only open MP4, even though `mediaway-container::webm` (VP8 mux/demux since v0.1.3) had no C-reachable path
- `mediaway-ffi`: Ogg and ADTS reach the container C ABI via dedicated single-stream handles (ABI v2 → v3, `adr/container/0004-ogg-adts-c-abi.md`) — neither format has track registration or `Open`/`Live` typestate, so they don't fit the generic muxer/demuxer handles WebM used
- `mediaway-ffi`: FLV reaches the container C ABI via dedicated handles (ABI v3 → v4, `adr/container/0005-flv-c-abi.md`) — mux writes tag bytes directly into a caller-supplied buffer, mirroring `flv::Muxer`'s own shape
- `mediaway-ffi`: MPEG-TS reaches the container C ABI via dedicated handles (ABI v4 → v5, `adr/container/0006-mpeg-ts-c-abi.md`), including the crate's only multi-packet demux call (`mediaway_ts_demuxer_finish`)
- `mediaway-ffi`: MP3 reaches the container C ABI via dedicated handles (ABI v5 → v6, `adr/container/0007-mp3-c-abi.md`)
- `mediaway-ffi`: WAV reaches the container C ABI (ABI v6 → v7, `adr/container/0008-wav-c-abi.md`), closing out all 8 `mediaway-container` formats
- C++, C#, Python, and Node.js bindings: all 8 `mediaway-container` formats wired end-to-end (WebM/Ogg/ADTS/FLV/MPEG-TS/MP3/WAV joining MP4), each verified against a real native dylib reusing shared byte patterns across the four bindings
- All 4 C-ABI bindings (C++/C#/Python/Node.js) verified on Linux x64, container capability only — `mediaway-ffi` needed zero Rust changes
- C++, C#, Python, and Node.js bindings: video decode (`DecodeSession`) and Opus audio decode (`AudioDecodeSession`) reach all four bindings — the decode session C ABI (`adr/0004-auto-decode-c-abi.md`, `adr/pipeline/0006-audio-decode-c-abi.md`) existed since v0.1.4 with no binding wired to it. Each binding mirrors its existing `AutoVideoEncoder`/`AudioEncoder` single-step-handle shape; the Opus round trip encodes via a raw-ABI path in each language (except Python, whose `AudioEncoder.open()` already accepted `codec=Codec.OPUS`)

#### Fixed

- `mediaway-ffi`: `container.h`'s `mediaway_codec_kind_t` was missing `MEDIAWAY_CODEC_VP8`; `mediaway_container_ffi_abi_version()` had drifted to a stale hardcoded `0`
- `mediaway-common`: `CodecKind` gains explicit `#[repr(u8)]` discriminants, found while wiring the C++ bindings
- C++/Python/Node.js bindings: `Muxer`'s auto-assigned track ids started at `0`, silently rejected by WebM/Matroska (TrackNumber must not be `0`); now start at `1`
- Node.js binding: `RawPacket`'s TypeScript interface was missing a `dts` field the underlying C ABI already had; `@mediaway/container`/`device`/`encoder`'s internal `@mediaway/*` cross-dependency pins were exact-version instead of caret, so npm's workspace linker could silently resolve stale published packages instead of local sources

### Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. The workspace ships 11
freestanding, independently versioned core crates (`iso-bmff`, `ebml-webm`,
`flv-core`, `adts-core`, `ogg-core`, `riff-wave-core`, `mpeg-ts-core`,
`mpeg-audio`, `iso-cenc`, `rtmp`, `rtp-core`) plus one `mediaway` umbrella with
five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and
a single C ABI (`mediaway-ffi`).

### Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified. All 4 C-ABI bindings (C++/C#/Python/Node.js) now verified
  for the container capability on Linux x64 (pure CPU); device/pipeline
  capabilities remain Windows-hardware-verified only.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`, WebCodecs encode
  and decode (`DecodeSession`); encoder/decoder/device crates build for
  wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

### Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`, with
  multi-frame GOP + CBR rate control and live `set_bitrate`), QuickSync (VPL);
  AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video, both
  hardware-verified for GPU decode; AAC — software (ADTS). Auto video
  decode C ABI (CPU output), now reachable from all 4 non-Rust bindings.
- Audio: Opus — Windows decode via Media Foundation and a cross-platform
  software decoder (both behind the `AudioDecoder` trait), software
  encode/decode (`unsafe-libopus`); audio processing module (sonora); AAC —
  software encode (C# `Mediaway.Pipeline.AudioEncoder`, ABI v2). Opus decode
  C ABI now reachable from all 4 non-Rust bindings.
- Containers: ISOBMFF/MP4, WebM, FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE, MPEG
  audio — all verified playable in mpv; CENC encryption/decryption; RTMP
  (proposed, unpublished); `rtp-core` for RTP payloadization (H.264/HEVC).

### Bindings

- C: [`mediaway_ffi.h`](https://github.com/nyxways/mediaway/releases/tag/v0.1.5)
  + CMake/CPack archives (GitHub Release assets) — all 8 `mediaway-container`
  formats (WebM, Ogg, ADTS, FLV, MPEG-TS, MP3, WAV, joining MP4) now reach the
  C ABI (ABI v7); verified building on Linux x64 in addition to Windows.
- C#: [`Mediaway.*`](https://www.nuget.org/packages/Mediaway.Common) packages
  on NuGet (Trusted Publishing, OIDC) — all 8 container formats plus video +
  Opus audio decode (`DecodeSession`/`AudioDecodeSession`) now wired into
  `Mediaway.Pipeline`/`Mediaway.Container`; verified on Linux x64 in addition
  to Windows (container capability).
- Python: [`mediaway`](https://pypi.org/project/mediaway/) on PyPI (Trusted
  Publishing) — all 8 container formats plus video + Opus audio decode now
  wired into the package; verified on Linux x64 in addition to Windows
  (container capability).
- Node: [`@mediaway/ffi`](https://www.npmjs.com/package/@mediaway/ffi),
  [`@mediaway/container`](https://www.npmjs.com/package/@mediaway/container),
  [`@mediaway/device`](https://www.npmjs.com/package/@mediaway/device),
  [`@mediaway/encoder`](https://www.npmjs.com/package/@mediaway/encoder) on
  npm (OIDC Trusted Publishing) — all 8 container formats plus video + Opus
  audio decode now wired into `@mediaway/container`/`@mediaway/encoder`;
  verified on Linux x64 in addition to Windows (container capability).
- C++: all 8 container formats plus video + Opus audio decode
  (`decoder::DecodeSession`/`AudioDecodeSession`) wired into
  `bindings/cpp/include/mediaway/`; verified on Linux x64 in addition to
  Windows (container capability).
- Browser: [`@mediaway/browser`](https://www.npmjs.com/package/@mediaway/browser)
  (wasm, wasm-bindgen).

### Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

### Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines for
every backend. Container mux/demux and video/Opus-audio decode bindings now
reach parity across four ecosystems (C/C++, C#, Python, Node.js) and two
platforms (Windows, Linux x64 for the pure-CPU container capability); device
and pipeline (encode/decode) capabilities remain Windows-hardware-verified
only. Costly paths (CPU readback, SW fallbacks) are documented at each API
(`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.

## [0.1.4] - 2026-08-07

### What's new since 0.1.3

#### Added

- Video decode C ABI (`mediaway_decode_session_open/push_packet/poll_frame/flush/close`) wrapping the auto video decoder — CPU output only this pass
- Capture-to-encode bridge C ABI — a polled Camera/Screen capture frame pushes straight into an encode session with no extra copy (Screen is Zero-Copy end-to-end)
- Opus audio decode C ABI, and `CodecKind::Opus` wired into the existing audio encode C ABI (previously AAC-only)
- Vulkan H.264/HEVC encode: multi-frame GOP (P-frame prediction) plus CBR rate control for H.264, hardware-verified on an RTX 4090
- `VideoEncoder::set_bitrate` — live CBR bitrate retargeting mid-session with no reopen, implemented for Vulkan H.264
- `AudioDecoder` trait in `mediaway-decoder`, implemented by the WMF Opus decoder and a cross-platform software Opus decoder
- New freestanding sans-io crate `rtp-core` — RTP payloadization for H.264/HEVC (RFC 3550/6184/7798), closing the workspace's previous no-RTP gap
- `@mediaway/browser`: `DecodeSession` — demux-then-decode fMP4 playback via the browser's native WebCodecs decoders, the decode-side mirror of `EncodeSession`
- FFI + C#: GOP/CBR encode config and live `set_bitrate` now reach the C ABI (`mediaway_auto_video_encode_config_t`, ABI v5 → v6) and the `Mediaway.Pipeline` C# package
- D3D12 native video-encode backend (internal, not yet wired into the public API) gains GOP support and row-based intra-refresh for H.264/HEVC

#### Changed

- FFI: shared C header value types (`mediaway_rational_t`, pixel/sample formats, GPU handles) consolidated into a new `include/mediaway/common.h`
- FFI: adopted `cbindgen` tooling for header generation; the shipped headers stay hand-written pending a follow-up migration

#### Fixed

- Vulkan HEVC GPU decode no longer produces an all-zero picture — a missing PPS slice-header flag was desyncing the driver's CABAC parser
- Windows CPU H.264 decode silently produced zero frames for Annex-B streams from a WMF encoder — now decodes correctly
- D3D12 native H.264 decode (internal): `BitOffsetToSliceData` corrected per the official DXVA spec
- D3D12 native AV1 encode (internal): fixed a feature-query bug plus DPB-index, buffer-size, and subregion-metadata bugs — output is now structurally valid, though real hardware decode verification is still open
- FFI: fixed a double-free crash during decode session teardown

### Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. The workspace ships 11
freestanding, independently versioned core crates (`iso-bmff`, `ebml-webm`,
`flv-core`, `adts-core`, `ogg-core`, `riff-wave-core`, `mpeg-ts-core`,
`mpeg-audio`, `iso-cenc`, `rtmp`, `rtp-core`) plus one `mediaway` umbrella with
five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and
a single C ABI (`mediaway-ffi`).

### Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770 — Vulkan H.264 **and** HEVC GPU decode both hardware-verified
  this release.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`, WebCodecs encode
  **and** decode (`DecodeSession`); encoder/decoder/device crates build for
  wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

### Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`, now with
  multi-frame GOP + CBR rate control and live `set_bitrate`), QuickSync (VPL);
  AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video, both
  hardware-verified for GPU decode; AAC — software (ADTS). New auto video
  decode C ABI (CPU output).
- Audio: Opus — Windows decode via Media Foundation and a new cross-platform
  software decoder (both behind the new `AudioDecoder` trait), software
  encode/decode (`unsafe-libopus`); audio processing module (sonora); AAC —
  software encode (C# `Mediaway.Pipeline.AudioEncoder`, ABI v2).
- Containers: ISOBMFF/MP4, WebM, FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE, MPEG
  audio — all verified playable in mpv; CENC encryption/decryption; RTMP
  (proposed, unpublished); new `rtp-core` for RTP payloadization (H.264/HEVC).

### Bindings

- C: [`mediaway_ffi.h`](https://github.com/nyxways/mediaway/releases/tag/v0.1.4)
  + CMake/CPack archives (GitHub Release assets) — video decode,
  capture-to-encode bridge, and Opus audio decode/encode all newly reachable
  this release.
- C#: [`Mediaway.*`](https://www.nuget.org/packages/Mediaway.Common) packages
  on NuGet (Trusted Publishing, OIDC) — GOP/CBR encode config and live
  `SetBitrate` newly reachable this release.
- Python: [`mediaway`](https://pypi.org/project/mediaway/) on PyPI (Trusted
  Publishing).
- Node: [`@mediaway/ffi`](https://www.npmjs.com/package/@mediaway/ffi),
  [`@mediaway/container`](https://www.npmjs.com/package/@mediaway/container),
  [`@mediaway/device`](https://www.npmjs.com/package/@mediaway/device),
  [`@mediaway/encoder`](https://www.npmjs.com/package/@mediaway/encoder) on
  npm (OIDC Trusted Publishing).
- Browser: [`@mediaway/browser`](https://www.npmjs.com/package/@mediaway/browser)
  (wasm, wasm-bindgen) — now decode-capable via `DecodeSession`, not just
  encode.

### Breaking changes

`mediaway-ffi`'s pipeline C ABI version bumped 5 → 6 (new
`mediaway_auto_video_encode_config_t` fields, new
`mediaway_encode_session_set_bitrate` export) — recompile any C/C++ caller
against the updated header. Pre-1.0; APIs may change without a major bump.

### Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines for
every backend. GOP/CBR/live-bitrate-retargeting reach the FFI/C# surface this
release but are honestly scoped: the auto-selected backend they resolve to
today (WMF on Windows) does not yet implement them, so those fields are a
documented no-op through that path — only the standalone Vulkan encoders
honor them. Costly paths (CPU readback, SW fallbacks) are documented at each
API (`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.

## [0.1.3] - 2026-08-05

### What's new since 0.1.2

#### Added

- C#: `Mediaway.Pipeline.AudioEncoder` — AAC audio encode (ABI v2), matching the existing Node.js `@mediaway/encoder` capability
- C#: `Device/CaptureMicrophone.cs` and `Pipeline/EncodeAudio.cs` examples; existing examples reorganized under `Container/`/`Device/`/`Pipeline/` to mirror the Node.js binding's example layout
- `ebml-webm`: `Muxer::push_laced_frames` — EBML lacing on the mux side (previously demux-only)
- `CodecKind::Vp8`, wired into `mediaway-container::webm` mux + demux — closes the WebM VP8 gap

#### Changed

- `ebml-webm` demux: indefinite-size `Cluster` sibling-ID lookahead — the open-element stack no longer grows unboundedly on a long-running live `WebM` stream

#### Fixed

- `ebml-webm` mux output is now verified against system `ffprobe` in addition to this crate's own demuxer round-trip

### Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. This release ships the
consolidated workspace (ADR-0021): 10 freestanding, independently versioned
core crates (`iso-bmff`, `ebml-webm`, `flv-core`, `adts-core`, `ogg-core`,
`riff-wave-core`, `mpeg-ts-core`, `mpeg-audio`, `iso-cenc`, `rtmp`) plus one
`mediaway` umbrella with five capability crates (`container`, `encoder`,
`decoder`, `device`, `sw`) and a single C ABI (`mediaway-ffi`).

### Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`; encoder/decoder/
  device crates build for wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

### Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`),
  QuickSync (VPL); AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video (sans-io SPS/PPS/
  slice parsing is unit-tested; GPU decode hardware-verified for H.264);
  AAC — software (ADTS).
- Audio: Opus — Windows decode via Media Foundation (public
  `mediaway_decoder::windows::WmfOpusDecoder`), software encode/decode
  (`unsafe-libopus`); audio processing module (sonora); AAC — software encode
  (C# `Mediaway.Pipeline.AudioEncoder`, ABI v2).
- Containers: ISOBMFF/MP4, WebM (EBML, now including VP8 mux/demux and
  mux-side lacing), FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE, MPEG audio — all
  verified playable in mpv; CENC encryption/decryption; RTMP (proposed,
  unpublished).

### Bindings

- C: `mediaway_ffi.h` + CMake/CPack archives.
- C#: `Mediaway.*` packages on NuGet (Trusted Publishing, OIDC).
- Python: `mediaway` on PyPI (Trusted Publishing).
- Node: `@mediaway/ffi`, `@mediaway/container`, `@mediaway/device`,
  `@mediaway/encoder` on npm (OIDC Trusted Publishing).
- Browser: `@mediaway/browser` (wasm, wasm-bindgen).

### Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

### Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines.
Costly paths (CPU readback, SW fallbacks) are documented at each API
(`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.

## [0.1.2] - 2026-08-04

### What's new since 0.1.1

#### Added

- Windows Opus decode via Media Foundation (public API) and software Opus encode in the facade

#### Changed

- All 8 container formats (audio included) verified playable in mpv via the playback-verification example
- `flv` and `ogg` freestanding cores renamed to `flv-core` / `ogg-core` (crates.io name collisions)
- `ebml-webm` 0.2.0 — CodecPrivate API
- All crates re-published on crates.io (workspace 0.1.2, freestanding cores 0.1.1, `ebml-webm` 0.2.1) with refreshed READMEs and consumer-facing descriptions

#### Fixed

- MP4s that failed to play: malformed `stsz` box and raw SPS written as `avcC`
- Playback timing corrections: ISO-BMFF mux duration/DTS delta (ADR-0004) and Ogg demux/CRC fixes

### Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. This release ships the
consolidated workspace (ADR-0021): 10 freestanding, independently versioned
core crates (`iso-bmff`, `ebml-webm`, `flv-core`, `adts-core`, `ogg-core`,
`riff-wave-core`, `mpeg-ts-core`, `mpeg-audio`, `iso-cenc`, `rtmp`) plus one
`mediaway` umbrella with five capability crates (`container`, `encoder`,
`decoder`, `device`, `sw`) and a single C ABI (`mediaway-ffi`).

### Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`; encoder/decoder/
  device crates build for wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

### Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`),
  QuickSync (VPL); AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video (sans-io SPS/PPS/
  slice parsing is unit-tested; GPU decode hardware-verified for H.264);
  AAC — software (ADTS).
- Audio: Opus — Windows decode via Media Foundation (public
  `mediaway_decoder::windows::WmfOpusDecoder`), software encode/decode
  (`unsafe-libopus`); audio processing module (sonora).
- Containers: ISOBMFF/MP4, WebM (EBML), FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE,
  MPEG audio — all 8 verified playable in mpv; CENC encryption/decryption;
  RTMP (proposed, unpublished).

### Bindings

- C: `mediaway_ffi.h` + CMake/CPack archives.
- C#: `Mediaway.*` packages on NuGet (Trusted Publishing, OIDC).
- Python: `mediaway` on PyPI (Trusted Publishing).
- Node: `@mediaway/ffi`, `@mediaway/container`, `@mediaway/device`,
  `@mediaway/encoder` on npm (OIDC Trusted Publishing).
- Browser: `@mediaway/browser` (wasm, wasm-bindgen).

### Breaking changes

None. APIs are pre-1.0 and may change without a major bump.

### Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines.
Costly paths (CPU readback, SW fallbacks) are documented at each API
(`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.

## [0.1.1] - 2026-08-03

First release (0.1.0 was a manual npm-only publish; this is the first
cross-registry release). Early pre-1.0 snapshot — see the maturity bar below
before relying on any API.

### What's new since 0.1.0

- npm packages now ship READMEs with runnable examples + consumer-facing
  descriptions; NuGet packages carry a shared README
- crates.io: 19-crate dependency-ordered publish (9 freestanding cores +
  mediaway family + avcli/avprobe + vpl-sys; colliding names published as
  adts-core / mpeg-ts-core / riff-wave-core)
- release pipeline: OIDC Trusted Publishing for npm/NuGet/PyPI (no tokens),
  branch ruleset + environment approval gate on release branches

### Overview

Mediaway is a cross-platform media toolkit built on Zero-Copy paths (GPU
handles or shared CPU buffers), sans-io cores for mux/demux/bitstream/config,
and low-level APIs as first-class entry points. This release ships the
consolidated workspace (ADR-0021): 10 freestanding, independently versioned
core crates (`iso-bmff`, `ebml-webm`, `flv`, `adts`, `ogg-core`, `riff-wave`,
`mpeg-ts`, `mpeg-audio`, `iso-cenc`, `rtmp`) plus one `mediaway` umbrella with
five capability crates (`container`, `encoder`, `decoder`, `device`, `sw`) and
a single C ABI (`mediaway-ffi`).

### Platforms

- Windows (win64): primary target. Media Foundation capture/decode, NVENC,
  QuickSync (VPL), and Vulkan Video encode/decode verified on an RTX 4090 and
  Intel UHD 770.
- Linux: camera backends (pipewire/v4l), encoder scaffolding — compiles, not
  hardware-verified.
- Web (wasm32): `@mediaway/browser` ships `iso-bmff-wasm`; encoder/decoder/
  device crates build for wasm32 via wasm-bindgen.
- macOS / iOS / Android: not yet implemented.

### Codecs

- Encode: H.264 — NVENC, Vulkan Video (`VK_KHR_video_encode_queue`),
  QuickSync (VPL); AV1 — software (rav1e).
- Decode: H.264/HEVC — Media Foundation and Vulkan Video (sans-io SPS/PPS/
  slice parsing is unit-tested; GPU decode hardware-verified for H.264);
  AAC — software (ADTS).
- Audio: Opus encode/decode (software), audio processing module (sonora).
- Containers: ISOBMFF/MP4, WebM (EBML), FLV, MPEG-TS, ADTS, Ogg, RIFF/WAVE,
  MPEG audio; CENC encryption/decryption; RTMP (proposed, unpublished).

### Bindings

- C: `mediaway_ffi.h` + CMake/CPack archives.
- C#: `Mediaway.*` packages on NuGet (Trusted Publishing, OIDC).
- Python: `mediaway` on PyPI (Trusted Publishing).
- Node: `@mediaway/ffi`, `@mediaway/container`, `@mediaway/device`,
  `@mediaway/encoder` on npm (OIDC Trusted Publishing).
- Browser: `@mediaway/browser` (wasm, wasm-bindgen).

### Breaking changes

None — first release. Note for early adopters of the pre-consolidation layout:
`mediaway-pipeline` was renamed `mediaway` (ADR-0021) and platform backend
crates became `#[cfg]`-gated modules inside their capability crate. APIs are
pre-1.0 and may change without a major bump.

### Maturity bar

Not production-ready. Backends are stage 0/1: capability probes and minimal
hardware-verified paths, not full rate-controlled multi-frame pipelines.
Costly paths (CPU readback, SW fallbacks) are documented at each API
(`docs/spec/caveats-and-clarity.md`). Sans-io cores carry the test weight;
hardware paths are verified on specific GPUs only. See `docs/spec/status.md`.
