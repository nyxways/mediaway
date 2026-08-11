# Python binding

> **Status: ✅ verified** — the `mediaway` package in `mediaway/` is real
> (pure-Python `ctypes` over the C ABI) and the examples in `examples/` run against
> the native libraries: mux/demux roundtrip, real H.264 encode, real camera + mic
> capture. This README is the **DX contract** the package implements: context
> managers, exceptions, Rational-second timestamps, bytes for buffers.

See the [C binding README](../c/README.md) for the underlying C ABI contract (status
enums, ownership, thread confinement) — the Python package translates that ABI into
idiomatic Python.

The native library behind that ABI is 100% Rust — no `libav*`/GPL codec
dependencies, memory-safe by construction on the native side. This package is
a thin `ctypes` wrapper, not a reimplementation.

**Platforms**: Windows x64 is the fully hardware-verified platform (device/pipeline
capture and encode). Linux x64 is container-verified — `tests/test_mux_roundtrip.py`
and `tests/test_all_formats_smoke.py` (pure CPU, no hardware) both pass against a
real `libmediaway_ffi.so` build; device/pipeline capability on Linux is untested here
(see [`../../docs/ai/wiki/platform/linux-encode.md`](../../docs/ai/wiki/platform/linux-encode.md)
/ [`linux-decode.md`](../../docs/ai/wiki/platform/linux-decode.md) for the Rust-level
Linux encode/decode status).

## What Mediaway is (the capabilities)

A streaming-first media stack. The C ABI currently covers three capabilities (full
detail in [`../c/README.md`](../c/README.md) and `docs/spec/c-ffi.md`):

1. **Container — mux + demux, all 8 `mediaway-container` formats**: MP4/WebM share
   `Muxer`/`Demuxer` (`format=ContainerFormat.MP4`/`.WEBM`, typestated Open→Live via
   `begin()`, never touches files — the caller owns byte I/O). Ogg/ADTS/FLV/MPEG-TS/MP3
   get dedicated classes (`OggMuxer`/`OggDemuxer`, `AdtsMuxer`/`AdtsDemuxer`,
   `FlvMuxer`/`FlvDemuxer`, `TsMuxer`/`TsDemuxer`, `Mp3Muxer`/`Mp3Demuxer`) reflecting
   each format's own C ABI shape — see each module's (`_container_*.py`) top comment.
   WAV is mux-only (`WavMuxer`, consuming `finish()`); demux is the one-shot
   `wav_parse()` function, not a class at all. These 6 formats use `RawPacket`
   (ABI-native integer pts/dts, not `Rational` seconds) since none of them have MP4's
   per-track time base to convert against. Fully real, all formats run-verified.
2. **Pipeline — auto video encode → fMP4, plus decode**: one call picks the best
   available OS/GPU encoder for a config, wires it into an internal MP4 muxer;
   `finish()` returns complete MP4 bytes. The audio encoder is separate (ABI v2,
   adr/0003): `AudioEncoder.open()` streams AAC (or Opus) packets for the caller's
   own muxer. Decode is the mirror shape (adr/0004, adr/pipeline/0006):
   `DecodeSession` wraps the best available video decoder (CPU output only;
   Windows/WMF today), `AudioDecodeSession` wraps the cross-platform Opus decoder —
   both single-step handles (the handle IS the decoder), `NO_BACKEND` raises
   `DecoderUnavailableError` gracefully.
3. **Device — capture**: camera (CPU frames), microphone/loopback (PCM), hotplug.
   **Screen capture is real** (GPU-backed, DXGI Desktop Duplication) via the
   `GpuDevice` factory (adr/0007-gpu-device-factory.md) — `VideoCapture.open(source=
   "screen")` builds one internally, or share your own with an encoder. There is no
   CPU pixel readback path for Screen frames; real pixels only ever move through
   `EncodeSession.write_frame_from_desktop_capture` (adr/pipeline/0005). Window
   capture is still `UNSUPPORTED` from C (no constructor this pass) — an honest gap.

## The real ABI beneath (what the wrapper wraps)

