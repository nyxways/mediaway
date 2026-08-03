# ADR-0001: Adopt `sonora` for audio enhancement; `AudioProcessor` / `VoiceActivityDetector` shape

- **Status**: Accepted
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-audio-apm`

**2026-07-31 addendum**: `sonora`/`sonora-agc2`/`sonora-simd = "0.2"` added to
`[workspace.dependencies]`, gated behind this crate's `apm`/`vad` features. `cargo deny
check` run for real against the resolved graph — clean, no new exceptions needed.
Implementation may proceed.

## Context

No crate in this workspace does echo cancellation, noise suppression, gain
control, or voice-activity detection today (confirmed by search across
`crates/` for AEC/echo/noise-suppress/VAD/gain-control — the only hits are an
unrelated `mfxstructures.h` vendor header and encoder-ADR prose). This is a
genuinely new **capability**, not an extension of an existing one.

`sonora` (+ `sonora-agc2`, `sonora-simd`) is a pure-Rust, SIMD-accelerated
(SSE2/AVX2 on x86_64, NEON on ARM64, scalar fallback) port of Google's
libwebrtc `AudioProcessing` module (WebRTC M145 lineage, via PulseAudio's
earlier C++ extraction), validated against 2400+ ported reference tests.
Real, field-tested usage exists in a sibling personal project,
`nyxie_voice` (`E:\51P\Project-Eddy\Native`, a Unity voice-chat native
library — **not** part of this workspace), whose `src/apm.rs` wraps:

- `sonora::AudioProcessing` — builder (`Config{echo_canceller,
  noise_suppression, gain_controller2, ..}` + `capture_config`/
  `render_config` `StreamConfig`) → `process_render_f32`/`process_capture_f32`
  on 10ms deinterleaved frames.
- `sonora_agc2::vad_wrapper::VoiceActivityDetectorWrapper` — RNN VAD,
  constructed with `sonora_simd::detect_backend()`.

Two real operational lessons from that project carry directly into this
design (both cited by name below): (1) every `build`/`process` call is
wrapped in `catch_unwind`, permanently disabling the APM (raw passthrough) on
any panic, because Sonora is young (0.1 at the time, 0.2 as of this ADR) and
its build/process panic surface is not something that project's author could
assert closed; (2) the RNN VAD assumes i16-scale (±32768) PCM internally — an
f32 `[-1, 1]` signal fed unscaled reads as permanent silence.

Two precedents from this workspace ground the shape decisions:

- `mediaway` ADR-0001 (`FrameFilter`) — the just-shipped mid-pipeline
  hook for **video**: a trait, an ordered `SmallVec<[Box<dyn FrameFilter>; 4]>`
  chain, `process(&mut self, frame: VideoFrame) -> Result<VideoFrame,
  FilterError>` — one input stream, one output frame, synchronous 1:1.
- `docs/adr/0005-gpu-interop.md` / `docs/spec/gpu-interop.md` — "native
  handles remain source of truth… adapters wrap or export them… not a custom
  [abstraction] API instead of interop [that] duplicates \[the upstream
  library\]". `mediaway-wgpu` is the concrete precedent: **one crate**, no
  platform split, thin adapter over one external framework, no invented
  abstraction layer on top of it.

## Decision

> Add a new facade crate, **`mediaway-audio-apm`**, holding two independent,
> concrete (not trait-based) types — `AudioProcessor` (AEC3+NS+AGC2, `apm`
> feature) and `VoiceActivityDetector` (RNN VAD, `vad` feature) — as thin,
> Mediaway-`AudioFrame`-typed adapters directly over `sonora` /
> `sonora-agc2` / `sonora-simd`'s real APIs. **Not** a `FrameFilter`-parallel
> `AudioFilter` trait/chain — the two-stream render/capture split and the
> fixed-10ms-block-vs-arbitrary-input-size mismatch are real, structural
> differences from a single-stream video filter, not cosmetic ones. Callers
> wire it directly between `mediaway-device`'s `AudioCapture::poll_frame()`
> output and whatever consumes the signal next (an `AudioEncoder`, a VAD
> gate, app logic) — **not** through `mediaway::EncodeSession`,
> which has no audio track support today (Stage 1b, "not yet scoped").
> Adopt an explicit catch-and-disable panic-safety posture for the `sonora`/
> `sonora-agc2` call sites specifically, deliberately narrower and less
> silent than nyxie_voice's own shape.

### 1. License verdict

Confirmed via the crates.io API (not assumed) for all three crates:

| Crate | License | Latest | Repo |
|-------|---------|--------|------|
| `sonora` | BSD-3-Clause | 0.2.0 | `github.com/dignifiedquire/sonora` |
| `sonora-agc2` | BSD-3-Clause | 0.2.0 | same |
| `sonora-simd` | BSD-3-Clause | 0.2.0 | same |

BSD-3-Clause is **already** on `deny.toml`'s `[licenses] allow` list — no new
exception entry needed, `cargo deny check licenses` passes unmodified once
these land as real dependencies. No GPL/LGPL/AGPL/SSPL/BUSL anywhere in this
edge; the crate is a from-scratch Rust port (not a `bindgen`/FFI wrapper
around WebRTC's C++), so there is no `libwebrtc` binary or source vendored
into the Cargo graph — it is a **derivation**, tracked upstream as
Google libwebrtc → PulseAudio extraction → Sonora Rust port, same BSD-3
lineage throughout per the repo's own attribution footer.

`sonora-ffi` / `sonora-sys` (a separate crate in the same repo, used to
validate against the C++ reference suite) is **not** a dependency edge we
take — we depend only on `sonora` / `sonora-agc2` / `sonora-simd`'s safe
public Rust API. `sonora-simd` almost certainly uses `unsafe` internally for
its own architecture intrinsics, but that unsafe is entirely inside
`sonora-simd`, not exposed to or re-implemented by us — `mediaway-audio-apm`
itself needs **zero** `unsafe` and keeps `#![forbid(unsafe_code)]`, matching
`mediaway-common`/`mediaway-device`/`mediaway-wgpu`.

