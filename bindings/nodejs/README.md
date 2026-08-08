# Node.js binding (JS/TS)

> **Status: ✅ verified** — the `@mediaway/*` packages in `packages/` are real
> (FFI over the C ABI via `koffi`) and the examples in `examples/` run against the
> native libraries: mux/demux roundtrip, real H.264 encode, real camera + mic
> capture. This README is the **DX contract** the packages implement; napi-rs is the
> eventual official native-addon path, koffi the current implementation.

Node.js is a **Tier B host** (through the C ABI) — distinct from the browser host
(Tier C, WASM, no C ABI). See [`../browser/README.md`](../browser/README.md) for that
host and `docs/spec/c-ffi.md` § Tier C; see [`../c/README.md`](../c/README.md) for the
underlying C ABI contract.

The native library behind that ABI is 100% Rust — no `libav*`/GPL codec
dependencies, memory-safe by construction on the native side. These packages
are thin `koffi` FFI wrappers, not a reimplementation.

## What Mediaway is (the capabilities)

A streaming-first media stack. The C ABI currently covers three capabilities (full
detail in [`../c/README.md`](../c/README.md)):

1. **Container — mux + demux, all 8 `mediaway-container` formats**: MP4/WebM share
   `Muxer`/`Demuxer` (`new Muxer("mp4" | "webm")`, typestated Open→Live via `begin()`,
   never touches files — the caller owns byte I/O). Ogg/ADTS/FLV/MPEG-TS/MP3 get
   dedicated classes (`OggMuxer`/`OggDemuxer`, `AdtsMuxer`/`AdtsDemuxer`,
   `FlvMuxer`/`FlvDemuxer`, `TsMuxer`/`TsDemuxer`, `Mp3Muxer`/`Mp3Demuxer`) reflecting
   each format's own C ABI shape — see each module's (`ogg.ts`/`adts.ts`/`flv.ts`/
   `ts.ts`/`mp3.ts`) top comment. WAV is mux-only (`WavMuxer`, consuming `finish()`);
   demux is the one-shot `parseWav()` function, not a class at all. Fully real, all
   formats run-verified.
2. **Pipeline — auto video encode → fMP4**: one call picks the best available OS/GPU
   encoder for a config, wires it into an internal MP4 muxer; `finish()` returns
   complete MP4 bytes. **Video only** — the audio encoder is separate (ABI v2,
   adr/0003): `AudioEncoder.open()` streams AAC packets for the caller's own muxer.
3. **Device — capture**: camera (CPU frames), microphone/loopback (PCM), hotplug.
   **Screen capture is `UNSUPPORTED` from C today** (needs a GPU device handle with no
   C representation yet) — an honest gap, not a bug.

## The real ABI beneath (what the wrapper wraps)

