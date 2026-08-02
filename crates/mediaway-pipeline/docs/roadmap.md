# mediaway-pipeline — roadmap

**Facade-of-facades** crate (composition only, no traits of its own).
Packaging: [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md).
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Crate + `docs/` / `adr/`
- [x] Workspace ADR-0014: crate exists, `EncodeSession` shape, platform dispatch migration
- [x] `EncodeSession<E: VideoEncoder>` — open / write_frame / finish
- [x] `platform` module — `AutoEncoder`/`AutoDecoder`/`ScreenCapture`/`Microphone`
      marker types, each with an `open` associated function (migrated from
      `examples/platform.rs`; renamed from free functions 2026-07-31, see ADR-0014's
      addendum — `screen_config` dropped as a duplicate of
      `VideoCaptureConfig::screen`)
- [x] `PipelineError` (`thiserror`, wraps `EncodeError` + container `Error`)

### 1 — Windows

- [x] `AutoVideoEncoder` (unboxed) and `Box<dyn VideoEncoder>` both work with `EncodeSession`
- [x] Screen-record composed through this crate end-to-end (`platform::ScreenCapture::open`
      + `platform::Microphone::open` + `platform::AutoEncoder::open` DX11 Zero-Copy H.264 +
      `WindowsAudioEncoder` AAC → shared two-track `mediaway_container::mp4::Muxer`) —
      `tests/screen_mic_av_smoke.rs`. `EncodeSession` itself stays untouched (see 1b); the
      second track is composed directly against the muxer, mirroring
      `mediaway-encoder-windows/tests/av_fmp4_smoke.rs`
- [x] Decode → trim → splice → re-encode round trip through real mux/demux —
      `tests/trim_and_splice_windows.rs` + `examples/pipeline/trim_and_splice.rs`; this is what
      surfaced and drove the AVCC/Annex-B extradata fix in `mediaway-decoder-windows`
      ADR-0001 (demuxed samples are AVCC-framed, encoder/decoder MFTs expect Annex-B)

### 1b — Audio / multi-track — done (2026-08-01)

- [x] `EncodeSession` extension for a second (audio) track —
      [ADR-0003](../adr/0003-audio-track-and-apm-integration.md): `open_with_audio`,
      `write_audio_frame`/`write_audio_render_frame`, `finish()` flushes both tracks
- [x] Optional `mediaway-audio-apm` (AEC3+NS+AGC2 `AudioProcessor`, RNN
      `VoiceActivityDetector`) wiring — `attach_audio_processor`/`attach_vad`,
      `poll_vad_score`; unit-tested with synthetic PCM (`src/session_tests.rs`), no real
      mic needed
- [ ] Migrate `tests/screen_mic_av_smoke.rs` off its hand-rolled second-track muxing
      onto `EncodeSession::open_with_audio` directly (now possible, not yet done)

### 2 — Web / 3 — Linux / 4 — Other

- [x] `ScreenCapture::open` Linux dispatch — `mediaway-device-linux` existed as
      a workspace member with a real `LinuxScreenCapture` backend, but was
      never added as a pipeline dependency nor wired into `platform.rs`'s
      `#[cfg(...)]` dispatch; this was a real, pre-existing gap (not a
      not-yet-landed backend) — fixed this session, verified via WSL2
      (`cargo build`/`test`/`clippy -p mediaway-pipeline`, real Linux target)
- [ ] Extend `platform` dispatch as remaining backends (Web, camera, audio on
      Linux) land, following the workspace platform order

### 5 — Device capability / permission dispatch

- [x] `platform::device_support` / `platform::request_device_permission` —
      `#[cfg]` dispatch to `mediaway-device-windows`/`-linux`
      `capabilities::{support, request_permission}`, mirroring
      `ScreenCapture::open`'s pattern (see `mediaway-device` ADR-0003)

### 6 — Mid-pipeline frame filter hook

- [x] `FrameFilter` trait + `FilterError` (`src/filter.rs`), `EncodeSession::filters`
      (`SmallVec<[Box<dyn FrameFilter>; 4]>`) and `push_filter` (`src/session.rs`),
      `PipelineError::Filter(#[from] FilterError)` — additive, `open`/`write_frame`
      signatures unchanged; v1 is CPU-frame-only, `Gpu`-backed frames + a non-empty
      chain fail loudly with `FilterError::GpuFrameUnsupported` (see
      [ADR-0001](../adr/0001-frame-filter-hook.md))