**Maintenance nuance, not a license concern:** the *algorithm* (WebRTC
AudioProcessing) is a mature, extremely battle-tested C++ codebase used by
essentially every Chromium-based call product; the *Rust port* is young
(0.1→0.2 within this same review, 221 commits, 59 stars, small
single-maintainer repo). That gap — mature algorithm, young transpilation —
is exactly why § Panic-safety posture below treats this dependency
differently from, say, `wgpu` or `vulkanalia`, both far larger and more
widely consumed. Per `deps-policy.md`, pin through minor: `"0.2"`, not
nyxie_voice's `"0.1"` (latest stable, not a mirror of the sibling project's
older pin).

### 2. Crate placement — new facade, no platform split

`mediaway-audio-apm` (facade, `mediaway-`-prefixed per ADR-0012 — it depends
on `mediaway_common::AudioFrame`, so it is not a freestanding unprefixed
core). **Not** folded into an existing crate:

- **`mediaway-common`** — shared-types crate, currently only depends on
  `bytes`. Adding `sonora` here would force a SIMD audio-DSP dependency onto
  every consumer of any Mediaway type (container, encoder, decoder, device,
  pipeline), wildly disproportionate to what `mediaway-common` is for.
- **`mediaway-device`** — this facade's job is OS capture/playback
  **sessions** (`AudioCapture`, `AudioPlayback`, capability probing);
  `AudioProcessor` is a pure CPU buffer transform with no OS handle at all —
  identical behavior on every platform, since `sonora`'s SIMD backend
  selection is internal/runtime, not per-OS the way WASAPI/PipeWire/WebAudio
  are. Folding it in would (a) force `sonora` onto every caller who only
  wants raw capture, and (b) mirror the exact anti-pattern
  `crate-packaging.md` names for WMF/WebCodecs: "do not fold \[X\] into the
  facade as `cfg` modules" — same reasoning applies to folding a whole new,
  externally-dependency-heavy capability into an unrelated facade.
- **`mediaway`** — its own roadmap header states it is a
  "facade-of-facades… composition only, no traits of its own." This crate
  needs to *own* a new low-level type surface (`AudioProcessor`,
  `VoiceActivityDetector`, `ApmError`), which contradicts that crate's
  stated shape. `mediaway` may later *compose* `mediaway-audio-apm`
  once its Stage 1b (audio/multi-track `EncodeSession`) is scoped — that is
  a separate, future ADR.

