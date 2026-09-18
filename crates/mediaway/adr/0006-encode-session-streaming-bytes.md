# ADR-0006: Streaming byte output on `EncodeSession`

- **Status**: Accepted
- **Date**: 2026-09-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: mediaway

## Context

`EncodeSession` had exactly one way to get container bytes out: `finish()`, which
consumes the session and returns the entire recording as a single `Vec<u8>`.

Everything underneath it already streams. `mediaway_container::mp4::Muxer` implements
`Mux::poll_bytes(&mut Vec<u8>) -> usize`, and `iso_bmff::mux::Muxer` drains its internal
buffer on every call (draining the consumed prefix once it passes 64 KiB, so a polled
muxer's memory stays bounded). `examples/pipeline/screen_record.rs` drives that muxer
directly and writes to a `File` as it goes — precisely because it cannot do so through
`EncodeSession`.

So the convenience layer was the only non-streaming element in a streaming stack. For any
long recording that is not a footnote:

- a one-hour 20 Mbps capture is ~9 GB, held in RAM until the session ends;
- nothing reaches disk until `finish()`, so a crash loses the whole recording rather than
  its tail;
- the caller cannot start uploading, segmenting, or hashing output while capture runs.

This contradicts the workspace rule that Mediaway is **streaming-first, whole-buffer only
as convenience** (`AGENTS.md` § Architecture & API shape rule 5,
[`docs/spec/async-and-streaming.md`](../../../docs/spec/async-and-streaming.md) ·
ADR-0007), and the rule that convenience layers **compose** low-level capabilities rather
than hide them (`docs/spec/api-layers.md`). It is closer to a bug against those rules than
to a missing feature.

## Decision

> We adopt incremental byte output on `EncodeSession`, with `finish()` redefined as a
> convenience wrapper over it.

Three methods, additive — no existing signature changes:

```rust
pub fn poll_bytes(&mut self, out: &mut Vec<u8>) -> usize;
pub fn finish_into(self, out: &mut Vec<u8>) -> Result<usize, PipelineError>;
pub fn finish(self) -> Result<Vec<u8>, PipelineError>;   // unchanged signature
```

- `poll_bytes` appends whatever container bytes are ready and returns how many. It is
  callable at any point in the session and delegates straight to
  `mp4::Muxer::poll_bytes`. Polling is optional; a caller that never polls behaves exactly
  as before.
- `finish_into` flushes both encoders and the muxer, then appends the remaining bytes to
  the caller's buffer. It **consumes** the session.
- `finish` is now `let mut bytes = Vec::new(); self.finish_into(&mut bytes)?; Ok(bytes)`.

**`finish_into` and `finish` return what has not been polled yet, not the whole stream.**
A caller that polled during the session receives only the tail; a caller that never polled
receives the complete recording, which is the pre-existing behaviour. This is stated in the
rustdoc on both methods, per `docs/spec/caveats-and-clarity.md` (the source must carry the
contract).

### Scope

- `mediaway` crate only. No container, encoder, or muxer change — the capability already
  existed one layer down.
- No async surface. Sans-io/poll shape, consistent with ADR-0007.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `flush(&mut self)` + caller polls afterwards | Leaves the session alive in a flushed state, so `write_frame` after `flush` becomes a runtime-invalid call nothing rejects. Consuming `self` in `finish_into` makes that unrepresentable through ownership alone — no state flag, no typestate parameter, no runtime check. |
| `EncodeSession<E, State>` typestate for open/flushed | A second generic parameter on a type that callers name in their own signatures, to encode a transition that ownership already encodes. Over-engineered for a two-state machine whose terminal state has no methods. |
| `finish_into<W: Write>(self, w: &mut W)` | Puts I/O inside the facade for a caller that may not want a `Write` sink (upload buffer, ring, hasher). `&mut Vec<u8>` matches the `Mux` trait's own shape, and writing a `Vec` to any `Write` is one line at the call site. |
| Return `Bytes` instead of appending to `&mut Vec<u8>` | Would force an allocation per poll. Appending lets the caller reuse one buffer across the whole session — the point of the change. |
| Leave it; callers who need streaming use `mp4::Muxer` directly | That is the status quo, and it is what `examples/pipeline/screen_record.rs` had to do. It makes the convenience layer unusable for its most obvious use case (long recordings) and violates api-layers' "convenience composes, it does not exclude." |

## Consequences

### Positive

- A long session's memory is bounded by poll cadence instead of by recording length.
- Output can reach disk/network during capture, so a crash costs the un-flushed tail
  rather than everything.
- `EncodeSession` now satisfies the streaming-first rule it previously broke, and
  `examples/pipeline/screen_record.rs`-style incremental writing becomes expressible
  through the facade.
- Purely additive: existing `finish()` callers compile and behave identically.

### Negative / Trade-offs

- Two exits now exist for the same session, and their meaning depends on whether the
  caller polled. Documented explicitly rather than designed away — any streaming API has
  this property.
- `poll_bytes` returning `0` is ambiguous between "no packets yet" and "everything already
  drained". Callers that care must track their own totals. Reporting the difference would
  require threading muxer state that no current caller needs.
- A caller who polls but never calls `finish_into`/`finish` gets a truncated, unflushed
  fragment stream. Same hazard as the underlying muxer, now reachable from one layer up.

## References

- [`docs/spec/async-and-streaming.md`](../../../docs/spec/async-and-streaming.md) ·
  workspace ADR-0007 — streaming-first, whole-buffer as convenience
- [`docs/spec/api-layers.md`](../../../docs/spec/api-layers.md) — convenience composes
  low-level APIs
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) ·
  ADR-0006 (workspace) — the source carries the contract
- workspace [ADR-0014](../../../docs/adr/0014-pipeline-convenience-crate.md) —
  `EncodeSession`'s original shape
- [ADR-0003](0003-audio-track-and-apm-integration.md) — `finish()`'s two-track flush order,
  preserved here
- `crates/mediaway-container/src/lib.rs` — the `Mux::poll_bytes` contract this mirrors
- `examples/pipeline/screen_record.rs` — the caller that had to bypass `EncodeSession`
