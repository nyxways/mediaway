# ADR-0002: Facade modules for `riff-wave`/`adts`/`mpeg-audio`/`ogg`/`flv`/`mpeg-ts`

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-container`

## Context

Six freestanding container cores (`riff-wave`, `adts`, `mpeg-audio`, `ogg`,
`flv`, `mpeg-ts`) existed as workspace members with no Mediaway-typed facade
module — the same state `ebml-webm` was in before ADR-0001. Each has a
genuinely different shape (whole-buffer vs. incremental; single implicit
stream vs. multi-stream via PSI/tags; fixed vs. per-track timebase), so a
single generic adapter pattern does not fit all six uniformly.

## Decision

> One thin adapter module per core (`adts`, `wav`, `mp3`, `ogg`, `flv`, `ts`),
> each implementing [`Demux`] where the format's own semantics support
> incremental push/poll, and [`Mux`] only where a `Packet`'s generic
> `pts`/`dts`/`payload` shape is enough to write a correct frame — not forced
> where it would silently paper over real per-format detail.

Module naming avoids colliding with the extern crate of the same name where
one exists (`wav`/`mp3`/`ts` instead of `riff_wave`/`mpeg_audio`/`mpeg_ts`);
`adts`/`ogg`/`flv` module names don't collide (verified: Rust resolves a bare
`use adts::X` inside `mod adts` to the extern crate, not `self`, since the
module itself is never a member of its own namespace under its own name).

### Per-format `Mux` fit

| Format | `Mux` impl | Why / why not |
|--------|-----------|---------------|
| `adts` | Yes | One frame per `push_packet`; only `payload` is needed. |
| `mpeg_audio` | **No** | `write_frame` needs an explicit `padding: bool` per frame (bit-reservoir accounting) — no slot in `Packet`; defaulting it would silently write wrong-length frames for real VBR-ish streams. |
| `ogg` | Yes | `packet.pts` → `granule_position`, `packet.is_discard` → `eos`; both fit naturally. |
| `flv` | **No** (own method shape) | `Muxer::add_track`/`push_packet` gained the codec-aware `AudioTagHeader`/`VideoTagHeader` sub-framing symmetric with demux (2026-07-29, follow-up to this ADR) — still not the shared `Mux` trait, since the codec set (`add_track` rejects anything but AVC video / AAC·MP3 audio) doesn't map cleanly onto `Mux`'s codec-agnostic contract without either a fallible `push_packet` per-track-codec check the trait has no room to express, or silently accepting codecs FLV can't actually carry. |
| `mpeg_ts` | **No** | PTS/DTS are a fixed 90 kHz clock, not a per-track `Rational` — reinterpreting `Packet::pts` (arbitrary time base) as 90 kHz would silently desync playback. Exposes `write_access_unit(pid, data, pts_90k, dts_90k, ...)` directly instead. |
| `riff_wave` | **No** (buffer-then-`finish()`) | RIFF chunk sizes must be known before the header is written — there is no incremental flush to fit `Mux::poll_bytes`'s "append what's ready" contract. |

### Demux-side codec identification

- **`adts`/`mpeg_audio`**: single implicit stream; `pts`/`duration`
  synthesized from a running sample count (1024 samples/frame for ADTS-AAC;
  1152/576 for MPEG-1 / MPEG-2·2.5 Layer III) — neither format carries timing
  metadata itself.
- **`ogg`**: codec identified by reading the first packet's declared
  identification header (`OpusHead` magic + fixed fields per RFC 7845 §5.1;
  Vorbis identification header per Vorbis I §4.2.2) — reading a codec's own
  declared config bytes, the same boundary `iso-bmff` already crosses for
  AAC's `esds`. `granule_position` becomes `pts` directly (Opus: always 48 kHz
  per RFC 7845 §4; Vorbis: the stream's own rate, parsed from the header).
- **`flv`**: reads `AudioTagHeader`/`VideoTagHeader` sub-bytes the same way
  (`SoundFormat`/`CodecID`, `AACPacketType`/`AVCPacketType`) to split
  sequence-header tags (→ `extra_data`) from data tags (→ `Packet`).  Only AVC
  video and AAC/MP3 audio are recognized (the common real-world case);
  everything else (VP6, Sorenson H.263, Nellymoser, …) has no `CodecKind`
  mapping and is dropped — same posture as WebM's VP8 gap (ADR-0001).
- **`mpeg_ts`**: `StreamType` already comes from PMT parsing in the core; this
  facade only maps it to `CodecKind` and fixes `time_base = 1 / 90_000`.

### New `CodecKind` variants

`Mp3` and `Vorbis` were added to `mediaway_common::CodecKind` (`RawAudio` was
already added earlier this session for WASAPI capture and is reused for
`riff_wave`'s PCM, not duplicated). Adding `Vorbis` also closed half of
WebM's already-documented VP8/Vorbis gap (ADR-0001) — `webm.rs::codec_kind`
now maps `A_VORBIS`, a one-line follow-up once the variant existed anyway.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Force every format through `Mux`/`Demux` uniformly | Would require silently guessing padding bits (MP3), reinterpreting a 90 kHz clock as an arbitrary time base (TS), or building codec-aware tag sub-framing not yet symmetric with demux (FLV) — each a real correctness risk, not a style preference. |
| One `mediaway-container-audio` crate bundling `adts`/`mpeg_audio`/`ogg`/`wav` | Contradicts ADR-0012 (thin adapters live in the facade, not a new crate) and mixes formats with genuinely different mux fit. |

## Consequences

### Positive

- All 6 cores are now reachable through Mediaway-typed `StreamInfo`/`Packet`,
  not just their own freestanding types.
- Real per-format subtleties (MP3 padding, TS 90 kHz clock, Ogg granule
  semantics, FLV tag sub-framing) are documented at the point they matter,
  not hidden behind a one-size-fits-all trait.

### Negative / Trade-offs

- `mpeg_audio`/`flv`(mux)/`mpeg_ts`(mux)/`riff_wave` don't implement the
  shared `Mux` trait — callers needing those must use each module's own
  method names, not a common interface.
- `flv` mux gained a codec-aware convenience layer symmetric with its own
  demux side (2026-07-29) — closes the gap this ADR originally left open,
  though it's still not the shared `Mux` trait (see the per-format table
  above).

## References

- [ADR-0001](0001-webm-ebml-demux.md) — same thin-adapter pattern, VP8/Vorbis gap partially closed here
- Crate-local ADRs: `riff-wave/adr/0001`, `adts/adr/0001`, `mpeg-audio/adr/0001`, `ogg/adr/0001`, `flv/adr/0001`, `mpeg-ts/adr/0001`
- [`docs/ai/wiki/container/audio-containers.md`](../../../docs/ai/wiki/container/audio-containers.md), [`general-containers.md`](../../../docs/ai/wiki/container/general-containers.md)
