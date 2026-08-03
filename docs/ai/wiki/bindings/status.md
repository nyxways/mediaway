# Language bindings — status & FFI learnings

## Status (2026-08)

| Language | Mechanism | Status |
|---|---|---|
| C | the C ABI itself | ✅ verified — 7 examples link+run; real camera (1920×1080) + mic capture → two-track MP4 (H.264 + AAC) |
| C++ | `bindings/cpp/include/mediaway/mediaway.hpp` RAII wrapper | ✅ verified — 7 examples compile+run; two-track camera_record on real hardware |
| Python | `bindings/python/mediaway/` ctypes package | ✅ verified — 7 examples run; encode output byte-identical to C/C++/Node (6253 B video; 27372 B audio) |
| Node.js | `bindings/nodejs/packages/@mediaway/*` koffi FFI | ✅ verified — 7 examples run; napi-rs is the eventual official path |
| C# | `bindings/csharp/src/` P/Invoke | ✅ verified (xUnit against native libs; ADR-0017/0018) |
| Browser | WASM (`iso-bmff-wasm` + WebCodecs) | ✅ verified — `@mediaway/browser` (ADR-0020): wasm mux/demux + WebCodecs H.264/AAC encode to fMP4, E2E-verified in Chromium + real Edge (`tools/e2e-web`, `browser-package.spec.ts`) |

## DX-driven example flow

Per-language `README.md` is a **brief**: capabilities, the real ABI beneath, the ideal
API (DX contract), and the scenario truth table. A context-less subagent wrote the
ideal API examples from the brief alone (validating self-sufficiency); the real
bindings were then implemented to satisfy those examples. Examples mirror the Rust
`examples/` sector layout: `container/`, `pipeline/`, `device/`.

## Capability truth (as of 2026-08)

- mux/demux fMP4, auto video encode → fMP4, camera + mic capture: **real** through the C ABI.
- **Audio encode: real** (ABI v2, `mediaway-pipeline-ffi/adr/0003-auto-audio-encode-c-abi.md`):
  `mediaway_audio_encoder_open` is single-step (the session IS the encoder — no intermediate
  handle, no consumption trap); `push_pcm`/`poll_packet` stream AAC; `stream_info` exposes the
  AudioSpecificConfig (materialized after the first pushed frame — the muxer track needs it).
  camera_record now produces ONE two-track MP4 (H.264 + AAC, remuxed) on real hardware.
- **Screen capture not from C**: needs a live `ID3D11Device*` with no CPU fallback; Screen + `NONE` gpu → `INVALID_INPUT`, Window → `UNSUPPORTED` (both verified). Browser host: `getDisplayMedia` is native and real.

## Audio encode learnings (this pass)

- **The WMF AAC MFT rejects hand-built output types with `MF_E_ATTRIBUTENOTFOUND`** — the
  encoder only accepts types from its own catalog (`GetOutputAvailableType`), matched on
  sample rate + channel count, with the bitrate overridden on a copy. Negotiate, don't assemble.
- **F32 input must be `MFAudioFormat_Float`**, not `MFAudioFormat_PCM` + 32 bits/sample.
- **The ASC arrives late**: `MF_MT_USER_DATA` on the output type is populated only after the
  first input sample; the blob is a 14-byte WAVEFORMATEX-ish prefix whose trailing 2 bytes are
  the AudioSpecificConfig (`asc_from_waveformatex` now handles both the 20-byte and 14-byte
  shapes). The bindings' call order is push → stream_info → mux.

## FFI learnings (repo fixes this pass)

- **`include/mediaway/device.h` was stale**: it still declared the pre-split
  `mediaway_video_capture_*` surface while the crate shipped
  `mediaway_camera_capture_*` / `mediaway_desktop_capture_*` /
  `mediaway_audio_capture_*` (ADR-0004 domain-feature-split). Rewritten to the real
  ABI (config structs `MediawayCameraCaptureConfig` etc. are CPU-only; desktop keeps
  `gpu_device`).
- **Header co-inclusion**: the three `*-ffi` headers each define
  `mediaway_rational_t` / `mediaway_pixel_format_t` / gpu handle types. The C++
  wrapper needs all three in one TU, so the shared typedefs got
  `MEDIAWAY_*_T_DEFINED` include guards (this is why `camera_record.c` previously
  hand-declared the pipeline surface).
- Handle-consumption traps verified across wrappers: `mediaway_encode_session_open` /
  `_finish` consume their handle unconditionally (even on failure) — wrappers must
  release, never close, on the failure path (C++ `finish()` and the Node `finish()`
  both had release-after-consume bugs this pass; fixed).

## Open items

- Browser is DONE: `@mediaway/browser` ships (ADR-0020) — wasm mux/demux + WebCodecs
  H.264/AAC encode; E2E specs in `tools/e2e-web/browser-package.spec.ts`. The browser
  audio surface is WebCodecs `AudioEncoder` (native), not a wasm AAC codec.
- Official package-layout ADRs for the C++/Python/Node bindings (mirror ADR-0017/0018)
  before shipping — packaging is set up (`tools/scripts/*-package*.ts`, see
  `bindings/README.md` § Publishing), the ADRs are the remaining formality.
- Multi-platform native assets: all language packages ship Windows x64 GNU DLLs
  today; macOS/Linux native packages need per-platform builds in CI.
- Screen capture from C remains the only hardware gap (audio encode landed; screen
  needs the live GPU-device-handle ADR).
