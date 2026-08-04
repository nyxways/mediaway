# ADR-0004: Indefinite-`Cluster` sibling-ID lookahead + mux lacing

- **Status**: Accepted
- **Date**: 2026-08-05
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `ebml-webm`

## Context

`adr/0001` and `adr/0002` each left one gap explicitly deferred (not silently
dropped, tracked in `docs/roadmap.md`):

1. **Indefinite-size `Cluster` lookahead** (demux): an indefinite-size
   `Cluster` stayed open on the element stack until its parent `Segment`
   closed or EOF. For a live-streaming `WebM` producer that writes many
   indefinite-size `Cluster`s back to back, each new sibling `Cluster` got
   nested (pushed) under the still-open previous one instead of closing it
   first — the open-element stack grew by one entry per `Cluster` for the
   rest of the stream.
2. **Mux-side `SimpleBlock` lacing**: `adr/0003`'s muxer always wrote one
   `SimpleBlock` per pushed frame. The demuxer already reads all three
   lacing kinds (Xiph, fixed-size, EBML — `adr/0002`) for other encoders'
   output, but this crate's own muxer had no way to *write* a laced block —
   callers with many small same-timecode sub-frames (e.g. short audio
   frames) had no way to amortize the per-`SimpleBlock` header cost.

## Decision

> Close both gaps in place — no new crate, no breaking API changes to
> existing callers.

1. **Sibling-ID lookahead** (`demux.rs`, `ids.rs`): rather than a full
   parent/child schema table (this crate models only a subset of Matroska's
   element tree — a generic table risks mistaking a real-but-unmodeled child
   of `Cluster`, e.g. `SilentTracks`/`Void`, for a sibling and closing early),
   `ids::is_segment_level_child` recognizes only the IDs that can *only* ever
   be a direct `Segment` child (`SegmentInfo`/`Tracks`/`Cluster`/`Cues`/`SeekHead`
   — exactly the set `ids::is_descend_master` already treats as `Segment`'s
   children). `Demuxer::step` checks this before deciding whether to
   push/handle the new element: while the open-element stack's top is an
   indefinite-size `Cluster` and the new ID is segment-level, pop and close
   it first (RFC 8794 §9.4 unknown-size resolution). This bounds the stack
   to `Segment` + at most one open `Cluster`, regardless of stream length.
2. **EBML lacing writer** (`mux.rs`, `lacing.rs`, `vint.rs`): new
   `Muxer::push_laced_frames(track, timecode, keyframe, payloads: &[&[u8]])`
   writes one `SimpleBlock` with EBML lacing (chosen over Xiph/fixed-size —
   most size-efficient generically, and the demux side already decodes it).
   `payloads.len() == 1` falls back to the existing plain `write_simple_block`
   (identical wire bytes to `push_frame`). New low-level primitives:
   `vint::encode_size_fixed_len` (writes an unsigned VINT at an
   **explicitly chosen** length rather than `encode_size`'s auto-minimal
   one — `encode_size` itself now delegates to it) and
   `vint::encode_signed_delta` (EBML lacing's bias-shifted signed VINT,
   exact inverse of `lacing::split`'s `Lacing::Ebml` decode branch — the
   length must be chosen *before* encoding since the bias depends on it,
   unlike a plain size VINT). `lacing::encode_ebml_lace_sizes` threads these
   together for the full sub-frame size list (all but the last — matching
   `split`'s "last frame takes whatever remains" decode convention).
   `MAX_LACED_FRAMES = 256` (EBML's `frame_count - 1` field is one byte);
   `MuxError::EmptyLace`/`LaceTooLarge` cover misuse.
3. **External oracle** (`tests/mux_oracle.rs`): mirrors `iso-bmff`'s
   `tests/conformance_oracle.rs` — mux a small two-track `WebM`, run system
   `ffprobe` on it, assert success and that it recognizes both codecs.
   Skips cleanly when `ffprobe` is absent (Tier 7). This replaces mux
   verification that previously relied *only* on round-tripping through
   this crate's own `Demuxer` (`adr/0003`'s documented risk: "some subtle
   wire-format assumption this crate's demux and mux sides happen to agree
   on diverges from the actual Matroska spec in a way neither side's tests
   would catch") with an independent implementation's read of the bytes.

## Consequences

### Positive

- Live/streaming `WebM` producers (indefinite-size `Cluster`s) no longer
  grow this demuxer's open-element stack unboundedly.
- Callers can now express real Matroska lacing on write, not just read it.
- Mux output is verified against a second, independent `WebM` implementation
  (`ffprobe`), not only this crate's own demux/mux pair.

### Negative / Trade-offs

- `ids::is_segment_level_child` is deliberately narrower than a full EBML
  "unknown-size resolution" per RFC 8794 §9.4 (which also accounts for
  global elements like `Void`/`CRC-32` valid under any master) — it only
  covers the specific gap this session's roadmap tracked (indefinite
  `Cluster`, closed by a `Segment`-level sibling). A file using indefinite
  sizes on some *other* element this crate doesn't model would not benefit;
  that was already unrecoverable before this ADR (`step`'s existing
  "indefinite size on a non-descend element" halt), not a regression.
- `push_laced_frames` still only ever emits `SimpleBlock`s, never
  `BlockGroup` — unchanged from `adr/0003`'s original scope decision (no
  real product need for `BlockGroup` on write).

## References

- `adr/0001-ebml-vint-webm-schema-v1.md`, `adr/0002-full-matroska-profile.md`,
  `adr/0003-webm-mux.md` — the three gaps this ADR closes
- `crates/iso-bmff/tests/conformance_oracle.rs` — oracle test pattern reused here
- `docs/standards/registry.toml` (`rfc-8794-ebml`, `webm-container-guidelines`)
