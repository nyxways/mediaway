# ADR-0003: Audio track support on `EncodeSession`, with optional `mediaway-audio-apm` (AEC/NS/AGC + VAD) integration

- **Status**: Accepted
- **Date**: 2026-08-01
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway`

## Context

`EncodeSession<E: VideoEncoder>` is video-only, single-track (`src/session.rs`). Audio
(microphone capture + AAC encode) is composed manually today, outside `EncodeSession`,
directly against a second `mp4::Muxer` track — see
`tests/screen_mic_av_smoke.rs`. That test's own doc comment states the reason:
"`EncodeSession` stays video-only / single-track per ADR-0014 ('extend … when a real
caller needs it — new ADR at that point if the shape changes materially')". This ADR is
that point.

Separately, `mediaway-audio-apm` (echo cancellation AEC3, noise suppression NS, gain
control AGC2 via `AudioProcessor`; RNN voice-activity detection via
`VoiceActivityDetector`) already exists as a real, implemented, hardware-independent
crate — but its own ADR-0001 explicitly deferred pipeline wiring: "`mediaway`
may later *compose* `mediaway-audio-apm` once its Stage 1b (audio/multi-track
`EncodeSession`) is scoped — that is a separate, future ADR." This is that ADR. The
user's explicit ask driving this: audio track support exists specifically *so that*
`AudioProcessor`/`VoiceActivityDetector` become usable from this crate, not as a
standalone goal.

Two structural constraints from the existing code shape this design:

1. **`mp4::Muxer` is typestate** (`iso-bmff/src/mux/mod.rs`): `add_track` exists only
   on `Muxer<Open>`; `begin()` consumes it into `Muxer<Live>`, after which no more
   tracks can ever be added. `EncodeSession::open()` already calls `begin()`
   immediately. This means an audio track cannot be added to an already-`open()`'d
   session after the fact — it must be known at construction time.
2. **`AudioProcessor`/`VoiceActivityDetector` are deliberately push/poll, not a
   `FrameFilter`-parallel `AudioFilter` trait** (`mediaway-audio-apm/adr/0001` § 3): two
   input streams (render + capture) for AEC, and fixed-10ms-block re-blocking vs.
   arbitrary push sizes. That ADR's reasoning is not revisited here — this ADR wires the
   existing push/poll shape into `EncodeSession`, it does not change that shape.

## Decision

> Add a second, additive constructor (`EncodeSession::open_with_audio`) that registers
> both tracks before `begin()`; add `write_audio_frame`/`write_audio_render_frame` for
> the audio-side push path; add optional `attach_audio_processor`/`attach_vad` so
> `mediaway-audio-apm` sits transparently inside `write_audio_frame`, not as a caller
> concern.

### Struct shape — boxed audio slot, no second generic parameter

```rust
pub struct EncodeSession<E: VideoEncoder> {
    encoder: E,
    muxer: mp4::Muxer<mp4::Live>,
    track_id: u32,
    filters: SmallVec<[Box<dyn FrameFilter>; 4]>,
    audio: Option<AudioTrack>,                    // new
}

struct AudioTrack {
    encoder: Box<dyn AudioEncoder>,
    track_id: u32,
    processor: Option<AudioProcessor>,
    vad: Option<VoiceActivityDetector>,
    vad_scores: VecDeque<f32>,
}
```

`EncodeSession<E, A: AudioEncoder>` (a second generic parameter) was rejected for the
same reason `FrameFilter`'s own ADR-0001 rejected `EncodeSession<E, F>`: it would force
every video-only caller (still the common case) to also name an audio encoder type, or
thread a `NoAudio` sentinel through — a real ergonomic and call-site-propagation cost
for a feature most callers don't use. `Box<dyn AudioEncoder>` matches the exact
`Box<dyn FrameFilter>` precedent: one vtable dispatch per audio frame, not per sample,
bounded and acceptable (`zero-cost-abstractions.md`'s "facade plugin feature,
documented" case).

`AudioProcessor`/`VoiceActivityDetector` are stored **unboxed**, as concrete owned
fields — unlike `FrameFilter`, there is exactly one of each per session (not a
caller-composed chain of heterogeneous implementations), so there is nothing to type-erase.

### Construction — a second constructor, not a builder or a changed `open()`

```rust
impl<E: VideoEncoder> EncodeSession<E> {
    pub fn open(encoder: E) -> Result<Self, PipelineError>;                     // unchanged

