# `mediaway-audio-apm` — echo cancel / noise suppress / gain control / VAD

**Status: Implemented** —
[`crates/mediaway-audio-apm/adr/0001-sonora-audio-processing-adoption.md`](../../../../crates/mediaway-audio-apm/adr/0001-sonora-audio-processing-adoption.md)
(Accepted).

- **What:** wraps [`sonora`](https://github.com/dignifiedquire/sonora) (+
  `sonora-agc2`, `sonora-simd`) — pure-Rust, SIMD-accelerated, BSD-3-Clause
  port of Google's WebRTC AudioProcessing (AEC3 + NS + AGC2 + RNN VAD).
- **Two independent public types:** `AudioProcessor` (`apm` feature) and
  `VoiceActivityDetector` (`vad` feature) — concrete, not trait-based; each
  usable standalone.
- **Where in the pipeline:** right after mic capture
  (`mediaway_device_audio::AudioCapture::poll_frame`), before anything else
  touches the signal. As of `mediaway` ADR-0003, `EncodeSession`
  wires this in transparently (`attach_audio_processor`/`attach_vad`,
  called from inside `write_audio_frame`) — see
  [pipeline/audio-track-and-apm](../pipeline/audio-track-and-apm.md). This
  crate's own public types (`AudioProcessor`, `VoiceActivityDetector`) are
  unchanged by that integration and remain directly usable standalone.
- **Shape — deliberately not `FrameFilter`-parallel:** `sonora` needs two
  input streams (render reference + capture), not one, and processes fixed
  10ms blocks regardless of real capture-frame size. `AudioProcessor` is
  push/poll-shaped (`push_render_frame` / `push_capture_frame` /
  `poll_processed_frame`), mirroring `VideoEncoder`/`AudioEncoder`'s own
  push/poll idiom.
- **Config:** `ApmConfig` = `sonora::Config` re-exported directly (plus the
  whole `sonora::config` module, re-exported as `mediaway_audio_apm::config`)
  — no parallel config surface; construct
  `ApmConfig { echo_canceller: Some(config::EchoCanceller::default()), .. }`.
  All components disabled by default (matches `sonora`'s own default).
- **Panic-safety:** every `sonora`/`sonora-agc2` call site (`build`,
  `process_render_f32`, `process_capture_f32`, `analyze`) is wrapped in
  `catch_unwind(AssertUnwindSafe(...))`. On the call that catches a panic:
  `Err(ApmError::BackendPanicked)`, and the instance's `inner` becomes
  `None` (`is_disabled() == true`) for its remaining lifetime. **After**
  that: `AudioProcessor::poll_processed_frame` passes raw accumulated PCM
  through unmodified (the underlying signal is still valid, only the
  enhancement is lost) — `push_capture_frame` keeps buffering even while
  disabled so there is raw PCM to pass through; `push_render_frame` becomes
  a no-op (no benefit to buffering an echo reference nothing will use).
  `sonora`'s own **non-panic** typed errors (e.g. an unsupported sample
  rate) surface as `ApmError::Backend` and do **not** disable the instance.
- **VAD disabled behavior diverges from `AudioProcessor`:** a scalar speech
  probability has no honest passthrough — synthesizing a fixed value (e.g.
  always `0.0`) would be silently wrong for a caller gating on it. Once
  disabled, every `VoiceActivityDetector::analyze` call returns
  `Err(ApmError::BackendPanicked)` (mirrors this workspace's `*-ffi`
  poisoned-handle "error forever" precedent, applied here because — unlike
  `AudioProcessor` — there is no safe degraded output to fall back to).
- **VAD gotcha:** the RNN VAD assumes i16-scale (±32768) PCM internally; f32
  `[-1, 1]` input reads as permanent silence unless scaled ×32768.0 first —
  `VoiceActivityDetector::analyze` does this internally (`SONORA_PCM_SCALE`
  in `src/vad.rs`) so callers never see the scale factor. Regression test:
  `speech_like_frame_reports_higher_probability_than_near_silent` in
  `src/vad_tests.rs` — asserts a realistic-amplitude synthetic "speech-like"
  signal scores meaningfully higher than near-silence, which only holds if
  the scale is actually applied.
- **`analyze` has no internal re-blocking** — `frame` must carry exactly
  `sample_rate / 100` samples per channel (one 10ms block) or it returns
  `ApmError::FrameLengthMismatch`; intended input is
  `AudioProcessor::poll_processed_frame`'s output, already shaped that way.
- **Not Zero-Copy (CPU ⚡):** `sonora`'s src/dst-slice API + 10ms re-blocking
  force a real payload copy on the hot path (`src/pcm.rs` byte↔f32
  conversion, `#![forbid(unsafe_code)]` rules out a pointer-cast reinterpret)
  — never claim ⚡ here (see [zero-copy/marks](../zero-copy/marks.md)).
- **Crate shape:** one crate, no platform split (like `mediaway-wgpu`) —
  `sonora` is cross-platform CPU/SIMD, not an OS API.

See also: [pipeline/frame-filter-hook](../pipeline/frame-filter-hook.md) (the
video-side precedent this design explicitly diverges from, with reasons) and
[device/windows-audio](../device/windows-audio.md) (the capture stage this
crate sits downstream of).
