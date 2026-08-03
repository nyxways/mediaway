# Audio track + `mediaway-audio-apm` integration on `EncodeSession`

`mediaway` ADR-0003 (crate-local) extends `EncodeSession` from
video-only/single-track to an optional second (audio) track, with
`mediaway-audio-apm`'s `AudioProcessor` (AEC3+NS+AGC2) and
`VoiceActivityDetector` (RNN VAD) wired in transparently. **Implemented** —
`src/session.rs` + `src/session_tests.rs`.

```text
caller → open_with_audio(video_encoder, audio_encoder)
              (both tracks registered before mp4::Muxer::begin() — typestate,
               tracks can't be added after)

caller → write_audio_frame(&AudioFrame)
              │
   no processor attached? ──yes──► audio_encoder.push_frame(frame)   (today's
              │no                                                     manual fast path)
              ▼
   processor.push_capture_frame(frame)
              │
   loop: processor.poll_processed_frame() → Some(block)?
              │yes                              │no → done
              ▼
   vad attached? → vad.analyze(&block) → push_back(score)   (best-effort;
              │                                               a disabled VAD
              ▼                                               just stops
   audio_encoder.push_frame(&block)                           scoring, does
                                                                NOT abort)
```

- **No second generic parameter.** `EncodeSession<E: VideoEncoder>` stays as-is;
  the audio side is `Option<AudioTrack>` holding a `Box<dyn AudioEncoder>` —
  same `Box<dyn Trait>`-over-second-generic reasoning as
  [frame-filter-hook](frame-filter-hook.md)'s `Box<dyn FrameFilter>`.
- **`open_with_audio`, not `add_audio_track` after `open`.** `mp4::Muxer` is
  typestate (`iso-bmff/src/mux/mod.rs`): tracks only add before `begin()`.
  Both tracks must therefore be known at construction time — a second
  constructor, not a builder.
- **Track-id collision, fixed explicitly.** Two independently constructed
  encoders both typically default to `stream_info().id == 0` — `open()`
  never hits this (single track), but `open_with_audio` renumbers explicitly
  (video `0`, audio `1`) via `StreamInfo::with_id` before registering, or
  `mp4::Muxer::add_track` rejects the duplicate with `Error::InvalidTrack`.
- **`AudioProcessor`'s push/poll absorbed inside one `write_audio_frame`
  call** — this is exactly the shape mismatch
  [audio/apm](../audio/apm.md) explains `FrameFilter` cannot express
  (N frames pushed, M ≠ N blocks produced). `write_frame` (video) still
  can't do this; `write_audio_frame` was never a filter-chain method, so
  nothing stops it.
- **VAD failure never blocks audio encoding.** Once a `VoiceActivityDetector`
  is disabled (caught backend panic), `analyze` errors forever — but
  `write_audio_frame` only stops pushing new scores onto the
  `poll_vad_score()` queue; the processed PCM still reaches the encoder. A
  degraded `AudioProcessor` similarly keeps producing (unenhanced) output
  rather than erroring the whole write.
- **`PipelineError` lost `Clone + PartialEq + Eq`** — `ApmError` wraps an
  external `sonora::Error` (`#[source]`, no `Clone`/`PartialEq` upstream) and
  can't honestly support them either. Checked before the change: nothing in
  the workspace compared or cloned a `PipelineError`.
- **Known gap, not fixed here:** a trailing audio block shorter than 10ms
  sitting in `AudioProcessor`'s internal buffer at `finish()` time is not
  flushed — `AudioProcessor` has no "flush a partial block" method today.
  Inherited from `mediaway-audio-apm`, named in ADR-0003 § `finish()`.

See also: [audio/apm](../audio/apm.md) (the crate this integrates),
[frame-filter-hook](frame-filter-hook.md) (the video-side precedent this
design deliberately diverges from for the push/poll reasons above),
[screen-record-av](screen-record-av.md) (the hand-rolled two-track pattern
this formalizes — not yet migrated onto `open_with_audio`, tracked in
`docs/roadmap.md` Stage 1b).
