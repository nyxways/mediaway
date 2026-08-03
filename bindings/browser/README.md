# Browser binding (Web / WASM)

> **Status: 📐 design** — aspirational example code only; no `@mediaway/browser`
> package exists yet (the `crates/iso-bmff-wasm` smoke exports are test-only, not this
> package). This README is the **DX contract**: the ergonomics a future browser/WASM
> binding should aim for, and what its examples demonstrate.

The browser is a **Tier C host**: it reaches Mediaway through **WASM (`wasm-bindgen`)
+ native Web APIs, never the C ABI** (`docs/spec/c-ffi.md` § Tier C). This is the
mirror image of Node.js (Tier B, C ABI) — do not collapse the two JS/TS hosts.

## What Mediaway is (the capabilities)

A streaming-first media stack. The browser binding covers the same capability ideas as
the native stack, with a different split of labor:

1. **Mux + demux (WASM)**: sans-io fragmented-MP4 muxer (register video/audio tracks,
   `begin()` → live, push packets, flush, `pollBytes()`) and demuxer (`pushBytes`,
   `streams()`, `pollPacket()`). Pure computation — no I/O, so it maps perfectly to
   WASM (`crates/iso-bmff-wasm` already proves the core runs in-browser).
2. **Encode (WASM + WebCodecs)**: Mediaway's WASM module owns the encode→mux wiring
   (`AutoVideoEncoder` / `EncodeSession`), while the actual codec work goes through the
   browser's **native WebCodecs** (`VideoEncoder`) and WebGPU paths where applicable —
   the platform supplies codecs; Mediaway supplies the pipeline + container.
3. **Capture (native Web APIs, not wrapped)**: the browser already exposes
   `getUserMedia()` (camera/mic) and `getDisplayMedia()` (screen). The binding does
   **not** wrap capture; examples use the native APIs and feed frames into Mediaway's
   encode session — the same shape as the native stack's device capability, delivered
   by the host instead.

Note: the browser track's video encode/decode goes through WebCodecs — the codec set is
whatever the user's browser ships (typically H.264/VP8/VP9/AV1). The truth table below
applies to the *native* stack; the browser's own limits are the browser's.

## Ideal API — the DX contract

A single `@mediaway/browser` package, TypeScript-first, `wasm-bindgen`-generated
classes wrapped in idiomatic JS.

- **One `init()` before anything else**: `await init()` fetches + instantiates the WASM
  module; every call after it resolves is synchronous-feeling.
- **Explicit `.free()` for WASM-side objects** (`Muxer`, `Demuxer`, encoder/session
  handles): JS GC cannot see into WASM memory, so examples release handles when done —
  that is the browser analog of the native stack's `close()`.
- **`Uint8Array` for byte buffers**: `pollBytes(): Uint8Array`, `pushBytes(bytes)`,
  `frame.data: Uint8Array` (NV12/BGRA8). Bytes are copied into JS-owned memory.
- Typed config objects (`VideoTrackInfo`, `AudioTrackInfo`, `Rational = { num, den }`),
  `Error` subclasses (`EncoderUnavailableError`) for expected failures.
- **Name collision**: WebCodecs already has a global `VideoFrame` type; Mediaway's
  frame type must be imported under an alias in examples (`MediawayVideoFrame`) or the
  types must be distinct.
- `EncodeSession` terminal `finish(): Uint8Array`; `MediaStreamTrackProcessor` feeds
  native `VideoFrame`s from a captured track into `writeFrame`.

## Example scenarios

`examples/` mirrors the Rust `examples/` layout — sector subfolders, one file per
scenario (English comments only; each file's header comment states real vs.
aspirational):

| File | Capability | Real today? |
|---|---|---|
| `container/mux-roundtrip.ts` | mux 90 fake video + audio packets → fMP4 → demux back, count packets | ✅ core proven in-WASM (`iso-bmff-wasm`) |
| `pipeline/encode-to-mp4.ts` | WebCodecs H.264 encode of 90 synthetic NV12 frames → fMP4 | 🚧 depends on the package + WebCodecs availability |
| `pipeline/encode-audio.ts` | mic PCM → AAC → audio-only fMP4 | 🚧 aspirational — audio encode is real in the C ABI (v2) but `@mediaway/browser` has no wasm-bindgen AAC surface yet; capture is native `getUserMedia` |
| `device/camera-record.ts` | `getUserMedia` camera → WebCodecs encode → fMP4 | 🚧 aspirational |
| `device/capture-microphone.ts` | `getUserMedia` microphone, level capture | ✅ native Web API |
| `pipeline/screen-record.ts` | `getDisplayMedia` + mic → WebCodecs encode → fMP4 | 🚧 aspirational (Chromium Insertable-Streams flag) |
| `device/capture-screen.ts` | `getDisplayMedia` screen capture, frame count | ✅ native Web API (Chromium Insertable-Streams flag) |

## Rules

- English comments only.
- Map existing Rust surfaces; do not invent capabilities the Rust side doesn't have —
  but note the browser's capture is *native Web APIs*, which is a host capability, not
  an invention.
- No C ABI in this host, ever: no `ctypes`, no ffi, no pointers in examples.
- Not part of the Cargo workspace; durable API changes require an ADR (ADR-0004).
