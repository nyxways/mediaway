# mediaway-audio-apm — roadmap

**Facade** crate wrapping the external `sonora` / `sonora-agc2` / `sonora-simd`
crates (pure Rust, BSD-3-Clause). No platform split — `sonora` is
cross-platform CPU/SIMD, not an OS API, so unlike `mediaway-device` /
`mediaway-encoder` this crate has no `mediaway-audio-apm-<platform>` siblings
(same shape as `mediaway-wgpu`: one crate, one external framework, no OS
backends). Packaging: [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md).
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold + ADR (2026-07-31)

- [x] Crate directory + `README.md` / `docs/roadmap.md` / `adr/`
- [x] [ADR-0001](../adr/0001-sonora-audio-processing-adoption.md) (**Accepted**):
      license verdict (BSD-3-Clause, already on `deny.toml`'s allow-list),
      crate placement (new facade, not folded into `mediaway-device` /
      `mediaway-common` / `mediaway-pipeline`), `AudioProcessor` /
      `VoiceActivityDetector` API shape (push/poll, render+capture split —
      **not** a `FrameFilter`-parallel `AudioFilter` trait), panic-safety
      posture (catch-and-disable, not FFI-driven), VAD i16-scale gotcha.
- [x] Added to `[workspace] members` / `[workspace.dependencies]`; `sonora` /
      `sonora-agc2` / `sonora-simd` behind this crate's `apm` / `vad`
      features; `cargo deny check` clean against the resolved graph.

### 1 — `AudioProcessor` (AEC3 + NS + AGC2), `apm` feature — done

- [x] `sonora` in `[workspace.dependencies]`, gated behind `apm` (`dep:sonora`)
- [x] `ApmConfig` = `sonora::Config` re-exported directly (+ `config` module
      re-export for `EchoCanceller`/`NoiseSuppression`/`GainController2`/…)
      rather than a duplicated parallel surface — `AudioStreamFormat` defined
      locally (`src/processor.rs`)
- [x] `AudioProcessor::open` / `push_render_frame` / `push_capture_frame` /
      `poll_processed_frame` / `set_stream_delay_ms` / `is_disabled` — see
      ADR-0001 § API shape
- [x] `catch_unwind`-based disable-on-panic (ADR-0001 § Panic-safety posture),
      `ApmError::BackendPanicked`; non-panic `sonora::Error` returns surfaced
      via `ApmError::Backend` without disabling the instance
- [x] `SampleFormat::F32`-only validation at `open` and on every pushed frame
      (`ApmError::UnsupportedSampleFormat`) + `ApmError::StreamFormatMismatch`
      for rate/channel mismatches
- [x] Internal 10ms re-blocking (render + capture separately, flat interleaved
      accumulator + reused deinterleaved scratch) — unit-tested with
      synthetic PCM (`src/processor_tests.rs`), no real mic needed
- [x] rustdoc "costly path" note on `AudioProcessor`: hot path is **not**
      CPU ⚡ — deinterleave + re-block + `sonora`'s own src/dst-slice contract
      force a real payload copy on every frame

### 2 — `VoiceActivityDetector` (RNN VAD), `vad` feature — done

- [x] `sonora-agc2` + `sonora-simd` in `[workspace.dependencies]`, gated
      behind `vad` (`dep:sonora-agc2`, `dep:sonora-simd`)
- [x] `VoiceActivityDetector::open` (wraps `sonora_agc2::vad_wrapper::VoiceActivityDetectorWrapper::new`
      + `sonora_simd::detect_backend()`)
- [x] `analyze(&mut self, frame: &AudioFrame) -> Result<f32, ApmError>` —
      documented to run on `AudioProcessor::poll_processed_frame()`'s output
      (already 10ms-blocked, post-NS); requires an exact 10ms block
      (`ApmError::FrameLengthMismatch` otherwise, no internal re-blocking)
- [x] ×32768.0 i16-scale correction applied internally before calling
      `sonora`'s `analyze` (ADR-0001 § VAD scaling gotcha) — regression test
      (`speech_like_frame_reports_higher_probability_than_near_silent` in
      `src/vad_tests.rs`) mirroring nyxie_voice's
      `vad_amplitude_sweep_finds_silence_boundary` finding
- [x] Panic-safety posture diverges intentionally from `AudioProcessor` here:
      a scalar VAD score has no honest passthrough, so once disabled every
      `analyze` call returns `ApmError::BackendPanicked` (documented on
      `analyze`), not a synthesized probability

### 3 — Integration examples (no `EncodeSession` hook yet)

- [ ] Example: `mediaway-device` `AudioCapture::poll_frame()` →
      `AudioProcessor::push_capture_frame` → `poll_processed_frame` →
      `mediaway-encoder` `AudioEncoder::push_frame`, entirely at the caller
      level (mirrors how `mediaway-pipeline`'s screen-record smoke test
      composes audio manually today — see that crate's roadmap § 1b)
- [ ] Revisit `mediaway-pipeline::EncodeSession` integration only once that
      crate's own Stage 1b (audio/multi-track `EncodeSession`) is scoped —
      separate ADR, not this one

### 4 — Deferred / explicit scope cuts

- [ ] A second APM backend (e.g. an OS-native AEC path such as Windows WASAPI
      Voice Processing) is a `mediaway-device-windows` **capture-level**
      concern, not this crate's — no `AudioProcessor` trait abstraction until
      a second real backend exists (ADR-0001 § Alternatives)
- [ ] No FFI crate (`mediaway-audio-apm-ffi`) until a real Rust MVP lands
      (mirrors every other `*-ffi` crate's precedent)
