# ADR-0003: WebM mux facade (`webm::Muxer`)

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-container`

## Context

`adr/0001` scoped `webm` to demux only, deferring `Mux`/`ContainerFormat` —
`ebml-webm` itself had no muxer at the time. `ebml-webm/adr/0003` now adds a
real `mux::Muxer`. This ADR wires it into the Mediaway-typed facade, mirroring
`mp4::Muxer`'s open/live typestate + `Mux` trait impl shape.

## Decision

> `webm::Muxer<Open | Live>` wraps `ebml_webm::mux::Muxer`, converting
> `StreamInfo` → `ebml_webm::TrackInfo` on `add_track` and `Packet` →
> `push_frame` args on `push_packet`. Gated by the `mux` Cargo feature
> (parallel to the existing `demux` gate on `Demuxer`) — `pub mod webm;`'s
> gate broadens from `demux` to `any(mux, demux)`.

1. **`CodecKind` → `WebM` `CodecID` is a hand-written reverse table**
   (`webm_codec_id`), not a derived inverse of the existing `codec_kind`
   (demux → `CodecKind`) function — the two aren't quite symmetric (demux
   accepts `"A_AAC"`-prefixed profile strings a muxer never needs to write).
   Supports exactly the same codec set demux already recognizes (`Vp9`,
   `Av1`, `Opus`, `Vorbis`, `Aac`); any other `CodecKind` (`H264`, `Hevc`,
   `Mp3`, …) is rejected with `Error::UnsupportedCodec` at `add_track` time,
   not discovered later as a malformed write.
2. **New facade-local `Error` enum** (`Mux(ebml_webm::MuxError)` +
   `UnsupportedCodec` + `UnknownStream`) rather than reusing
   `ebml_webm::MuxError` directly — the facade has failure modes the core
   crate doesn't know about (codec mapping, `StreamInfo`'s `#[non_exhaustive]`
   variants).
3. **Track id = `WebM` `TrackNumber`, both directions** (`u64::from(id)` on
   write, `track_id`'s existing saturating `u32::try_from` on read) — same
   1:1 mapping the demux side already established; a muxer this facade
   itself drives never assigns a `TrackNumber` outside `u32` range, so the
   only lossy direction (reading a real-world file with a huge
   `TrackNumber`) was already a demux-only concern before this ADR.
4. **No new product-facing knobs** — `Cluster` batching, `TimecodeScale`,
   etc. all use `ebml_webm::mux::Muxer`'s own defaults; a caller who needs
   different values constructs the core `ebml_webm::mux::Muxer` directly
   (still first-class/public per the low-level-APIs rule) rather than this
   facade growing pass-through parameters for everything the core exposes.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Reuse `ebml_webm::MuxError` as the facade's own `Mux::Error` | Can't represent `UnsupportedCodec`/`UnknownStream`, which are facade-level, not core-crate-level, failures |
| Silently drop unsupported-codec tracks instead of erroring `add_track` | Contradicts "no silent slow defaults / no silent drops" (`docs/spec/caveats-and-clarity.md`); the demux side already treats an unmapped codec as a visible gap (track omitted from `streams()`, documented), not a mux-time error — but *writing* a track a reader can never see back is a worse failure mode than refusing upfront |

## Consequences

### Positive

- WebM's Container support README row can move from mux 🛠️ to a real mark —
  `mediaway-container::webm` now has a complete `Mux`+`Demux` pair, same
  shape as `mp4`.
- Round-trip-testable end-to-end (facade `Muxer` → facade `Demuxer`) in
  `webm_tests.rs`, on top of `ebml-webm`'s own lower-level round-trip tests.

### Negative / Trade-offs

- Same "no external WebM oracle" caveat as `ebml-webm/adr/0003` — this
  facade's mux path is only verified against its own demux path, not a
  second independent WebM implementation.
- `H264`/`Hevc` (real WebM allows neither in practice, this isn't a gap) and
  `Mp3` (real WebM doesn't define an MP3 `CodecID` either) correctly stay
  unsupported — but `Aac` support here means Mediaway can write an
  Opus/Vorbis/AAC-audio + VP9/AV1-video WebM, not an arbitrary
  Mediaway-internal codec set.

## References

- `adr/0001-webm-ebml-demux.md` (this crate)
- `ebml-webm/adr/0003-webm-mux.md`
- `crates/mediaway-container/src/mp4.rs` (typestate + `Mux` impl pattern reused here)
