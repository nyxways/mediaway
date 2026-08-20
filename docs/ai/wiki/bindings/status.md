# Language bindings — status & FFI learnings

## Status (2026-08)

Package versions (v0.1.2, 2026-08-04): npm `@mediaway/*` 0.1.2 · NuGet `Mediaway.*`
0.1.2 · PyPI `mediaway` 0.1.2 · crates.io `mediaway-*` family 0.1.2,
freestanding cores 0.1.1 (`ebml-webm` 0.2.1) · CPack `Mediaway-0.1.2-win64`.

| Language | Mechanism | Status |
|---|---|---|
| C | the C ABI itself | ✅ verified — 7 examples link+run; real camera (1920×1080) + mic capture → two-track MP4 (H.264 + AAC); all 8 container formats are in `container.h` (source of the other bindings' wiring) but C has no `all_formats_smoke.c` of its own yet — README fixed 2026-08-08, example still open |
| C++ | `bindings/cpp/include/mediaway/{core,container,pipeline,device}.hpp` RAII wrapper | ✅ verified — 9 examples compile+run (incl. all 8 container formats); two-track camera_record + GPU device factory/Screen capture/capture-encode bridge on real hardware ([cpp-gpu-device](../meta/cpp-gpu-device.md)); Linux container-verified ([linux-support](linux-support.md)) |
| Python | `bindings/python/mediaway/` ctypes package | ✅ verified — 7 examples run; encode output byte-identical to C/C++/Node (6253 B video; 27372 B audio); all 8 container formats wired; Linux container-verified (WSL2, incl. an installed-wheel smoke test) |
| Node.js | `bindings/nodejs/packages/@mediaway/*` koffi FFI | ✅ verified — 7 examples run; napi-rs is the eventual official path; all 8 container formats wired; Linux container-verified (WSL2) |
| C# | `bindings/csharp/src/` P/Invoke | ✅ verified (xUnit against native libs; ADR-0017/0018); 6 examples under `Container/`/`Device/`/`Pipeline/`, mirroring Node's layout; all 8 container formats wired; Linux container-verified (WSL2, real `dotnet test`) |
| Browser | WASM (`iso-bmff-wasm` + WebCodecs) | ✅ verified — `@mediaway/browser` (ADR-0020 + ADR-0022): wasm mux/demux + WebCodecs H.264/AAC encode to fMP4 AND `DecodeSession` decode back (video + audio), E2E-verified in Chromium + real Edge (`tools/e2e-web`, `browser-package.spec.ts`) |

## DX-driven example flow

Per-language `README.md` is a **brief**: capabilities, the real ABI beneath, the ideal
API (DX contract), and the scenario truth table. A context-less subagent wrote the
ideal API examples from the brief alone (validating self-sufficiency); the real
bindings were then implemented to satisfy those examples. Examples mirror the Rust
`examples/` sector layout: `container/`, `pipeline/`, `device/`.

## Capability truth (as of 2026-08)

- mux/demux fMP4, auto video encode → fMP4, camera + mic capture: **real** through the C ABI.
- **Audio encode: real** (ABI v2, `mediaway-ffi/adr/0003-auto-audio-encode-c-abi.md`):
  `mediaway_audio_encoder_open` is single-step (the session IS the encoder — no intermediate
  handle, no consumption trap); `push_pcm`/`poll_packet` stream AAC; `stream_info` exposes the
  AudioSpecificConfig (materialized after the first pushed frame — the muxer track needs it).
  camera_record now produces ONE two-track MP4 (H.264 + AAC, remuxed) on real hardware.
  C# gained its own `Mediaway.Pipeline.AudioEncoder` wrapper (previously Node-only) —
  hardware-verified, matching Node's own output to within container-padding noise.
- **Screen capture not from C**: needs a live `ID3D11Device*`, no CPU fallback; Screen + `NONE` gpu → `INVALID_INPUT`, Window → `UNSUPPORTED`. Browser host: `getDisplayMedia` is native and real.
- **C# Screen capture hardware-verified** — `CaptureTests` gained a test-only raw
  `D3D11CreateDevice` P/Invoke polling real GPU-backed 2560×1440 frames end to end.

## FFI learnings (accumulated)

- WMF AAC MFT rejects hand-built output types — negotiate via `GetOutputAvailableType`;
  the ASC (`MF_MT_USER_DATA`) only populates after the first pushed input sample.
- Handle-consumption traps recur across wrappers: `_open`/`_finish`-style calls consume
  their handle unconditionally (even on failure) — wrappers must release, never close,
  on the failure path.
- Wiring a new container format or codec variant always surfaces the same bug class:
  a mirror enum (`CodecKind` et al.) missing a variant in one of C#/Python/Node/C++, or
  a stale gitignored native lib shadowing a fresh dev build. Check both first.
- `docs/spec/c-ffi.md` (ADR-0004) documents the current single-crate, feature-gated
  `mediaway-ffi` module layout (post ADR-0021 merge).

## Open items

- Browser is DONE, both directions: `@mediaway/browser` ships (ADR-0020 + ADR-0022) —
  wasm mux/demux + WebCodecs H.264/AAC encode, and `DecodeSession` decode back (video +
  audio) via the browser's native `VideoDecoder`/`AudioDecoder`; E2E specs in
  `tools/e2e-web/browser-package.spec.ts`. The browser codec surface is WebCodecs-native
  (`AudioEncoder`/`AudioDecoder`/…), not a wasm codec implementation.
- Official package-layout ADRs for the C++/Python/Node bindings (mirror ADR-0017/0018)
  before shipping — packaging is set up (`tools/scripts/*-package*.ts`, see
  `bindings/README.md` § Publishing), the ADRs are the remaining formality.
- Multi-platform native assets: ADR-0024 (Accepted) implemented for v0.1.7 —
  `release.yml` gained `native-assets-linux`/`native-assets-macos` + matching RC-gate
  jobs; every package (npm/NuGet/PyPI/CPack) now ships win-x64 + linux-x64 + osx-x64 +
  osx-arm64. `@mediaway/ffi` bundles all platforms directly (measured too small —
  ~1.9-3.1 MB each release build — to justify the `optionalDependencies` split the ADR
  first proposed). Linux real-verified via WSL2 (built `.so`, ran the real per-binding
  round-trip tests, installed the built wheel into a clean venv); macOS not yet CI-run
  (branch unpushed) — treat as authored, not verified.
- Screen capture from C (the raw C ABI end-to-end example) remains the only hardware gap;
  C# is covered now (Capability truth) — the C gap still needs the live GPU-device-handle ADR.
- GOP/CBR/`set_bitrate` reach the C ABI + C# now (ABI v6), no-op through auto-select
  until Vulkan joins it — [gop-cbr-set-bitrate](gop-cbr-set-bitrate.md).
- Android AAR/Maven Central distribution: ADR-0025 (Proposed) — design-only,
  no code yet — [android-status](android-status.md).
