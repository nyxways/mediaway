# ADR-0001: `mpeg-ts` — freestanding MPEG-2 Transport Stream mux + demux

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mpeg-ts`

## Context

MPEG-TS underlies broadcast, HLS segment payloads, and many RTP profiles. It is
the largest of the container formats added this session (188-byte packetization,
PAT/PMT PSI tables with their own CRC-32 variant, and PES packetization with
bit-packed PTS/DTS) and had no support anywhere in the workspace.

## Decision

> New unprefixed freestanding crate `mpeg-ts` (naming: ADR-0012), sans-io, no
> Mediaway dependency. **Single-program v1 scope**: one PAT entry, one PMT,
> a handful of elementary streams (`H264`/`Hevc`/`Aac`/`Mp3` — the codecs this
> workspace already supports elsewhere).

- Module split mirrors the format's own layering: `packet.rs` (188-byte TS
  packet read/write + adaptation-field stuffing, shared by PSI and PES), `psi.rs`
  (PAT/PMT section build/parse), `pes.rs` (PES header + PTS/DTS bit-packing),
  `crc.rs` (MPEG-2 PSI CRC-32 — same polynomial as `ogg`'s CRC but a **different
  init value**, 0xFFFFFFFF vs 0; the two are not interchangeable), `mux.rs`/
  `demux.rs` (tie the layers together per program).
- **Adaptation-field stuffing correctness was the one real bug this crate's own
  round-trip tests caught**: the first draft only wrote a mandatory
  `adaptation_field` flags byte when `random_access_indicator` needed setting,
  but the spec requires that flags byte whenever `adaptation_field_length > 0`
  (i.e. for *pure padding* too) — producing 189-byte "packets" instead of 188.
  Fixed by `packet.rs::write_adaptation_field`, which distinguishes the `budget
  == 1` case (pure stuffing, no flags byte — the field is just its own
  zero-length byte) from `budget >= 2` (flags byte mandatory, then stuffing).
- Frames already-encoded elementary-stream access units — no H.264/HEVC/AAC/MP3
  encode/decode, same "frame, don't encode" boundary as `adts`/`mpeg-audio`/
  `ogg`/`flv`.
- PES supports PTS-only and PTS+DTS (not "neither") — every access unit needs at
  least a presentation timestamp for this crate's mux API to be meaningful.
- No PCR insertion — `PMT`'s `PCR_PID` is always written as the reserved
  "unassigned" value `0x1FFF`; real playback timing reconstruction from PCR is a
  deferred feature, not silently faked.
- PSI (PAT/PMT) sections spanning more than one TS packet are not reassembled —
  this crate's own `Muxer` never produces one (a single program with a few
  streams always fits in one packet), but an arbitrary third-party stream with a
  very large PMT would not parse correctly here.
- `Demuxer::poll_access_unit` only confirms an access unit complete when the
  *next* PES packet on the same PID starts (inherent to PES framing, not a
  limitation specific to this crate) — `Demuxer::finish()` force-flushes
  whatever's still accumulating per PID, so the very last access unit isn't lost
  at end-of-stream.

## Consequences

- Multi-program transport streams, PCR, DTS-only (no PTS) access units, and
  multi-packet PSI sections are out of scope — tracked in `docs/roadmap.md`.
- No `mediaway-container` facade wiring yet (freestanding core only).

## References

- `crates/adts/adr/0001-adts-freestanding-core.md`, `crates/mpeg-audio/adr/0001-mpeg-audio-freestanding-core.md`, `crates/ogg/adr/0001-ogg-freestanding-core.md`, `crates/flv/adr/0001-flv-freestanding-core.md` — same "frame already-encoded data" boundary applied to sibling formats
- `crates/ogg/src/crc.rs` — the sibling CRC-32 variant with a different init value; cross-checked via `crc_tests.rs::differs_from_ogg_variant_on_same_input`
- ADR-0012 (workspace) — unprefixed freestanding-core naming
- ISO/IEC 13818-1 (MPEG-2 Systems) — paywalled; not pinned. Field layout implemented from widely-documented community references (e.g. multimedia.cx / tsduck project notes) and cross-checked via this crate's own mux↔demux round-trip tests, which caught one real framing bug (see above) before it shipped