DLLs: `mediaway_ffi`, `mediaway_ffi`, `mediaway_ffi` (built
for `x86_64-pc-windows-gnu`, see the C README's build recipe). Headers
`crates/mediaway-*-ffi/include/mediaway/{container,pipeline,device}.h` are the
authoritative layout.

- Opaque handles, all **thread-confined** (no concurrent calls on one handle).
- Every status is a per-crate enum, `OK = 0`; a caught Rust panic poisons the handle.
  `NO_BACKEND` / `UNSUPPORTED` are expected outcomes, not errors.
- Ownership: borrowed inputs valid for the call only (the wrapper must copy in);
  owned outputs (`poll_bytes` buffers, demuxed packets/stream info, encode `finish`
  buffers, polled device frames) must be released via the matching `_free` — the
  wrapper's job is to make this automatic (context managers / finalizers).
- Handle-consumption traps the wrapper MUST hide: `mediaway_encode_session_open`
  consumes the encoder unconditionally; `mediaway_encode_session_finish` consumes the
  session. Python can hide this by folding `open` into `EncodeSession` construction and
  making `finish` terminal.

## Ideal API — the DX contract

A single `mediaway` package, pure-Python `ctypes` glue + idiomatic wrappers.
**snake_case everywhere** (Python convention beats the C names): `Rational`,
`VideoStreamInfo`, `AudioStreamInfo`, `Packet`, `Codec` (enum), `VideoFrame`;
classes `Muxer`, `Demuxer`, `EncodeSession`, `AutoVideoEncoder`, `VideoCapture`,
`AudioCapture`, `GpuDevice`.

- **Context managers**: `with Muxer() as m:`, `with Demuxer() as d:`,
  `with EncodeSession(...) as s:` — `__exit__` closes the underlying handle (and, for
  capture sessions, joins the backend worker thread). This is the primary lifecycle
  shape; explicit `.close()` exists for non-`with` users.
- **Exceptions**: a `MediawayError` base carrying the raw status code, with subclasses
  for the expected outcomes (`EncoderUnavailableError`, `DeviceUnavailableError`,
  `CaptureUnsupportedError`) so examples can catch-and-continue rather than crash on
  missing hardware. No status-code checking in example bodies.
- **Typestate as two classes** (mirrors C++): `Muxer` (Open: `add_video_track` /
  `add_audio_track`) → `.begin()` returns `LiveMuxer` (push_packet / flush /
  poll_bytes). Calling track registration on a `LiveMuxer` is impossible, matching the
  ABI's `INVALID_STATE`.
- **bytes for byte buffers**: `poll_bytes() -> bytes`; `push_bytes(bytes)`;
  `packet.payload -> bytes`; `frame.data -> bytes` (NV12/BGRA8). The wrapper copies
  out of borrowed/owned native buffers — no `memoryview` leaking into the API.
- `Rational(num, den)` as a small `dataclass(frozen=True)`; info structs as dataclasses.
- `EncodeSession(encoder)` takes ownership of the encoder object; `finish() -> bytes`
  is terminal (no `close()` after it).

## Example scenarios

`examples/` mirrors the Rust `examples/` layout — sector subfolders, one file per
scenario (English comments only; each file's header comment states real vs.
aspirational):

| File | Capability | Real today? |
|---|---|---|
| `container/mux_roundtrip.py` | mux 90 fake video + audio packets → fMP4 → demux back, count packets | ✅ run verified |
| `pipeline/encode_to_mp4.py` | auto H.264 encode of 90 synthetic NV12 frames → `out.mp4` | ✅ run verified |
| `pipeline/encode_audio.py` | auto AAC encode of 96 synthetic F32 stereo frames → audio-only fMP4 (ABI v2) | ✅ run verified (96 packets → 27372 bytes fMP4) |
| `pipeline/decode_roundtrip.py` | auto H.264 decode (encode→mux→demux→decode) + Opus audio decode round trip | ✅ run verified (10 video frames, 50 Opus frames) |
| `device/camera_record.py` | camera + mic → H.264 + AAC → ONE two-track MP4 (remuxed; audio track registered with the encoder's AudioSpecificConfig) | ✅ run verified on real hardware (47 frames + 80 AAC packets → ~251 KB two-track MP4); video-only fallback without mic/audio backend |
| `device/capture_microphone.py` | microphone capture, raw PCM | ✅ run verified (real mic) |
| `pipeline/screen_record.py` | screen + mic → encode → MP4, via `GpuDevice` + the capture-to-encode bridge | ✅ run verified on real hardware (real 2560x1440 GPU-backed frames bridged; GPU-input encode itself gracefully skips as `UNSUPPORTED` on this dev machine's current encoder/driver — same pre-existing limitation the Rust/C/Node.js/C# siblings hit, not introduced here); mic PCM drained, not muxed (see `camera_record.py` for two-track remux) |
| `device/capture_screen.py` | screen capture only, via `GpuDevice` | ✅ run verified on real hardware (5 real 2560x1440 GPU-backed frames polled) |

## Testing

The release pipeline stages the built native library at `mediaway/_native/`
(`mediaway_ffi.dll` on Windows, `libmediaway_ffi.so` on Linux — the wheel's
native directory; `_ffi.py` picks the filename via `platform.system()`). The
round-trip binding check validates that library against the documented ABI
contract:

```
python tests/test_mux_roundtrip.py
```

Run from `bindings/python/`. Pure std-lib (no pytest), assert-based: it muxes
90 synthetic H.264 video + 90 synthetic AAC audio packets into a fragmented
MP4, demuxes the bytes back, and asserts the 1:1 packet round-trip plus the
recovered stream metadata (video codec/dimensions/frame rate, audio codec). A
failed assertion exits nonzero, which is the CI job's failure signal. Pure CPU
— no hardware required.

`tests/test_all_formats_smoke.py` covers the other 7 `mediaway-container`
formats (WebM/Ogg/ADTS/FLV/MPEG-TS/MP3/WAV) the same way, reusing the
C++/C# bindings' own verified byte patterns:

```
python tests/test_all_formats_smoke.py
```

## Rules

- English comments only.
- Map existing Rust surfaces; do not invent capabilities the Rust side doesn't have.
- Wrap the ABI fully: no `ctypes` types or raw handles visible in examples.
- Not part of the Cargo workspace; durable API changes require an ADR (ADR-0004).
