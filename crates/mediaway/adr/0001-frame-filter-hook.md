# ADR-0001: Mid-pipeline frame filter hook on `EncodeSession`

- **Status**: Accepted — implemented 2026-07-31, see `src/filter.rs` + `src/session.rs`
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway`

## Context

`EncodeSession::write_frame` ([`src/session.rs`](../src/session.rs)) currently does exactly
two things with an incoming `&VideoFrame`: `self.encoder.push_frame(frame)`, then drain
packets into the muxer. There is no processing step between "caller hands us a frame" and
"encoder consumes it" — a caller who wants to transform frames in flight (crop, color
correction, a watermark overlay, a simple temporal blend) has to hand-roll their own loop
around the low-level `VideoEncoder` trait instead, which is exactly the boilerplate this
crate exists to remove (ADR-0014).

A bigger idea — a GPU shader-pass / render-graph framework sitting at this same seam —
was discussed and **intentionally deferred** by the user in this design conversation (see
[Deferred](#deferred)). This ADR is scoped to a small, plain-Rust hook only.

`VideoFrame` ([`mediaway-common/src/frame.rs`](../../mediaway-common/src/frame.rs)) carries
`storage: VideoFrameStorage`, which is either:

- `Cpu { data: Bytes }` — `Bytes::clone()` is an `Arc`-style refcount bump, not a pixel
  memcpy.
- `Gpu(GpuBufferHandle)` — `GpuBufferHandle` is `#[derive(Clone, Copy, …)]`
  ([`mediaway-common/src/gpu.rs`](../../mediaway-common/src/gpu.rs)): a small opaque
  handle, not a texture copy.

So `VideoFrame::clone()` is always cheap regardless of which variant it holds — but a
**plain Rust closure/trait cannot meaningfully transform pixels sitting in a GPU texture**
without either a compute-shader mechanism (the deferred idea) or a CPU readback (silently
breaking Zero-Copy, forbidden by `caveats-and-clarity.md`). v1 must therefore make an
honest, explicit scope call on `Gpu`-backed frames rather than pretend to support them.

`VideoEncoder` ([`mediaway-encoder/src/video.rs`](../../mediaway-encoder/src/video.rs)) is
a plain (non-`Send`) trait; `push_frame(&mut self, frame: &VideoFrame)` is the exact call
site the filter chain must sit in front of.

## Decision

> Add a `FrameFilter` trait + an ordered, opt-in filter chain to `EncodeSession`, applied
> in `write_frame` immediately before `encoder.push_frame`. **v1 operates on CPU-backed
> frames only** — a filter chain attached to a session that receives a `Gpu`-backed frame
> fails loudly, it never silently reads back.

### `FrameFilter` — trait, not a bare closure

```rust
/// One step of an [`EncodeSession`] frame filter chain.
///
/// Operates on `VideoFrameStorage::Cpu` frames only (see ADR-0001) — a session
/// with a non-empty filter chain rejects `Gpu`-backed frames with
/// `FilterError::GpuFrameUnsupported` rather than silently reading them back.
pub trait FrameFilter: 'static {
    /// Transform one frame. May return a different frame (new pixels, new pts)
    /// or the input unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] when the filter rejects or fails to process `frame`.
    fn process(&mut self, frame: VideoFrame) -> Result<VideoFrame, FilterError>;
}
```

