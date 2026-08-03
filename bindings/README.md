# Language bindings

Mediaway exposes a hand-written C ABI (`*-ffi` crates, [`docs/spec/c-ffi.md`](../docs/spec/c-ffi.md) · ADR-0004) so non-Rust languages can call the stack. The browser host (Tier C) goes through WASM instead — never the C ABI.

Each language folder is self-contained: its own `README.md` is the entry point. It explains the binding's **status**, the **capabilities** it covers, and its **ideal API** — the DX contract its examples (and eventual real source) follow. Example code lives in `bindings/<lang>/examples/`.

## Status legend

| Mark | Meaning |
|------|---------|
| ✅ **verified** | Real binding source, built and run against the native libraries |
| 🔷 **real** | Real binding source, not yet verified end-to-end |
| 📐 **design** | README brief + aspirational example code only — nothing compiles or ships yet |

## Languages

| Language | Folder | Interop | Status | Entry point |
|---|---|---|---|---|
| C | [`c/`](c/) | The C ABI itself (`<mediaway/*.h>`) | ✅ verified | [README](c/README.md) |
| C++ | [`cpp/`](cpp/) | Thin RAII wrapper over the C ABI | ✅ verified | [README](cpp/README.md) |
| C# | [`csharp/`](csharp/) | P/Invoke over the C ABI (net8.0 + netstandard2.0) | ✅ verified | [src](csharp/src/) · [unity UPM](csharp/unity/com.mediaway.unity/README.md) (🔷) |
| Python | [`python/`](python/) | `ctypes` over the C ABI | ✅ verified | [README](python/README.md) |
| Node.js | [`nodejs/`](nodejs/) | FFI over the C ABI (`koffi`; napi-rs is the eventual official path) | ✅ verified | [README](nodejs/README.md) |
| Browser | [`browser/`](browser/) | WASM (`wasm-bindgen`) + Web APIs — **not** the C ABI | 📐 design | [README](browser/README.md) |

Node.js and the browser are **two distinct hosts** for JS/TS with two distinct interop paths — see `docs/spec/c-ffi.md` § Tier C. Do not collapse them.

## Capability truth table (today)

What each scenario can actually do through the real ABI — a binding README that claims more than this is aspirational and says so:

| Scenario | Container | Pipeline | Device | Real via C ABI? |
|---|---|---|---|---|
| `mux_roundtrip` | mux fMP4 + demux | — | — | ✅ yes |
| `encode_to_mp4` | — | auto video encode → fMP4 | — | ✅ yes |
| `encode_audio` | — | auto AAC encode → audio-only fMP4 | — | ✅ yes (ABI v2, `adr/0003` in mediaway-pipeline-ffi) |
| `camera_record` | — | video + audio encode | camera + mic capture | ✅ **two-track MP4** (H.264 + AAC, remuxed; hardware-verified). Video-only fallback without mic/audio backend |
| `screen_record` | — | video encode | screen capture | 🚧 **not from C** — Screen needs a live GPU device handle (`ID3D11Device*`) with no CPU fallback and no C representation yet: Screen + `NONE` gpu → `INVALID_INPUT`, Window → `UNSUPPORTED` (verified) |

## Scenario map

Each language's `examples/` mirrors the Rust [`examples/`](../examples/) layout —
sector subfolders (`container/`, `pipeline/`, `device/`), one file per scenario:

| File (name varies by language convention) | Mirrors | Capability | Real via C ABI? |
|---|---|---|---|
| `container/mux_roundtrip.*` | [`examples/container/mux_demux_mp4.rs`](../examples/container/mux_demux_mp4.rs) | sans-io container mux + demux roundtrip | ✅ |
| `pipeline/encode_to_mp4.*` | [`examples/pipeline/encode_to_mp4.rs`](../examples/pipeline/encode_to_mp4.rs) | auto video encoder → fragmented MP4 | ✅ |
| `pipeline/encode_audio.*` | (synthetic-PCM sibling of the audio encode ABI) | auto AAC encoder → audio-only fragmented MP4 | ✅ |
| `pipeline/screen_record.*` | [`examples/pipeline/screen_record.rs`](../examples/pipeline/screen_record.rs) | screen + mic capture → encode → MP4 | 🚧 gap demo (C-ABI hosts) / native (browser) |
| `device/camera_record.*` | [`examples/device/capture_camera.rs`](../examples/device/capture_camera.rs) | camera + mic capture → H.264 + AAC → ONE two-track MP4 | ✅ (video-only fallback without mic/audio backend) |
| `device/capture_microphone.*` | [`examples/device/capture_microphone.rs`](../examples/device/capture_microphone.rs) | microphone capture, raw PCM | ✅ |
| `device/capture_screen.*` | [`examples/device/capture_screen.rs`](../examples/device/capture_screen.rs) | screen capture only | 🚧 gap demo (C-ABI hosts) / native (browser) |

## Rules

- English comments only, per [`AGENTS.md`](../AGENTS.md) language policy.
- Map existing Rust surfaces; do not invent capabilities the Rust side doesn't have.
- Opaque handles + error codes at the raw C ABI layer; each language wrapper translates that into its own idiomatic error handling (exceptions, `Result`, error unions, ...).
- Not part of the Cargo workspace: not built, linted, or tested by CI.
- Durable changes to the *real* API surface still require an ADR ([`docs/adr/0004-c-ffi.md`](../docs/adr/0004-c-ffi.md)); this folder is exploratory input to that process, not a substitute for it.
