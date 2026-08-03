# ADR-0003: Auto audio encode C ABI — `AudioEncoder` reachable from C

- **Status**: Accepted
- **Date**: 2026-08-03
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi`

## Context

Every recording pipeline in the workspace (Rust `examples/pipeline/screen_record.rs`,
`bindings/csharp/examples/ScreenRecord.cs`, `CameraRecord.cs`, and the four verified
bindings added in `bindings/{c,cpp,python,nodejs}`) hits the same wall: **the C ABI
has no audio encoder.** Microphone PCM is captured (`mediaway-device-ffi`,
`mediaway_audio_capture_*`) but cannot become an MP4 audio track — the container
muxer (`mediaway_container_ffi`) happily muxes AAC packets, but nothing produces them
from C. All bindings drain and discard mic PCM, with a drain-only placeholder where the
audio track belongs.

The Rust side already solves this: `mediaway_encoder::AudioEncoder` is a streaming
trait (`push_frame` / `poll_packet` / `flush`, ADR-tracked design in
`mediaway-encoder`), and `mediaway_encoder_windows::WindowsAudioEncoder` wraps the
WMF AAC encoder — **hardware-verified** in `crates/mediaway/tests/screen_mic_av_smoke.rs`
(real microphone signal → real AAC). It is only unreachable from C.

## Decision

Add an auto audio-encode surface to `mediaway-ffi` (pipeline.h ABI v2),
mirroring the video surface's shapes but **single-step**:

- `mediaway_audio_encode_config_t` — plain value struct `{codec, sample_rate,
  channels, sample_format, time_base, bitrate_bps}`, plus the sugar constructor
  `mediaway_audio_encode_config_aac(sample_rate, time_base)` (mirrors
  `mediaway_auto_video_encode_config_h264`). `codec` is `AAC` today; only `F32`
  `sample_format` is accepted by the real Windows backend (mirrors
  `mediaway_audio_capture_config_t`'s documented constraint).
- `mediaway_audio_encoder_open(config, out_session)` — **one step, returns the
  encode session directly**. Unlike video (`mediaway_auto_encoder_open` →
  `mediaway_encode_session_open`), there is no intermediate handle: the video
  two-step exists because `EncodeSession` internally wires an MP4 muxer, which can
  fail independently of the encoder; an audio encoder session *is* the encoder (the
  caller composes packets into their own muxer via `mediaway_muxer_push_packet`), so
  a split would only recreate `mediaway_encode_session_open`'s unconditional
  handle-consumption trap for no benefit. **No consumption trap exists on this
  surface** — `close` is always safe.
- `mediaway_audio_encode_session_push_pcm(session, view)` — borrowed input view
  `{pts, duration, sample_rate, channels, sample_format, data, data_len}`, valid for
  the call only (same ownership direction as `mediaway_video_frame_t`).
- `mediaway_audio_encode_session_poll_packet(session, out_packet, out_has)` — owned
  output `{pts, dts, duration, is_keyframe, is_discard, payload, payload_len}`,
  released with `mediaway_pipeline_ffi_packet_free` (distinctly named from
  container-ffi's `mediaway_packet_free`, same convention as
  `mediaway_pipeline_ffi_buffer_free`).
- `mediaway_audio_encode_session_flush(session)` — end of input, then drain with
  `poll_packet`.
- `mediaway_audio_encode_session_stream_info(session, out_info)` — owned metadata
  `{codec, time_base, sample_rate, channels, extra_data, extra_data_len}`, released
  with `mediaway_pipeline_ffi_stream_info_free`. **`extra_data` is the AAC
  AudioSpecificConfig** (raw, MP4-`esds`-ready): an MP4 audio track is only playable
  when its `esds` carries the ASC, and the muxer needs it when the caller registers
  the track — before the first encoded packet exists. The WMF backend makes the ASC
  available only after the first input sample, so the documented call order is
  *push → stream_info → mux*.
- Statuses reuse `mediaway_pipeline_status_t` unchanged (`NO_BACKEND` /
  `UNSUPPORTED` remain expected outcomes).

**Dispatch** mirrors `mediaway_pipeline::platform::AutoEncoder`: `cfg(windows)` →
`WindowsAudioEncoder::open` (Cargo feature `audio`); every other platform →
`EncodeError::NoBackend` (graceful). Handle shape = `Box<dyn AudioEncoder>` — the
same thin-pointer pattern as `AutoEncoderHandle` (`adr/0001` §3).

**Known cost (documented, not hidden — `docs/spec/caveats-and-clarity.md`)**: the
trait takes `&AudioFrame` with an owned `Bytes` payload, so `push_pcm` copies the
borrowed PCM once into the frame — the same cost class as the video CPU-upload path.
No other allocation churn: `poll_packet` moves the encoded payload into one owned
allocation owned by the caller.

**Why `mediaway-ffi`, not a new crate**: the video auto-encoder already
lives here with the status enum, error mapping, and `catch_unwind` scaffolding; audio
encode is the same "pick the best backend, stream packets out" shape. A separate
`mediaway-audio-ffi` would duplicate all of it (crate-packaging ADR-0003 allows
per-capability `-ffi` crates but does not require splitting at this granularity).

## Consequences

- The bindings' `camera_record.*` can finally produce a **two-track MP4** (video +
  AAC audio) instead of draining mic PCM — the drain-only gap goes away.
- `screen_record` remains video-blocked (Screen capture's GPU-handle C gap,
  `mediaway-device-ffi/adr/0001` § Deferred) but its audio half is now real.
- ABI version bump `1 → 2` (`MEDIAWAY_PIPELINE_FFI_ABI_VERSION`), pre-1.0 no
  stability promise.
- WMF AAC is Windows-only; other platforms return `NO_BACKEND` until a backend
  exists (mirrors the video story).
