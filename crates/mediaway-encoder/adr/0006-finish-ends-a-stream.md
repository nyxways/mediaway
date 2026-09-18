# ADR-0006: `finish(self)` ends a stream; dropping discards what is in flight

- **Status**: Accepted
- **Date**: 2026-09-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder`

## Context

`VideoEncoder` and `AudioEncoder` end a stream with `flush()` followed by `poll_packet()` until
`Ok(None)`. That was documented as the intended lifecycle, but what happens if a caller skips
it was not written down. It is silent data loss. Hardware encoders are pipelined, so frames
still inside the encoder when it is dropped never become packets, and they are the *last*
frames of the stream.

It happened. A screen recorder built on this crate dropped its WMF encoder unflushed at
every stop and every resize-restart. Measured on a live window, every recording lost its
final frame (59→58, 19→18, 21→20 frames; with a flush, 23, 22, 18, 18 all arrived). On the
same backend, an unflushed drop was also the sharpest trigger for the crash fixed in #108.

`Drop` cannot fix this. It has nowhere to return the flushed packets, so flushing inside it
would lose the same frames, only later.

## Decision

> Both traits gain a provided method, `finish(self) -> Result<Vec<Packet>, EncodeError>`,
> that flushes and collects every remaining packet. It is the documented way to end a stream.
> Dropping an unflushed encoder is documented as *discarding* its in-flight frames: that is
> the way to abandon an encode.

- A provided method with `where Self: Sized`, so every backend gets it with no per-backend
  code, and `Box<dyn VideoEncoder>` gets it through the existing forwarding impl.
- It consumes the encoder, so a finished encoder cannot be pushed to again.
- `flush` + `poll_packet` stay for callers that need the encoder to remain usable.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Flush in `Drop` | The packets it produces have nowhere to go; the frames are lost either way. |
| Warn when an unflushed encoder is dropped | The workspace has no logging dependency, so the options were a debug-build panic or adding `tracing`. The panic would fire on legitimate aborts. `tracing` would be the workspace's first logging dependency, an ADR of its own. Dropping it was the user's call. |
| `abandon(self)` for explicit discard | Without a drop warning it would behave exactly like `drop`, and add nothing a reader cannot already see. |
| Leave it to documentation alone | The lifecycle *was* documented. A method that does the right thing is harder to get wrong than a paragraph describing it. |

## Consequences

### Positive

- Ending a stream correctly is one call, on every encoder.
- The trait docs now say what an unflushed drop costs.
- The hardware zero-copy test uses `finish` and requires *every* frame back, not "some
  packets", so an end-of-stream that loses frames fails it.

### Negative / Trade-offs

- `finish` allocates a `Vec` for the tail. It is a handful of packets, once per stream.
- If the flush or a poll fails, packets already collected are lost with the error. Callers
  who need them use `flush` and `poll_packet` directly.
- Nothing stops a caller from dropping without finishing. That is documented, not
  enforced: Rust cannot require a method call before drop.

## References

- ADR-0001 (the encoder traits), `crates/mediaway-encoder/src/video.rs`, `audio.rs`
- `adr/windows/0012-async-mft-zero-copy-sequencing.md` § Addendum (#108)
