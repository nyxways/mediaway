# Workspace crate map

**Packaging v1:** [`crate-packaging`](../../../spec/crate-packaging.md) · ADR-0003 ·
ADR-0012 · **ADR-0021** (consolidation: platform backends are `#[cfg]`-gated modules,
one C ABI, umbrella `mediaway`).

## Freestanding unprefixed cores (independent versioning — ADR-0021)

Reusable sans-io libraries with zero `mediaway-*` deps; own `version = "0.1.0"`.

| Member | Notes |
|--------|-------|
| `iso-bmff` | ISOBMFF/MP4 sans-io mux+demux; `iso-bmff-wasm` = wasm-bindgen wrapper for the browser package |
| `iso-cenc` | ClearKey CENC (ADR-0011) |
| `ebml-webm` · `ogg` · `flv` · `adts` · `riff-wave` · `mpeg-ts` · `mpeg-audio` | format cores consumed by `mediaway-container` |
| `rtmp` | RTMP publish client (sans-io handshake + chunk stream + AMF0); Proposed (publish = false) |

## Mediaway family (one workspace version, released together)

| Crate | Contents |
|-------|----------|
| `mediaway-common` | shared types (`Bytes`, `PixelFormat`, `VideoFrame`, GPU handles, `Rational`) |
| `mediaway-container` | mux/demux facade over the cores (`mp4` …) |
| `mediaway-encoder` | encoder traits + backends as modules: `nvenc` · `windows` · `linux` (VA-API) · `vulkan` · `quicksync` (oneVPL, `vpl-sys` build dep) · `web` (WebCodecs) |
| `mediaway-decoder` | decoder traits + backends as modules: `windows` · `linux` · `vulkan` · `web` |
| `mediaway-device` | capture/playback facades (`camera` · `desktop` · `audio`) + backends as modules: `windows` · `windows_camera` · `windows_desktop` · `windows_audio` · `linux` · `web` |
| `mediaway-sw` | pure-Rust software codecs: rav1e + `opus` (unsafe-libopus) + `apm` (sonora AEC3/NS/AGC2/VAD) |
| `mediaway` | umbrella: `EncodeSession` + auto-dispatch + `wgpu` GPU bridge (DX12 HAL) + re-exports of the five capability crates — one dependency for consumers |
| `mediaway-ffi` | single C ABI: one cdylib + `include/mediaway/{container,device,pipeline}.h`; modules `common`/`container`/`device`/`pipeline` |
| `mediaway-test-media` | generated test fixtures (BLAKE3-validated, `local/.cache/`) |
| `mediaway-avcli` / `mediaway-avprobe` | CLIs (`tools/`) |

## Rule changes (ADR-0021)

- Platform backends are `#[cfg(target_os / target_family)]` modules — **not** Cargo
  features and **not** separate crates (ADR-0003 amended; unprefixed cores stay).
- One C ABI: `mediaway-ffi` (ADR-0004 amended; ADR-0015 subsumed).
- Freestanding cores pin their own version; the family shares `[workspace.package] version`.