**No platform split** (`mediaway-audio-apm-windows`, …): unlike
`mediaway-encoder`/`mediaway-decoder`/`mediaway-device`, there is no OS API
here to abstract over. `sonora` runs identically on every target Cargo
compiles it for. The correct structural precedent is **`mediaway-wgpu`**:
one crate, one external framework, no per-platform siblings, "thin adapter,
not a custom abstraction" (`gpu-interop.md`).

### 3. API shape — render/capture push/poll, not an `AudioFilter`/`FrameFilter` parallel

The task's own framing asks this directly, and the honest answer is **no,
this does not mirror `FrameFilter`**, for two structural reasons, not one:

**(a) Two input streams, not one.** `sonora::AudioProcessing::process_render_f32`
feeds an internal echo-reference buffer; `process_capture_f32` consumes both
that buffer *and* the near-end mic signal to produce cleaned output.
`FrameFilter::process(&mut self, frame: VideoFrame) -> Result<VideoFrame,
FilterError>` has exactly one input, one output, no side channel. There is
no honest way to express "this filter also needs a second stream, fed out of
band and time-aligned via `set_stream_delay_ms`" through that one-method
trait signature without either a hidden side-channel setter (defeating the
"just a filter in a chain" model `FrameFilter` exists to provide) or a
tuple/enum frame type that every *other* filter in a hypothetical chain
would also have to understand and ignore. Forcing this shape into
`FrameFilter` would not be a smaller version of the real API — like the
`FrameFilter` ADR's own rejection of a generic `EncodeSession<E, F>`, it
would be "a different, unusable-for-this-purpose feature."

**(b) Fixed 10ms internal blocks vs. arbitrary-length real capture frames.**
`sonora` processes exactly `sample_rate / 100` samples per call
(`APM_FRAME_SAMPLES = 480` at 48kHz in nyxie_voice). Real capture frames
(WASAPI callback periods, `cpal` buffers) are **not** guaranteed to be
exactly 10ms — `mediaway-device`'s `AudioCapture::poll_frame()` already
returns `Option<AudioFrame>` precisely because "ready" and "frame boundary"
are not the same event. A synchronous `process(frame) -> frame` transform
(what `FrameFilter` does, because video frame boundaries and pixel counts
*are* stable 1:1 through a filter) would therefore be dishonest here: it
cannot promise "one call in, one call out" without either silently dropping
a partial block or silently padding one, both forbidden by
`caveats-and-clarity.md`. The type must accumulate/re-block internally and
expose **push/poll**, mirroring this codebase's own existing idiom for
exactly this shape mismatch (`VideoEncoder::push_frame`/`poll_packet`,
`AudioEncoder::push_frame`/`poll_packet`, `AudioCapture::poll_frame`).