- **Trait over a bare closure type**, because:
  1. A concrete `impl FrameFilter` struct is where per-filter state naturally lives (e.g.
     a frame-averaging filter needs a `history: VecDeque<VideoFrame>` field). A `FnMut`
     closure can hold state too via `move` capture, so this alone isn't disqualifying —
     but a named struct is more debuggable (shows up in backtraces/logs as a real type,
     not `<closure>`).
  2. Matches this codebase's existing plug-in-your-own-backend idiom: `VideoEncoder`,
     `VideoDecoder`, `AudioEncoder` are all traits, not closures, for the same
     "caller supplies an implementation" shape (`api-layers.md` § "Traits for caller
     contracts").
  3. Choosing trait-first does not block closures forever — a blanket
     `impl<F: FnMut(VideoFrame) -> Result<VideoFrame, FilterError> + 'static> FrameFilter
     for F` is a pure additive follow-up once a real caller wants closure ergonomics.
     Not adding it now is "no abstractions for one-off code," not a permanent rejection.
- `process` takes and returns an **owned** `VideoFrame` (not `&VideoFrame`), so a filter
  that keeps history (the averaging case) can move the frame straight into its buffer
  instead of being forced into an extra clone at its own boundary. Ownership moves
  frame-to-frame through the chain with zero *additional* copies past the one entry clone
  described below.

### Composition — ordered chain, not a single slot

`EncodeSession` holds `SmallVec<[Box<dyn FrameFilter>; 4]>` (field name `filters`), pushed
in registration order and run in that order. Chain, not a single `Option<Box<dyn
FrameFilter>>`, because:

- Realistic v1 use (resize, color-correct/LUT, watermark, simple temporal denoise) wants
  ordered composition. A single-slot API would just force every multi-step caller to
  hand-write their own compositor `FrameFilter` — moving the composition problem onto
  every caller instead of solving it once, which defeats the point of offering a hook.
- The implementation delta between "one optional filter" and "an ordered `Vec`-like
  chain" is a loop instead of an `if let` — not a meaningfully bigger surface.
- `SmallVec<[_; 4]>` (already an approved dependency per `zero-cost-abstractions.md`, used
  in-tree by `iso-bmff`) matches the "usually-small, bounded list" case exactly: v1 chains
  are expected to be 0–4 filters; heap spill beyond that stays correct.

### API surface — additive, no change to `open`/`write_frame` signatures

```rust
impl<E: VideoEncoder> EncodeSession<E> {
    pub fn open(encoder: E) -> Result<Self, PipelineError>;          // unchanged
    pub fn write_frame(&mut self, frame: &VideoFrame)
        -> Result<(), PipelineError>;                                 // unchanged signature

    /// Append a filter to the chain (runs after previously pushed filters).
    /// Filters may be pushed at any point before or between `write_frame` calls.
    pub fn push_filter<F: FrameFilter>(&mut self, filter: F) -> &mut Self;
}
```

`push_filter` — not a `.with_filter()` fluent builder on `open` — because `open` already
takes a single positional `encoder` argument, not a builder; nothing else in this crate
uses a fluent-construction shape. `push_filter` boxes internally (`Box::new(filter)`) so
callers pass a plain value, not `Box::new(...)`; it returns `&mut Self` only so optional
chaining (`session.push_filter(a).push_filter(b);`) works, not to imply typestate.

### `write_frame`'s insertion point

```rust
pub fn write_frame(&mut self, frame: &VideoFrame) -> Result<(), PipelineError> {
    if self.filters.is_empty() {
        self.encoder.push_frame(frame)?;               // unchanged fast path, zero clone
    } else {
        if matches!(frame.storage, VideoFrameStorage::Gpu(_)) {
            return Err(PipelineError::Filter(FilterError::GpuFrameUnsupported));
        }
        // clone: entry point into an owned filter chain — the caller only lent a
        // reference, but VideoFrame::clone() is a Bytes refcount bump (Cpu) or a
        // Copy of a small handle (Gpu, unreachable here), never a pixel memcpy.
        // Paid exactly once per frame, only when a filter chain is attached.
        let mut current = frame.clone();
        for filter in &mut self.filters {
            current = filter.process(current)?;
        }
        self.encoder.push_frame(&current)?;
    }
    self.drain()
}
```

- **Empty chain costs nothing** beyond one `is_empty()` check — no clone, no branch into
  filter machinery. Zero-Copy `Gpu`-backed pipelines with no filters attached are
  completely unaffected, preserving "Zero-Copy stays reachable" (`api-layers.md` rule 3).
- **`Gpu`-backed frame + non-empty chain fails loudly** (`FilterError::GpuFrameUnsupported`)
  instead of silently skipping the chain or silently reading back. Skipping would look
  like a filter bug ("why didn't my filter run?"); readback would silently break the
  Zero-Copy contract the caller opted into by choosing `VideoInputPreference::ZeroCopyGpu`
  upstream. An explicit error is the only honest option per `caveats-and-clarity.md`.

### Error handling — propagate through `write_frame`'s existing `Result`

New crate-local error type, `thiserror`, matching `PipelineError`'s existing shape:

```rust
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FilterError {
    /// A filter in the chain rejected or failed to process the frame.
    /// Details in logs when available (mirrors `EncodeError::Backend`).
    #[error("frame filter failed")]
    Rejected,
    /// A filter chain is configured but this frame is GPU-backed
    /// (`VideoFrameStorage::Gpu`) — v1 filters are CPU-frame-only (ADR-0001).
    #[error("frame filter chain does not support GPU-backed frames")]
    GpuFrameUnsupported,
}
```

`PipelineError` gains `Filter(#[from] FilterError)`, following the exact `#[from]` pattern
already used for `Encode`/`Mux`. No swallowed/logged-only filter errors: a filter failure
aborts the `write_frame` call the same way an encoder or mux failure already does — this
crate has no precedent for a "soft fail and continue" path, and inventing one only for
filters would be a silent-failure surface the rest of the crate doesn't have.

`Rejected` has no `String` payload (unlike a hypothetical `Rejected(String)`), matching
`EncodeError::Backend`'s existing "details in logs when available" convention rather than
introducing an `Other(String)`-shaped escape hatch — `error-handling.md` prefers specific
variants over open string payloads.

### ZCA shape — `Box<dyn FrameFilter>`, not a generic `EncodeSession<E, F>`

- The chain is a **caller-configured, heterogeneous, runtime-sized** list: a resize filter,
  a color-correct filter, and a watermark filter are different concrete types. A generic
  `EncodeSession<E, F: FrameFilter>` can only hold one repeated `F`, which cannot express a
  real multi-step chain at all — it isn't a weaker version of the chain, it's a different
  (unusable for this purpose) feature.
- An `EncodeSession<E, F>` generic would also need a `NoFilter` unit-type sentinel to make
  "no filters attached" expressible without `Option`-wrapping the whole session type, and
  the extra type parameter would propagate to every place `EncodeSession` is named
  (including cross-platform call sites already holding `Box<dyn VideoEncoder>`).
- Per-frame cost is one vtable dispatch **per attached filter**, not per pixel — bounded by
  chain length (expected 0–4), the same class of dynamic-dispatch cost this crate already
  accepts for `Box<dyn VideoEncoder>` in platform dispatch. `zero-cost-abstractions.md`
  names exactly this case ("facade plugin feature, documented") as the sanctioned use of
  `Box<dyn Trait>`.
- `SmallVec<[Box<dyn FrameFilter>; 4]>` keeps the *list itself* off the heap for the
  expected common case, even though each element is still a `Box<dyn FrameFilter>` — the
  two concerns (list allocation vs. per-filter type erasure) are independent, and the ZCA
  doc's own SmallVec section calls out "small track tables, short lists" as the pattern
  this matches.

## Deferred

A GPU shader-pass / render-graph framework at this same seam (WGSL compute passes,
node-graph composition, GPU-resident filter chains that avoid the CPU-only restriction
above) was proposed and **explicitly deferred by the user** in this design conversation.
It is out of scope for this ADR and not referenced further here. If revisited, it needs
its own ADR — it is a materially bigger design (GPU pipeline state, shader compilation/
caching, cross-backend WGSL/HLSL/MSL story) than a plain Rust hook and should not be
folded into `FrameFilter` retroactively.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Bare `FnMut(VideoFrame) -> Result<VideoFrame, FilterError>` closure, no trait | Works for stateless/simple-state filters via `move` capture, but loses named-type debuggability and breaks from this codebase's existing trait-based plug-in idiom (`VideoEncoder`/`VideoDecoder`); can be added later as a blanket impl without breaking this design |
| Single `Option<Box<dyn FrameFilter>>` slot instead of a chain | Forces every multi-step caller to hand-write their own compositor filter; barely simpler to implement than a real chain |
| `.with_filter()` fluent builder on `open` | `open` isn't a builder today; no other method on this type uses fluent chaining; `push_filter` covers the same need without inventing a new construction shape |
| Silently skip the filter chain for `Gpu`-backed frames | Looks like a filter bug from the caller's side — no signal that the chain was bypassed |
| Silently CPU-readback `Gpu`-backed frames to run filters | Breaks the Zero-Copy contract the caller opted into upstream (`VideoInputPreference::ZeroCopyGpu`); forbidden by `caveats-and-clarity.md` |
| Generic `EncodeSession<E, F: FrameFilter>` | Cannot express a heterogeneous multi-filter chain; needs a `NoFilter` sentinel for the empty case; propagates a new type parameter everywhere `EncodeSession` is named |
| Swallow/log filter errors instead of propagating | No precedent elsewhere in this crate for silent partial failure; contradicts "no silent failure" project norm |
| GPU shader-pass / render-graph hook (this same seam) | Explicitly deferred by the user this session — materially bigger scope, needs its own ADR |

## Consequences

### Positive

- Callers get in-pipeline frame transforms (crop, color-correct, watermark, temporal
  blend) without hand-rolling a `push_frame`/`poll_packet` loop around the low-level
  `VideoEncoder` trait themselves.
- Zero cost when unused: no clone, no branch into filter machinery, on the existing
  no-filter fast path — current callers of `write_frame` see no behavior or performance
  change.
- `write_frame`/`open` signatures are unchanged — fully additive, no breaking change to
  existing `EncodeSession` callers.
- Low-level surface stays reachable: `FrameFilter` is a public trait usable directly, not
  hidden behind `EncodeSession`; the encoder/muxer low-level path (`mux_roundtrip.rs`) is
  untouched.

### Negative / Trade-offs

- `Gpu`-backed frames cannot be filtered in v1 — a caller building a GPU Zero-Copy capture
  pipeline who also wants filtering must either filter before uploading to GPU or wait for
  the deferred GPU-pass design.
- `Box<dyn FrameFilter>` per chain element means one vtable dispatch per filter per frame
  (bounded, not per-pixel, but not free).
- One more small error enum (`FilterError`) and one more `PipelineError` variant to keep in
  sync with `mediaway-encoder`/`mediaway-container`'s own error surfaces.

## References

- [`src/session.rs`](../src/session.rs) — `EncodeSession`, insertion point
- [`src/error.rs`](../src/error.rs) — `PipelineError`, `#[from]` pattern followed here
- [`mediaway-common/src/frame.rs`](../../mediaway-common/src/frame.rs) — `VideoFrame`,
  `VideoFrameStorage`
- [`mediaway-common/src/gpu.rs`](../../mediaway-common/src/gpu.rs) — `GpuBufferHandle` is
  `Copy`
- [`mediaway-encoder/src/video.rs`](../../mediaway-encoder/src/video.rs) — `VideoEncoder`
  trait shape
- Workspace ADR-0014: `docs/adr/0014-pipeline-convenience-crate.md` (this crate's founding
  decision)
- [`docs/spec/api-layers.md`](../../../docs/spec/api-layers.md) — low-level stays
  reachable, Zero-Copy stays reachable
- [`docs/spec/zero-cost-abstractions.md`](../../../docs/spec/zero-cost-abstractions.md) —
  `Box<dyn Trait>` / `SmallVec` guidance (ADR-0009)
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) — no
  silent readback/copy (ADR-0006)
- [`docs/conventions/error-handling.md`](../../../docs/conventions/error-handling.md) —
  `thiserror` shape, prefer specific variants over `Other(String)`

ADRs are written in **English**.