    pub fn open_with_audio(
        encoder: E,
        audio_encoder: impl AudioEncoder + 'static,
    ) -> Result<Self, PipelineError>;                                          // new
}
```

Both tracks are registered on the still-`Open` muxer before one `begin()` call — video
first (`track_id` unchanged from today, `0` in the common case), audio second. This is
the only place a session can gain an audio track; there is no `add_audio_track` on an
already-open session, because the muxer typestate makes that impossible without a
materially bigger redesign (a lazy-`begin()` state machine) that no real caller has
asked for. A fluent builder (`.with_audio(...)` on `open`) was considered and rejected
for the same reason `FrameFilter`'s ADR-0001 already gave: `open` isn't a builder today,
nothing else in this crate uses fluent construction, and a second named constructor
covers the need with less new surface.

### Optional APM attachment — `Result`-returning, not silent

```rust
impl<E: VideoEncoder> EncodeSession<E> {
    pub fn attach_audio_processor(&mut self, processor: AudioProcessor)
        -> Result<&mut Self, PipelineError>;
    pub fn attach_vad(&mut self, vad: VoiceActivityDetector)
        -> Result<&mut Self, PipelineError>;
}
```

Both return `Err(PipelineError::NoAudioTrack)` when called on a session opened via
plain `open()` (no audio track to attach to) — an explicit error, not a silent no-op,
matching this crate's existing "no silent failure" norm (`FrameFilter`'s ADR-0001
§ Alternatives). Neither is required: `write_audio_frame` on a session with no
processor attached pushes frames straight to the audio encoder, identical to today's
manual pattern.

### The write path — `AudioProcessor`'s push/poll absorbed inside one call

```rust
impl<E: VideoEncoder> EncodeSession<E> {
    pub fn write_audio_frame(&mut self, frame: &AudioFrame) -> Result<(), PipelineError>;
    pub fn write_audio_render_frame(&mut self, frame: &AudioFrame) -> Result<(), PipelineError>;
    pub fn poll_vad_score(&mut self) -> Option<f32>;
}
```

`write_audio_frame`, when a processor is attached: `processor.push_capture_frame(frame)`,
then loops `processor.poll_processed_frame()` — each returned 10ms block is (a) scored
by `vad.analyze()` if a VAD is attached, pushing the score onto an internal
`VecDeque<f32>`, and (b) pushed into the audio encoder, packets drained into the muxer.
This is exactly where `AudioProcessor`'s N-frames-in/M-blocks-out mismatch gets
absorbed — `EncodeSession`'s own `write_frame` cannot do this for video (`FrameFilter`
is a synchronous 1:1 transform by design), but nothing forces `write_audio_frame` to
share that constraint, since it was never a filter-chain method to begin with.

With no processor attached, `write_audio_frame` pushes `frame` straight to the audio
encoder — the exact fast path `screen_mic_av_smoke.rs` already hand-rolls today, now
inside `EncodeSession`.

`write_audio_render_frame` feeds `AudioProcessor::push_render_frame` (the "what's about
to play" echo reference) when a processor is attached; a no-op `Ok(())` when an audio
track exists but no processor is attached (nothing to feed); `Err(NoAudioTrack)` with no
audio track at all. Only meaningful for a caller that is *also* playing audio back
(e.g. a voice-chat app) — a pure screen+mic recorder has no render stream and simply
never calls this.

`poll_vad_score` drains the `VecDeque<f32>` one score per processed 10ms block, mirroring
this crate's and `mediaway-encoder`'s existing push/poll idiom (`poll_packet`,
`poll_frame`) rather than returning scores inline from `write_audio_frame` (which could
produce zero, one, or several scores per call — an `Option<f32>` return from
`write_audio_frame` itself would be dishonest about that).

### Error handling on a disabled processor/VAD — matches `mediaway-audio-apm`'s own posture, does not re-litigate it

- `AudioProcessor`: once disabled (a caught `sonora` panic), `poll_processed_frame`
  **stops erroring** and instead returns the raw, unenhanced block forever — this is
  `mediaway-audio-apm` ADR-0001's own deliberate choice ("a disabled `AudioProcessor`
  producing further raw output is not wrong, just unenhanced"). `write_audio_frame`
  therefore only ever propagates a *transient* `ApmError` (the one panicking call, or a
  genuine `Backend`/format-mismatch error) via `PipelineError::Apm(#[from] ApmError)` —
  audio encoding is never permanently blocked by a degraded processor.
- `VoiceActivityDetector`: once disabled, `analyze` returns `Err` **forever**
  (`mediaway-audio-apm`'s own choice — no honest passthrough for a scalar score). This
  ADR does **not** propagate that error into `write_audio_frame`'s `Result`: a VAD
  failure means "no more voice-activity scores," not "the audio track is broken" — the
  processed PCM is still pushed to the encoder regardless. A caller who cares can still
  observe VAD health indirectly (`poll_vad_score` simply stops producing new scores);
  a dedicated `is_vad_disabled()` query was considered and deferred (§ Alternatives) —
  add it if a real caller needs to distinguish "no speech detected" from "VAD stopped
  working" without inference.

### `finish()` — flush both tracks

`finish` gains: if an audio track exists, flush the audio encoder and drain its
remaining packets into the muxer, before the existing muxer-flush/byte-poll tail.
**Known, inherited limitation, not fixed here**: `AudioProcessor` has no "flush a
partial (<10ms) trailing block" method in its current public API
(`mediaway-audio-apm/src/processor.rs`) — up to ~10ms of audio pushed just before
`finish()` can be silently dropped if it never reaches a full block. This is an existing
gap in `mediaway-audio-apm` itself, not introduced by this ADR; named here per
`caveats-and-clarity.md` rather than silently inherited. Revisit in a
`mediaway-audio-apm` ADR if a real caller hits it.

### Dependency

`mediaway` adds `mediaway-audio-apm = { workspace = true, features = ["full"] }`
**unconditionally** (not behind a new Cargo feature on this crate) — this crate is
already "batteries included" (unconditional `mediaway-encoder`/`mediaway-container`/
`mediaway-device-*` deps, no granular feature gating), unlike `mediaway-device-ffi`
(ADR-0004 there), whose per-feature code-exclusion goal is about a *shipped native
binary's* size for downstream language bindings — a concern that does not apply to a
Rust-only convenience facade.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `EncodeSession<E, A: AudioEncoder>` (second generic) | Forces a `NoAudio` sentinel and propagates a second type parameter to every video-only call site — same rejection as `FrameFilter`'s ADR-0001 gave for `EncodeSession<E, F>` |
| `add_audio_track` on an already-`open()`'d session | Impossible without changing `mp4::Muxer`'s typestate (`add_track` only exists pre-`begin()`) — a materially bigger redesign no real caller has asked for |
| `AudioFilter` trait wrapping `AudioProcessor`, mirroring `FrameFilter` | Already rejected, with reasons, by `mediaway-audio-apm/adr/0001` § 3 — not re-litigated here |
| `write_audio_frame` returns produced VAD scores inline (`Result<SmallVec<[f32; 2]>, _>`) | A push call producing zero, one, or several scores is exactly the push/poll mismatch this crate already solves with a poll method elsewhere (`poll_packet`); a separate `poll_vad_score` is the honest, idiom-consistent shape |
| Propagate every VAD `analyze` error through `write_audio_frame`'s `Result` | Would make a disabled VAD abort audio encoding entirely — worse than `mediaway-audio-apm`'s own "degrade the enhancement, don't kill the stream" posture for the *processor*; VAD is a supplementary signal, not the audio track's data itself |
| A `.with_audio(...)`/`.with_apm(...)` fluent builder | `open` isn't a builder today; no other method here uses fluent construction (`FrameFilter` ADR-0001 already made this call for `push_filter`) |
| `is_vad_disabled()` query now | No real caller need identified yet; `poll_vad_score` silently stopping is enough for v1 — add later if a caller needs to distinguish "no speech" from "VAD broke" (`zero-cost-abstractions.md`'s "no abstractions for one-off code") |

## Consequences

### Positive

- `mediaway-audio-apm`'s `AudioProcessor`/`VoiceActivityDetector` become usable from
  `mediaway` with zero caller-side push/poll bookkeeping — `write_audio_frame`
  absorbs it.
- Fully additive: `open`/`write_frame`/`push_filter` signatures unchanged; existing
  video-only callers see no behavior change.
- The existing manual two-track pattern (`screen_mic_av_smoke.rs`) becomes expressible
  through `EncodeSession` directly — a future revision of that test (or a new one) can
  drop its hand-rolled second-track muxing.

### Negative / Trade-offs

- `open_with_audio` duplicates most of `open`'s body (mp4 track registration) — a small,
  accepted amount of repetition rather than a shared private helper that would need its
  own `Option<impl AudioEncoder>` parameter (defeating the "no second generic" goal).
- A trailing <10ms audio block can be silently lost at `finish()` — inherited from
  `mediaway-audio-apm`, named above, not fixed by this ADR.
- `PipelineError` loses its `Clone + PartialEq + Eq` derive (drops to `Debug + Error`
  only): `mediaway_audio_apm::ApmError` wraps `sonora::Error` (`#[source]`, no
  `Clone`/`PartialEq` on the external type) and cannot honestly support them either.
  Checked: no in-workspace caller compares or clones a `PipelineError` today (both
  `mediaway`'s own tests and `mediaway-ffi`'s `From<PipelineError>`
  conversion only pattern-match by value) — a real but so-far-inert breaking change to
  this type's trait surface.

## References

- `mediaway-audio-apm/adr/0001-sonora-audio-processing-adoption.md` — `AudioProcessor`/
  `VoiceActivityDetector` shape, panic-safety posture; explicitly defers this
  integration to "a separate, future ADR" (this one).
- `adr/0001-frame-filter-hook.md` (this crate) — `Box<dyn Trait>` over a second generic
  parameter precedent; "no silent failure" norm; "no fluent builder" precedent.
- `docs/adr/0014-pipeline-convenience-crate.md` — names this exact deferral ("new ADR if
  `EncodeSession`'s shape changes materially").
- `tests/screen_mic_av_smoke.rs` — today's hand-rolled two-track pattern this ADR
  formalizes into `EncodeSession`.
- `iso-bmff/src/mux/mod.rs` — `Muxer<Open>`/`Muxer<Live>` typestate, the structural
  reason audio must be declared at construction time.
- `docs/spec/zero-cost-abstractions.md` — `Box<dyn Trait>` "facade plugin feature"
  precedent.
- `docs/spec/caveats-and-clarity.md` — the trailing-partial-block gap is named, not
  silently inherited.

ADRs are written in **English**.