```rust
/// Sample format / rate / channel layout for one side of an [`AudioProcessor`].
/// `sample_format` must be `SampleFormat::F32` — sonora processes f32 only
/// (matches `AudioCaptureConfig::microphone()`'s existing default).
pub struct AudioStreamFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: SampleFormat,
}

/// Echo cancellation (AEC3) + noise suppression (NS) + gain control (AGC2),
/// via `sonora::AudioProcessing`. Not Zero-Copy (see § Costly path).
pub struct AudioProcessor { /* … */ }

impl AudioProcessor {
    /// # Errors
    /// [`ApmError::UnsupportedSampleFormat`] if either format's
    /// `sample_format != SampleFormat::F32`.
    pub fn open(
        config: ApmConfig,
        capture_format: AudioStreamFormat,
        render_format: AudioStreamFormat,
    ) -> Result<Self, ApmError>;

    /// Feed a render-reference (far-end / about-to-be-played) frame. No
    /// output — this only updates the internal echo reference sonora's
    /// capture path later consumes. No-op once [`is_disabled`](Self::is_disabled).
    pub fn push_render_frame(&mut self, frame: &AudioFrame) -> Result<(), ApmError>;

    /// Feed a near-end (mic) capture frame. Buffered internally into 10ms
    /// blocks — does not synchronously return output (see [`poll_processed_frame`](Self::poll_processed_frame)).
    pub fn push_capture_frame(&mut self, frame: &AudioFrame) -> Result<(), ApmError>;

    /// Pull the next processed 10ms capture block, if a full block is ready.
    /// After a caught backend panic, returns the **unmodified** input block
    /// (documented passthrough — see § Panic-safety posture), never silently
    /// disguised as a normally processed one from the caller's point of view:
    /// check [`is_disabled`](Self::is_disabled).
    pub fn poll_processed_frame(&mut self) -> Result<Option<AudioFrame>, ApmError>;

    /// Estimated render→capture round-trip delay (echo-alignment hint).
    pub fn set_stream_delay_ms(&mut self, ms: i32);

    /// `true` after a caught backend panic — this instance now passes frames
    /// through unmodified for its remaining lifetime. Construct a new
    /// instance to retry.
    #[must_use]
    pub fn is_disabled(&self) -> bool;
}

/// Standalone RNN voice-activity detector, via `sonora_agc2::vad_wrapper`.
/// Independent of [`AudioProcessor`] — sonora's own AGC2 uses this VAD
/// internally but does not expose it, so this type wraps the standalone
/// `sonora-agc2` crate directly, not `AudioProcessor`'s internals.
pub struct VoiceActivityDetector { /* … */ }

impl VoiceActivityDetector {
    pub fn open(sample_rate: u32) -> Result<Self, ApmError>;

    /// Speech probability in `[0, 1]` for one 10ms frame. **Intended input
    /// is [`AudioProcessor::poll_processed_frame`]'s output** — already
    /// exactly-10ms-blocked and post-NS, matching sonora's own validated
    /// usage pattern (AGC2's internal VAD consumes post-NS audio). Calling
    /// this on arbitrary-length raw capture frames is not supported — no
    /// internal re-blocking ring buffer here, by design (see § Alternatives).
    ///
    /// # Sonora i16-scale caveat
    /// See § VAD scaling gotcha — `frame` is scaled ×32768.0 internally
    /// before reaching sonora; do not pre-scale it yourself.
    pub fn analyze(&mut self, frame: &AudioFrame) -> Result<f32, ApmError>;

    #[must_use]
    pub fn is_disabled(&self) -> bool;
}
```

`push_capture_frame`/`poll_processed_frame` as two calls (not one
`process_capture(&mut self, frame) -> Result<AudioFrame, ApmError>`) is the
direct, honest consequence of (b) above — an input push does not
necessarily produce output on the same call, so collapsing them into one
signature would either lie about that (silently buffering more than the
caller can see) or force every caller to loop-and-retry a "not ready" error
that is not actually an error.

`AudioFrame` — not raw `&[f32]` slices like `sonora` itself — because it is
already `mediaway-device`'s `AudioCapture::poll_frame()` output type and
`mediaway-encoder`'s `AudioEncoder::push_frame()` input type; taking/
returning it here means this crate is a drop-in stage between those two
existing traits with zero extra glue code at the call site, matching
`VideoFrame`'s role for `FrameFilter`. Unlike `VideoFrame`, `AudioFrame` has
**no** `Gpu`-backed storage variant (`data: Bytes`, always CPU) — the
`FrameFilter` ADR's "`Gpu`-backed frame + non-empty chain fails loudly"
concern simply does not exist for audio; every `AudioFrame` is already
CPU-resident, so there is no equivalent scope cut to make here.

### 4. Panic-safety posture — catch-and-disable, narrower and less silent than nyxie_voice's

**Decision: yes, adopt an explicit `catch_unwind`-based disable pattern**,
scoped only to the actual `sonora`/`sonora-agc2` call sites (`build`,
`process_render_f32`, `process_capture_f32`, `analyze`) — not a blanket
policy for this crate's own code.

