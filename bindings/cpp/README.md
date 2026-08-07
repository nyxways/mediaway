# C++ binding

> **Status: ✅ verified** — the RAII wrapper in `include/mediaway/mediaway.hpp` is
> real and the examples in `examples/` compile, link, and run against the native
> libraries (mux/demux roundtrip, real H.264 encode, real camera + mic capture).
> This README is the **DX contract** the wrapper implements: RAII classes, typestate
> via move, exceptions carrying the raw ABI status.

See the [C binding README](../c/README.md) for the underlying C ABI contract (status
enums, ownership, thread confinement) — the C++ wrapper translates that ABI into
idiomatic C++, it does not replace it.

The native library behind that ABI is 100% Rust — no `libav*`/GPL codec
dependencies, memory-safe by construction on the native side. This header is a
thin RAII wrapper, not a reimplementation.

## What Mediaway is (the capabilities)

A streaming-first media stack. The C ABI currently covers three capabilities (full
detail in [`../c/README.md`](../c/README.md) and `docs/spec/c-ffi.md`):

1. **Container — mux + demux, all 8 `mediaway-container` formats**: MP4/WebM share
   `container::Muxer`/`Demuxer` (`Format::Mp4`/`Format::Webm`, typestated
   `Open`→`Live` via `begin()`, never touches files — the caller owns byte I/O).
   Ogg/ADTS/FLV/MPEG-TS/MP3 get dedicated classes (`OggMuxer`/`OggDemuxer`,
   `AdtsMuxer`/`AdtsDemuxer`, `FlvMuxer`/`FlvDemuxer`, `TsMuxer`/`TsDemuxer`,
   `Mp3Muxer`/`Mp3Demuxer`) reflecting each format's own C ABI shape (no track
   registration, out-buffer-per-call mux, or a construction-time stream list —
   see each header's top comment). WAV is mux-only as a class (`WavMuxer`,
   consuming `finish()`); demux is the one-shot `container::wavParse()` function,
   not a class at all. Fully real, all formats link+run verified.
2. **Pipeline — auto video encode → fMP4**: one call picks the best available OS/GPU
   encoder for a config, wires it into an internal MP4 muxer; `finish()` returns
   complete MP4 bytes. **Video only** — the audio encoder is separate (ABI v2,
   adr/0003): `AudioEncoder::open` streams AAC packets for the caller's own muxer.
3. **Device — capture**: camera (CPU frames), microphone/loopback (PCM), hotplug.
   **Screen capture is `UNSUPPORTED` from C today** (needs a GPU device handle with
   no C representation yet) — an honest gap, not a bug.

## The real ABI beneath (what the wrapper wraps)

Headers `crates/mediaway-*-ffi/include/mediaway/{container,pipeline,device}.h`.

- Opaque handles (`mediaway_muxer_t*`, `mediaway_demuxer_t*`, `mediaway_auto_encoder_t*`,
  `mediaway_encode_session_t*`, `mediaway_video_capture_t*`, `mediaway_audio_capture_t*`),
  all **thread-confined** (no concurrent calls on one handle).
- Every status is a per-crate enum (`mediaway_status_t` / `mediaway_pipeline_status_t` /
  `mediaway_device_status_t`), `OK = 0`; a caught Rust panic poisons the handle.
  `NO_BACKEND` / `UNSUPPORTED` are expected outcomes, not errors.
- Ownership: borrowed inputs valid for the call only; owned outputs released via the
  matching `_free` (`mediaway_buffer_free`, `mediaway_packet_free`,
  `mediaway_stream_info_free`, `mediaway_pipeline_ffi_buffer_free`,
  `mediaway_device_video_frame_free`, `mediaway_device_audio_frame_free`).
- Handle-consumption traps the wrapper MUST hide: `mediaway_encode_session_open` takes
  ownership of the encoder unconditionally (success or failure); likewise
  `mediaway_encode_session_finish` consumes the session. The C++ API must make these
  unrepresentable (move semantics / typestate), not document them.

## Ideal API — the DX contract

Header-only (or tiny) RAII wrapper, namespace `mediaway::` with per-capability
sub-namespaces (`mediaway::container`, `mediaway::encoder`, `mediaway::device`).
One class per opaque handle, owning it via `std::unique_ptr` + custom deleter.
Translating the ABI's errors into C++ exceptions at the boundary is the
idiomatic shape; examples catch `mediaway::Error` (carrying the raw status) at `main`.

- `mediaway::container::Muxer` — track registration (Open state) then
  `LiveMuxer liveMuxer = std::move(muxer).begin();` (**typestate via move**: the
  Open-state type is consumed, the Live-state type returned — the ABI's
  "add_track after begin is INVALID_STATE" becomes a compile error).
- `mediaway::container::Demuxer` — `pushBytes(const Bytes&)`, `streams()`,
  `std::optional<Packet> pollPacket()`, `setDecryptionKey`.
- `mediaway::encoder::AutoVideoEncoder::open(config)` (throws on `NO_BACKEND` /
  unavailable), `mediaway::encoder::EncodeSession` — `writeFrame`, `finish()` returns
  `Bytes`; **ownership of the encoder transfers into the session at construction and
  into `finish()`** — never expose a close-after-open on the consumed objects.
- Value types: `Rational{num, den}`, `TrackId`, `VideoStreamInfo`/`AudioStreamInfo`,
  `Packet`, `Bytes = std::vector<std::uint8_t>`; `VideoFrame` for encode input
  (NV12/BGRA8 CPU bytes).
- Destructors close handles (RAII) so examples never leak; explicit `close()` only
  where the ABI's close does real work (`mediaway_*_capture_close` joins a worker
  thread — can block up to one frame interval).

## Example scenarios

`examples/` mirrors the Rust `examples/` layout — sector subfolders, one file per
scenario (English comments only; each file's header comment states real vs.
aspirational):

| File | Capability | Real today? |
|---|---|---|
| `container/mux_roundtrip.cpp` | mux 90 fake video + audio packets → fMP4 → demux back, count packets | ✅ link+run verified |
| `container/all_formats_smoke.cpp` | round-trip all 7 non-MP4 formats (WebM, Ogg, ADTS, FLV, MPEG-TS incl. `finish()`, MP3, WAV incl. `wavParse()`) | ✅ link+run verified |
| `pipeline/encode_to_mp4.cpp` | auto H.264 encode of 90 synthetic NV12 frames → `out.mp4` | ✅ link+run verified |
| `pipeline/encode_audio.cpp` | auto AAC encode of 96 synthetic F32 stereo frames → audio-only fMP4 (ABI v2) | ✅ link+run verified (96 packets → 27385 bytes fMP4) |
| `device/camera_record.cpp` | camera + mic → H.264 + AAC → ONE two-track MP4 (remuxed; audio track registered with the encoder's AudioSpecificConfig) | ✅ link+run verified on real hardware (46 frames + 140 AAC packets → ~256 KB two-track MP4); video-only fallback without mic/audio backend |
| `device/capture_microphone.cpp` | microphone capture, raw PCM | ✅ link+run verified (real mic) |
| `pipeline/screen_record.cpp` | screen + mic → encode → MP4 | 🚧 aspirational — `ScreenCapture::open()` throws `Status::Unsupported` today |
| `device/capture_screen.cpp` | screen capture only | 🚧 same gap, capture-only |

## Rules

- English comments only.
- Map existing Rust surfaces; do not invent capabilities the Rust side doesn't have.
- RAII and typestate hide the ABI's ownership traps; exceptions carry the status.
- Not part of the Cargo workspace; durable API changes require an ADR (ADR-0004).