DLLs: `mediaway_ffi`, `mediaway_ffi`, `mediaway_ffi` (built
for `x86_64-pc-windows-gnu`; see the C README's build recipe). Headers
`crates/mediaway-*-ffi/include/mediaway/{container,pipeline,device}.h` are the
authoritative layout.

- Opaque handles, all **thread-confined** (no concurrent calls on one handle).
- Every status is a per-crate enum, `OK = 0`; a caught Rust panic poisons the handle.
  `NO_BACKEND` / `UNSUPPORTED` are expected outcomes, not errors.
- Ownership: borrowed inputs valid for the call only (the wrapper must copy in);
  owned outputs (`pollBytes` buffers, demuxed packets/stream info, `finish` buffers,
  polled device frames) must be released via the matching `_free` — the wrapper makes
  this automatic.
- Handle-consumption traps the wrapper MUST hide: `mediaway_encode_session_open`
  consumes the encoder unconditionally; `mediaway_encode_session_finish` consumes the
  session. In JS, fold `open` into `EncodeSession` construction and make `finish`
  terminal (session unusable afterward).

## Ideal API — the DX contract

Per-capability npm packages mirroring the Rust crate split: `@mediaway/container`
(`Muxer`, `Demuxer`, `Packet`, `VideoTrackInfo`, `AudioTrackInfo`, `Rational`),
`@mediaway/encoder` (`AutoVideoEncodeConfig`, `openAutoEncoder`, `EncodeSession`,
`VideoFrame`), `@mediaway/device` (capture sessions). TypeScript-first with
`strict` types.

- **Typed structs as plain interfaces/objects**: `Rational = { num, den }`, track
  info objects, `VideoFrame = { pts, duration, width, height, pixelFormat, data }`
  with `pixelFormat: "nv12" | "bgra8" | ...` and `data: Buffer`.
- **`Error` subclasses**, not status codes: `MediawayError` (carries raw status) with
  `EncoderUnavailableError` / `CaptureUnavailableError` subclasses for the expected
  outcomes — examples catch-and-continue on missing hardware.
- **Explicit `close()`** for handles with real close work (capture sessions join a
  worker thread; muxer/demuxer/encoder close frees the native handle). `finally`
  blocks are the idiomatic JS shape; `EncodeSession` additionally has terminal
  `finish()`.
- **`Buffer` for byte buffers**: `pollBytes(): Buffer`, `pushBytes(Buffer)`,
  `finish(): Buffer`. Callers own the bytes (copied out of native memory).
- **Async where the OS work is real**: `writeFrame`/`finish` may be async (napi-rs
  worker threads) — but a muxer that never blocks on I/O stays sync.
- `AutoVideoEncodeConfig.defaults(codec, width, height, timeBase)` plus overridable
  fields (`bitrateBps`, ...); `openAutoEncoder(config)` throws
  `EncoderUnavailableError` when no backend exists on this machine.

## Example scenarios

`examples/` mirrors the Rust `examples/` layout — sector subfolders, one file per
scenario (English comments only; each file's header comment states real vs.
aspirational):

| File | Capability | Real today? |
|---|---|---|
| `container/mux-roundtrip.ts` | mux 90 fake video + audio packets → fMP4 → demux back, count packets | ✅ run verified |
| `pipeline/encode-to-mp4.ts` | auto H.264 encode of 90 synthetic NV12 frames → `out.mp4` | ✅ run verified |
| `pipeline/encode-audio.ts` | auto AAC encode of 96 synthetic F32 stereo frames → audio-only fMP4 (ABI v2) | ✅ run verified (96 packets → 27372 bytes fMP4) |
| `device/camera-record.ts` | camera + mic → H.264 + AAC → ONE two-track MP4 (remuxed; audio track registered with the encoder's AudioSpecificConfig) | ✅ run verified on real hardware (46 frames + 140 AAC packets → ~264 KB two-track MP4); video-only fallback without mic/audio backend |
| `device/capture-microphone.ts` | microphone capture, raw PCM | ✅ run verified (real mic) |
| `pipeline/screen-record.ts` | screen + mic → encode → MP4 | 🚧 aspirational — `openScreenCapture()` throws `CaptureUnavailableError` today |
| `device/capture-screen.ts` | screen capture only | 🚧 same gap, capture-only |

## Testing

`test/mux-roundtrip.test.ts` is the container round-trip suite — the RC-stage
binding check. It exercises the **release-built DLL** end to end through the
public `@mediaway/container` API: mux 90 synthetic H.264 video packets + 90
synthetic AAC audio packets into a fragmented MP4, demux the bytes back, and
assert that packet counts, stream metadata (codec / geometry / timebase — the
MP4 demux ABI does not report audio sample rate / channels yet), timestamps,
keyframe flags, and payload bytes all survive the round trip. Deterministic
synthetic payloads only — no randomness, no files, no hardware. Any failed
assertion exits nonzero.

`test/all-formats-smoke.test.ts` covers the other 7 `mediaway-container`
formats (WebM/Ogg/ADTS/FLV/MPEG-TS/MP3/WAV) the same way, reusing the
C++/C#/Python bindings' own verified byte patterns.

The DLL must be staged at `packages/ffi/native/mediaway_ffi.dll` (the release
workflow stages it there via `tools/scripts/copy-native-dlls.ts`). Run the
suite with one command, from `bindings/nodejs/`:

    bun install && npm test

The `test` script rebuilds the `@mediaway/ffi` and `@mediaway/container` dist
with `tsc` (no cargo — the DLL is already staged), type-checks the suite, and
runs both test files via `tsx`.

Type-check the suite standalone (requires the dist build above to exist):

    bun node_modules/typescript/bin/tsc --noEmit

(Use the `node_modules/typescript/bin/tsc` path, not `npx tsc`/`bunx tsc` — on
Windows CI runners without `.bin` shims those can pull the placeholder
`tsc@2.0.4` package.)

## Rules

- English comments only.
- Map existing Rust surfaces; do not invent capabilities the Rust side doesn't have.
- No raw ABI types (status enums, pointers) visible in the public API or examples.
- Not part of the Cargo workspace; durable API changes require an ADR (ADR-0004).