This workspace's `unwrap_used = deny` / `expect_used = deny` / `panic =
deny` clippy lints, and `error-handling.md`'s `Result`-only public-error
policy, govern **our own** code. They cannot prevent a *third-party
dependency's* internal panic — and per § 1's maintenance nuance, `sonora` is
specifically a young Rust port of a large C++ codebase, a real (not
hypothetical) class of risk for an edge-case input triggering a
transpilation bug that surfaces as a Rust panic rather than a clean error
return.

This workspace already has a mechanically identical pattern for exactly this
problem: `mediaway-ffi` ADR-0001 §7 and `mediaway-container-ffi`
ADR-0001 §7 both wrap every exported call in
`catch_unwind(AssertUnwindSafe(...))` and set a `poisoned: bool` field on
catch, after which further calls short-circuit
(`HANDLE_POISONED`/`MEDIAWAY_*_STATUS_HANDLE_POISONED`). `mediaway-audio-apm`
reuses the same `catch_unwind(AssertUnwindSafe(...))` mechanism — but
**deliberately diverges on what happens after the catch**:

| | FFI poisoned-handle precedent | `AudioProcessor`/`VoiceActivityDetector` |
|-|-------------------------------|-------------------------------------------|
| Root cause | Rust 1.71+: an unhandled panic unwinding out of a plain `extern "C" fn` **aborts the whole host process** (defined, not UB, but still unacceptable for an embedded library) | Pure Rust boundary: an uncaught panic on default `panic = "unwind"` only tears down the *calling thread* — but that thread is realistically a real-time audio callback/worker thread, and many OS audio hosts (WASAPI, CoreAudio) do not tolerate a callback thread vanishing predictably |
| After a caught panic | Every subsequent call **errors** (`HANDLE_POISONED`) — an opaque handle's internal state after a mid-mutation panic is not verifiably safe to keep using | Every subsequent call **passes audio through unmodified** — the underlying signal (unprocessed PCM) is still perfectly usable; only the *enhancement* is lost |
| Why the difference is correct, not an oversight | A corrupted `Muxer`/`EncodeSession` producing further output would be silently wrong (bad MP4 bytes) — erroring is the only safe option | A disabled `AudioProcessor` producing further **raw** output is not wrong, just unenhanced — silently *stopping the audio stream* (by erroring forever) is a strictly worse outcome for a live capture/voice pipeline than continuing without AEC/NS/AGC |

This also deliberately departs from `mediaway` ADR-0001
(`FrameFilter`)'s "no silent failure… this crate has no precedent for a soft
fail and continue path" stance — named explicitly as a **considered, not
accidental** divergence: `EncodeSession::write_frame` fails a batch/file
encode job the caller can cleanly abort and retry; `AudioProcessor` sits
inside a live, real-time capture loop where "abort the whole session" has no
clean retry story and a materially worse failure mode (dead mic pipeline vs.
degraded-quality mic pipeline).

**Not fully silent**, unlike nyxie_voice's own `eprintln!`-based version —
this workspace denies `print_stderr`/`print_stdout` (clippy) and
`caveats-and-clarity.md` forbids silent slow/degraded defaults:

- The triggering call returns `Err(ApmError::BackendPanicked)` exactly once
  (loud, typed, matches this crate's `Result`-first error convention).
- `is_disabled(&self) -> bool` is a public, documented query — a caller can
  poll it, log it via their own logging stack (this crate adds none), or
  surface it in telemetry/UI.
- Rustdoc on `poll_processed_frame`/`analyze` states the passthrough
  contract explicitly, not just in this ADR — per `caveats-and-clarity.md`,
  "code carries the contract."

No `unsafe` is required — `catch_unwind`/`AssertUnwindSafe` are safe Rust,
already used at 3+ existing FFI call sites in this workspace
(`mediaway-ffi`, `mediaway-container-ffi`, `mediaway-device-ffi`),
so this is not a novel Rust idiom for this codebase even though those are
`extern "C"` boundaries and this crate is not. `mediaway-audio-apm` keeps
`#![forbid(unsafe_code)]`.

### 5. VAD scaling gotcha — documented, not rediscovered

`sonora-agc2`'s RNN VAD is a port of WebRTC's internal detector, which
assumes **i16-scale** PCM (`±32768`) for its spectral-energy silence
threshold. `mediaway_common::AudioFrame`'s realistic mic-capture content is
f32 normalized to `[-1, 1]` (matches `AudioCaptureConfig::microphone()`'s
`SampleFormat::F32` default). Feeding `[-1, 1]` samples unscaled means
spectral energy never crosses the detector's internal `SILENCE_THRESHOLD`
(`0.04`) — `analyze()` returns `0.0` for *every* input, including real
speech, which looks exactly like "VAD is broken" rather than "VAD needs
i16-scale input."

