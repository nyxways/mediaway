# Browser binding (Web / WASM)

> **Status: ✅ verified** — real `@mediaway/browser` npm package (ADR-0020 +
> [ADR-0022](../../docs/adr/0022-browser-decode-session-and-device-dx.md),
> [`docs/adr/0020-browser-wasm-npm-package.md`](../../docs/adr/0020-browser-wasm-npm-package.md)):
> WASM fMP4 mux/demux + WebCodecs encode/decode round trips, verified in real
> Chromium/Edge (E2E specs under `tools/e2e-web`,
> `browser-package.spec.ts`). This README is the **DX contract** the package
> implements, and what its examples demonstrate.

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

## The API — the DX contract (implemented)

A single `@mediaway/browser` package, TypeScript-first, `wasm-bindgen`-generated
classes wrapped in idiomatic JS (`packages/browser/`, built with
`npm run build` → `dist/` + wasm-pack `pkg/`).

- **One `init()` before anything else**: `await init()` fetches + instantiates the WASM
  module; every call after it resolves is synchronous-feeling. In Node (dev runs) pass
  the wasm bytes explicitly.
- **Explicit `.free()` for WASM-side objects** (`Muxer`, `Demuxer`): JS GC cannot see
  into WASM memory, so examples release handles when done — the browser analog of the
  native stack's `close()`.
- **`Uint8Array` for byte buffers**: `pollBytes(): Uint8Array`, `pushBytes(bytes)`,
  `sample.payload: Uint8Array`. Bytes are copied at the boundary.
- Typed config objects (`Track`, `Sample`, `Rational = { num, den }`), `Error`
  subclasses `EncoderUnavailableError` / `DecoderUnavailableError` for expected
  failures.
- **Codec config arrives late**: WebCodecs exposes avcC/AudioSpecificConfig only in
  the first output's metadata, so `EncodeSession` defers `begin()` until every planned
  track has its config — the browser analog of the C ABI's push → stream_info → mux
  order (`mediaway-ffi/adr/0003`).
- `EncodeSession` terminal `finish(): Uint8Array` (flush encoders + muxer).
  Capture feeds frames in via a canvas bridge (`VideoFrame(canvas)` — universal,
  no Insertable-Streams flag needed).

## Example scenarios

`examples/` mirrors the Rust `examples/` layout — sector subfolders, one file per
scenario (English comments only; each file's header comment states real vs.
aspirational):

| File | Capability | Real today? |
|---|---|---|
| `container/mux-roundtrip.ts` | mux 90 fake video + audio packets → fMP4 → demux back, count packets | ✅ WASM mux/demux (also runs under Node — `npx tsx`) |
| `pipeline/encode-to-mp4.ts` | WebCodecs H.264 encode of 90 synthetic NV12 frames → fMP4 | ✅ WebCodecs-dependent (E2E-verified; bundled Chromium may skip H.264 — msedge-real covers it) |
| `pipeline/encode-audio.ts` | synthetic PCM → WebCodecs AAC → audio-only fMP4 | ✅ WebCodecs-dependent (E2E-verified; ASC pulled from the encoder's first-output description) |
| `device/camera-record.ts` | `getUserMedia` camera → canvas bridge → WebCodecs encode → fMP4 | ✅ needs a real camera (native camera_record.* verified on hardware) |
| `device/capture-microphone.ts` | `getUserMedia` microphone, level capture | ✅ native Web API |
| `pipeline/screen-record.ts` | `getDisplayMedia` + canvas bridge → WebCodecs encode → fMP4 | ✅ browser — the C-ABI hosts cannot screen-capture from C at all (device-ffi adr/0001 § Deferred) |
| `device/capture-screen.ts` | `getDisplayMedia` screen capture, frame count | ✅ native Web API |
| `device/list-and-watch-devices.ts` | `enumerateDevices()` list + `devicechange` hotplug — added/removed by `deviceId` (native analog of `mediaway-device`'s `DeviceId`/`Select`/hotplug, ADR-0005) | ✅ native Web API |
| `pipeline/decode-roundtrip` (E2E harness: `browser-package.html` decode section) | demux fMP4 → WebCodecs `VideoDecoder`/`AudioDecoder` → decoded `VideoFrame`/`AudioData` (`DecodeSession`, ADR-0022) | ✅ WebCodecs-dependent (E2E-verified in `browser-package.spec.ts`; bundled Chromium may skip H.264/AAC — msedge-real covers it) |

## Rules

- English comments only.
- Map existing Rust surfaces; do not invent capabilities the Rust side doesn't have —
  but note the browser's capture is *native Web APIs*, which is a host capability, not
  an invention.
- No C ABI in this host, ever: no `ctypes`, no ffi, no pointers in examples.
- Not part of the Cargo workspace; durable API changes require an ADR
  ([`docs/adr/0020-browser-wasm-npm-package.md`](../../docs/adr/0020-browser-wasm-npm-package.md)).
- E2E: `tools/e2e-web` Playwright suite — `browser-package.spec.ts` drives the built
  package in Chromium + real Edge.
