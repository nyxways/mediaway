# ADR-0007: Mux payload placements — where each sample landed in the output

- **Status**: Accepted
- **Date**: 2026-09-25
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `iso-bmff`

## Context

A caller that writes an MP4 and also keeps the same packets for later (qarec's replay ring,
`mediaway-container/adr/0004`) holds every payload twice: once in the file, once in RAM. At
12 Mbps over five minutes that is about 450 MB of RAM for bytes that are already on disk. To
keep only a *reference* to the file instead, the caller needs to know where each sample's
payload was written. It cannot work that out from the outside: the muxer rewrites payloads
(Annex-B → length-prefixed, ADTS stripped), batches them into fragments, and decides when a
fragment is flushed.

## Decision

> `Muxer<Open>::with_placements()` opts into recording one
> `Placement { track_id, dts, offset, len }` per sample written. `Muxer<Live>::poll_placements`
> moves them out.

- **`offset` is absolute in the emitted stream**: counted from the first byte `poll_bytes`
  ever returned, so it is the file offset when the caller writes every polled byte
  sequentially from 0. The muxer tracks how many bytes it has drained from its output buffer
  (`drained`), so `output[i]` is stream byte `drained + i`.
- **`len` is the payload as written**, after conversion, so it can differ from the pushed
  payload's length. `track_id` is the caller's `stream_id`, not the ISOBMFF `track_ID`.
- **Recorded at fragment write**, which is also when the bytes become pollable. `write_fragment`
  ends with the `mdat` payload, so the fragment's payload starts at
  `output.len() - payload.len()`; samples follow back to back in push order.
- **Opt-in, zero work when off**: a `bool` checked once per fragment flush; the `Vec` is never
  touched or allocated. When on: one 24-byte push per sample into a `Vec` the caller drains
  (`Vec::append`, which keeps the muxer's capacity for reuse). The output bytes are identical
  either way.
- Sans-io is unchanged: the muxer still does no I/O; reading bytes back is the caller's.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Caller computes offsets by parsing the polled bytes (`moof`/`trun`) | Re-parses every fragment the muxer just built; duplicates box layout knowledge outside the crate. |
| Always record placements | Costs every muxer a push per sample for a feature one caller uses. |
| Offsets relative to each `poll_bytes` chunk | Forces the caller to track stream position itself and to pair chunks with placements; an absolute offset is what a file reader needs. |
| Return placements from `push_packet` | A sample's position is only known at fragment flush, possibly many pushes later. |

## Consequences

### Positive

- A file-writing caller can drop its in-memory payload copies and read them back by
  `(offset, len)`; the replay ring's `StoredPayload` is built for exactly that.

### Negative / Trade-offs

- Offsets assume the caller writes *every* polled byte, in order, from offset 0 of one file.
  A caller that skips, reorders or splits the stream across files must translate them.
- Payload bytes read back are the *converted* form (length-prefixed H.264/HEVC, raw AAC). A
  muxer they are pushed into again passes length-prefixed input through unchanged, but a
  consumer expecting Annex-B must convert.
- A sample whose payload exceeds `u32::MAX` bytes gets a clamped `len` (same limit as `trun`).

## References

- `src/mux/mod.rs` (`Placement`, `with_placements`, `poll_placements`), `src/mux/mux_tests.rs`
- `mediaway-container/adr/0004-replay-ring.md` § Updates (the consumer)