nyxie_voice's own `vad_amplitude_sweep_finds_silence_boundary` test
empirically confirms the boundary: silence-classified up to amplitude
`~1.0`, real (non-silence) detection from amplitude `~10.0` and up.
`VoiceActivityDetector::analyze` therefore applies a `×32768.0` scale
internally before calling `sonora`'s `analyze` — the caller passes ordinary
`[-1, 1]` `AudioFrame` content and never sees or manages the scale factor.
This is stated in rustdoc on `analyze` (§ 3 above) and restated here per
`caveats-and-clarity.md`'s "no silent footgun" rule — a future implementer
must not rediscover this by shipping a VAD that always reports silence.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `AudioFilter` trait mirroring `FrameFilter` exactly | Cannot express the render/capture two-stream split without a side-channel setter or a shared tuple frame type every chain member would need to understand; cannot express fixed-10ms-block re-blocking through a synchronous 1:1 `process()` without lying about "one call in, one call out" |
| Fold into `mediaway-device` as a processing option on `AudioCapture` | Forces `sonora` onto every caller who only wants raw capture; mixes an OS-session facade with a pure CPU DSP transform that has no OS handle at all — same anti-pattern `crate-packaging.md` names for folding WMF/WebCodecs into a facade as `cfg` modules |
| Fold into `mediaway-common` | Would force a SIMD audio-DSP dependency onto every consumer of any Mediaway type; `mediaway-common` today depends on `bytes` only |
| Fold into `mediaway` | That crate's own roadmap states "composition only, no traits of its own" — this needs to own a new low-level type surface |
| `AudioProcessor` as a trait now, for a hypothetical second backend (e.g. WASAPI Voice Processing) | No second real backend exists today; an OS-native AEC path is a `mediaway-device-windows` **capture-level** concern (a different capability), not a second implementation of this crate's contract. Speculative trait abstraction for zero current callers — revisit via a new ADR if/when a second real backend caller appears, same "not adding it now is no abstractions for one-off code, not a permanent rejection" reasoning `FrameFilter`'s ADR already used for a bare-closure blanket impl |
| Mirror nyxie_voice's fully silent `eprintln!` + `None`-forever fallback verbatim | This workspace denies `print_stdout`/`print_stderr` (clippy) and forbids silent degraded defaults (`caveats-and-clarity.md`); adapted instead to a typed one-shot error + queryable `is_disabled()` |
| Poisoned-handle "error forever" after a caught panic (mirroring the `*-ffi` precedent exactly) | Correct for an opaque handle whose corrupted internal state cannot safely produce further output; wrong here — the underlying raw PCM is still perfectly valid, so erroring forever needlessly kills a live audio stream instead of degrading it |
| `VoiceActivityDetector` with its own internal re-blocking ring buffer (accepts arbitrary-length input like `push_capture_frame` does) | Would duplicate `AudioProcessor`'s re-blocking logic for no real benefit — VAD's own validated usage pattern (per sonora/WebRTC) is to run on already-NS-processed, already-10ms-blocked audio, which `AudioProcessor::poll_processed_frame` already produces for free |
| `sample_format` conversion (S16/S32 → F32) inside `AudioProcessor::open`/push methods | Silent format coercion on a hot path is exactly what `caveats-and-clarity.md` forbids without an honest name; `AudioCaptureConfig::microphone()` already defaults to F32, so the common real path needs no conversion — S16/S32 callers get a named, explicit `ApmError::UnsupportedSampleFormat` instead |

## Consequences

### Positive

- Fills a real, previously-absent capability (audio enhancement) with a
  permissively licensed, pure-Rust, no-FFmpeg dependency, consistent with
  the workspace's license boundary.
- `AudioProcessor`/`VoiceActivityDetector` are usable standalone, satisfying
  `api-layers.md`'s "no opaque-only path" — no forced dependency on
  `mediaway` or `EncodeSession` to use enhancement at all.
