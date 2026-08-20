# Node.js binding (JS/TS)

> **Status: ✅ verified** — the `@mediaway/*` packages in `packages/` are real
> (FFI over the C ABI via `koffi`) and the examples in `examples/` run against the
> native libraries: mux/demux roundtrip, real H.264 encode, real camera + mic + screen
> capture. This README is the **DX contract** the packages implement; napi-rs is the
> eventual official native-addon path, koffi the current implementation.

Node.js is a **Tier B host** (through the C ABI) — distinct from the browser host
(Tier C, WASM, no C ABI). See [`../browser/README.md`](../browser/README.md) for that
host and `docs/spec/c-ffi.md` § Tier C; see [`../c/README.md`](../c/README.md) for the
underlying C ABI contract.

The native library behind that ABI is 100% Rust — no `libav*`/GPL codec
dependencies, memory-safe by construction on the native side. These packages
are thin `koffi` FFI wrappers, not a reimplementation.

**Platforms**: Windows x64 is the fully hardware-verified platform (device/pipeline
capture and encode). Linux x64 is container-verified (mux/demux); device/pipeline
capability on Linux is untested here.

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
2. **Pipeline — auto video encode → fMP4** (`@mediaway/encoder`): one call picks the
   best available OS/GPU encoder for a config, wires it into an internal MP4 muxer;
   `finish()` returns complete MP4 bytes. The audio encoder is separate (ABI v2,
   adr/0003): `AudioEncoder.open()` streams AAC packets for the caller's own muxer.
   `EncodeSession.writeFrameFromCameraCapture()`/`writeFrameFromDesktopCapture()` push a
   `@mediaway/device` capture session's polled frame straight into the encoder — no
   intermediate `VideoFrame`, no CPU copy for Screen's GPU frames (adr/pipeline/0005).
   **Decode** is its own peer package, `@mediaway/decoder` (adr/0004, adr/pipeline/0006):
   `DecodeSession` wraps the best available video decoder (CPU output only; Windows/WMF
   today), `AudioDecodeSession` wraps the cross-platform Opus decoder — both single-step
   handles (the handle IS the decoder), `NO_BACKEND` throws `DecoderUnavailableError`
   gracefully.
3. **Device — capture**: camera (CPU frames), Screen (GPU-only, Zero-Copy), microphone/
   loopback (PCM). `@mediaway/device` now includes the GPU device factory
   (`listGpuAdapters`/`GpuDevice`, `mediaway-device` ADR-0007): `openScreenCapture()`
   creates a device internally (or accepts a caller-supplied one) instead of throwing.
   `ScreenSession.pollFrame()` proves frames arrive but never copies pixels out (no CPU
   readback path in the wrapped backend) — real pixels move through
   `EncodeSession.writeFrameFromDesktopCapture()`'s Zero-Copy bridge instead. Hotplug has
   no Node wrapper yet.

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
`@mediaway/encoder` (`AutoVideoEncodeConfig`, `openAutoEncoder`, `EncodeSession`),
`@mediaway/decoder` (`DecodeSession`, `AudioDecodeSession`), `@mediaway/device`
(capture sessions, `GpuDevice`, `listGpuAdapters`). TypeScript-first with
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
| `pipeline/encode-audio.ts` | auto AAC encode of 96 synthetic F32 stereo frames → audio-only fMP4 (ABI v2) | ✅ run verified |
| `pipeline/decode-roundtrip.ts` | auto H.264 decode (encode→mux→demux→decode) + Opus audio decode round trip | ✅ run verified |
| `device/camera-record.ts` | camera + mic → H.264 + AAC → ONE two-track MP4 (remuxed; audio track registered with the encoder's AudioSpecificConfig) | ✅ run verified on real hardware; video-only fallback without mic/audio backend |
| `device/capture-microphone.ts` | microphone capture, raw PCM | ✅ run verified (real mic) |
| `pipeline/screen-record.ts` | GPU device factory → screen + mic capture → encode (bridge) → MP4 | ✅ run verified on real hardware (GPU-input H.264 encode gracefully skips as a known driver/encoder limitation, not a bug) |
| `device/capture-screen.ts` | GPU device factory → screen capture only | ✅ run verified on real hardware |

## Rules

- English comments only.
- Map existing Rust surfaces; do not invent capabilities the Rust side doesn't have.
- No raw ABI types (status enums, pointers) visible in the public API or examples.
- Not part of the Cargo workspace; durable API changes require an ADR (ADR-0004).
