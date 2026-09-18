# mediaway — roadmap

**Facade-of-facades** crate (composition only, no traits of its own).
Packaging: [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md).
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Crate + `docs/` / `adr/`
- [x] Workspace ADR-0014: crate exists, `EncodeSession` shape, platform dispatch migration
- [x] `EncodeSession<E: VideoEncoder>` — open / write_frame / finish
- [x] `WindowCapture` + `DesktopAudio` markers (2026-09-19) — the two capabilities a
      single-app recorder is built on were reachable only by naming backend types. First
      entry points shaped per ADR-0002 (concrete per-target type, `Infallible` where there
      is no backend); the older markers still box.
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
      `tests/screen_mic_av_smoke.rs` (composed through `EncodeSession::open_with_audio`
      since 1b — see below; it previously hand-rolled the second track against the
      muxer, mirroring `mediaway-encoder-windows/tests/av_fmp4_smoke.rs`)
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
- [x] Migrate `tests/screen_mic_av_smoke.rs` off its hand-rolled second-track muxing
      onto `EncodeSession::open_with_audio` directly — the test now opens the session
      with both encoders, feeds capture frames through
      `write_frame`/`write_audio_frame`, and takes the finished two-track fMP4 from
      `finish()`

### 2 — Web / 3 — Linux / 4 — Other

- [x] `ScreenCapture::open` Linux dispatch — `mediaway-device-linux` existed as
      a workspace member with a real `LinuxScreenCapture` backend, but was
      never added as a pipeline dependency nor wired into `platform.rs`'s
      `#[cfg(...)]` dispatch; this was a real, pre-existing gap (not a
      not-yet-landed backend) — fixed this session, verified via WSL2
      (`cargo build`/`test`/`clippy -p mediaway`, real Linux target)
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

### 7 — Streaming byte output — done (2026-09-18)

- [x] `EncodeSession::poll_bytes` / `finish_into`, with `finish()` demoted to a
      convenience wrapper — [ADR-0006](../adr/0006-encode-session-streaming-bytes.md).
      `EncodeSession` was the only non-streaming element in a streaming stack: the muxer
      underneath already exposed `Mux::poll_bytes`, which is why
      `examples/pipeline/screen_record.rs` had to drive `mp4::Muxer` directly. A long
      session's memory is now bounded by poll cadence, not by recording length.
- [ ] Migrate `examples/pipeline/screen_record.rs` onto `EncodeSession` now that the
      reason it bypassed the facade is gone (it also predates `open_with_audio`, so this
      is one migration, not two)

### 8 — Container choice — done (2026-09-18)

- [x] `EncodeSession<E, M: MuxOpen = mp4::Muxer<mp4::Open>>` + `open_in` /
      `open_in_with_audio` — [ADR-0007](../adr/0007-encode-session-generic-muxer.md). The
      facade was welded to fMP4 while `mediaway-container` shipped a WebM muxer of the same
      shape and `mediaway-ffi` already let a C caller pick between them; the layer above
      could not. Same shape of gap as stage 7: a convenience layer hiding a low-level
      capability rather than composing it.
- [x] `MuxOpen::FIRST_TRACK_ID` — track numbering is a container rule (Matroska reserves
      `TrackNumber` 0, encoders default to `id: 0`), found by the WebM round-trip test on
      its first run rather than reasoned out in advance.
- [ ] `mp4::Muxer`-only `set_track_extra_data` remains a real asymmetry: WebM writes
      `CodecPrivate` at `begin()`, so a late-config encoder backend (`VideoToolbox`) paired
      with WebM silently loses its configuration record. Documented on the trait method;
      a loud failure was rejected because the call is a legitimate no-op on MP4 too.