- Panic-safety posture keeps a live capture/voice pipeline alive through a
  young dependency's internal fault, without hiding that fault from the
  caller (one typed error + a queryable flag, not silence).
- Feature-gated (`apm`/`vad` independently optional) keeps slim builds slim
  — a caller wanting only VAD does not pull AEC3/NS/AGC2's fuller `sonora`
  dependency, and vice versa.

### Negative / Trade-offs

- Not Zero-Copy (CPU ⚡) on the hot path: `sonora`'s API takes separate
  src/dst slices, and re-blocking arbitrary input into fixed 10ms blocks
  requires a real payload copy on both push and poll — this crate's future
  rustdoc/README must never claim ⚡ here, and a `caveats-and-clarity.md`
  catalog row is owed once implemented (tracked in `docs/roadmap.md`
  Stage 1).
- Two independent, unrelated-looking public types (`AudioProcessor`,
  `VoiceActivityDetector`) in one crate rather than one unified type — a
  deliberate reflection of `sonora`'s own architecture (VAD is genuinely
  standalone, not exposed through `AudioProcessing`), not a simplification
  opportunity missed.
- No trait abstraction today means a second APM backend later (if one ever
  becomes real) requires a breaking API change or an additive trait
  extraction — accepted per `zero-cost-abstractions.md`'s "no abstractions
  for one-off code," revisited only when a second real backend exists.
- Depends on a young (0.1→0.2 within this same review), single-maintainer
  upstream crate — mitigated, not eliminated, by the panic-safety posture;
  correctness bugs that do *not* panic (wrong output, not garbage output)
  are not caught by this design and would need dedicated regression/oracle
  tests once implemented.

## References

- `E:\51P\Project-Eddy\Native\Cargo.toml`, `src/apm.rs` — real, shipped usage
  (sibling personal project, not part of this workspace)
- [`mediaway/adr/0001-frame-filter-hook.md`](../../mediaway/adr/0001-frame-filter-hook.md) —
  `FrameFilter` shape and "no silent failure" precedent, both explicitly
  diverged from here with reasons
- [`docs/adr/0005-gpu-interop.md`](../../../docs/adr/0005-gpu-interop.md),
  [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md) — thin
  adapter, not a custom abstraction
- [`mediaway-wgpu`](../../mediaway-wgpu) — structural precedent: one crate,
  no platform split, external framework adapter
- [`mediaway-ffi/adr/0001-auto-encode-c-abi.md`](../../mediaway-ffi/adr/0001-auto-encode-c-abi.md) §7,
  [`mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md`](../../mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md) §7 —
  `catch_unwind`/poisoned-handle precedent, diverged from on post-catch
  behavior (§ Panic-safety posture)
- [`mediaway-common/src/frame.rs`](../../mediaway-common/src/frame.rs) —
  `AudioFrame` real field shape
- [`mediaway-device/src/audio.rs`](../../mediaway-device/src/audio.rs) —
  `AudioCapture`, `AudioCaptureConfig::microphone()`'s F32 default
- [`mediaway-encoder/src/audio.rs`](../../mediaway-encoder/src/audio.rs) —
  `AudioEncoder::push_frame` (this crate's typical downstream consumer)
- [`docs/spec/sans-io.md`](../../../docs/spec/sans-io.md),
  [`docs/spec/api-layers.md`](../../../docs/spec/api-layers.md) — no device
  I/O in this crate; low-level types stay public and directly usable
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) —
  not-Zero-Copy honesty, no silent degraded defaults
- [`docs/spec/zero-cost-abstractions.md`](../../../docs/spec/zero-cost-abstractions.md) —
  no speculative trait abstraction for a single backend
- [`docs/conventions/error-handling.md`](../../../docs/conventions/error-handling.md) —
  `thiserror` shape, specific variants over `Other(String)`
- [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md),
  [`docs/conventions/security.md`](../../../docs/conventions/security.md) —
  BSD-3-Clause already allow-listed in `deny.toml`
- [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md),
  [`docs/adr/0012-unprefixed-reusable-cores.md`](../../../docs/adr/0012-unprefixed-reusable-cores.md) —
  facade naming, why this is `mediaway-`-prefixed

ADRs are written in **English**.
