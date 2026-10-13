# ADR-0009: A `moov` for samples already in the file (`write_indexed_moov`)

- **Status**: Accepted
- **Date**: 2026-10-14
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `iso-bmff`

## Context

A fragmented MP4 is written as it goes, so a crash leaves a file that plays to its last fragment.
It is also a file whose `moov` has empty sample tables: Windows (Media Foundation) reports no
duration for it, so players show no seek bar, and chat apps show no preview. A recorder wants both:
fragments while recording, an ordinary MP4 once it finishes cleanly, without rewriting gigabytes of
samples. OBS's "Hybrid MP4" (obs-studio `plugins/obs-outputs/mp4-mux.c`, GPL-2.0, read for the idea
only) does this; the first caller here is Recoway's recordings and exports.

Appending a complete `moov` after the fragments is not enough. Measured with FFmpeg (ffprobe
n7.1.1) on a file `ftyp, moov(fragmented, hidden as free), moof, mdat, ..., moov(sample tables)`:

```text
[mov,mp4,m4a,3gp,3g2,mj2] trun track id unknown, no tfhd was found
[mov,mp4,m4a,3gp,3g2,mj2] error reading header
```

FFmpeg's demuxer walks the top-level boxes in order and parses a `moof` before it has met the
`moov` at the end. The same bytes with the fragments inside one top-level `mdat` read cleanly.

## Decision

> `isobmff::write_indexed_moov(buf, &[ProgressiveTrack])` writes one `moov` with sample tables for
> samples that are already in a file, at the **absolute** offsets in `StblSample::offset`.

- It is `write_progressive`'s `moov` without the `ftyp`, the `mdat` header or a base offset, so
  the box does not depend on where it is put and can be appended to a finished file.
- **Chunks of adjacent samples.** Samples of a track whose bytes follow each other (a fragment's
  run) share a chunk (`stsc` has an entry where the samples per chunk change): the chunk table is
  as long as the file's fragments, not its samples. `write_progressive` keeps one sample per chunk.
- `stco`, or `co64` when any chunk starts past 4 GiB; durations, `ctts`, `stss` and edit lists as
  in `write_progressive`. A gap in a track's timeline is the caller's to put into a duration.
- **`mdhd` version 1** when a track's media is longer than a 32-bit count of ticks. This is a fix
  to both writers: at a microsecond timescale the 32-bit duration saturated at 71.6 minutes.
- The caller's half of the hybrid file (the layout that makes FFmpeg read it) is not here; it is
  Recoway's `recoway_media::hybrid`: a 16-byte `free` box behind the front `moov` that a clean
  finish turns, together with the `moov`'s type (`free`), into a 64-bit `mdat` header over every
  fragment, in one write after the new `moov` is flushed.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Append the `moov` and hide the front one, leave the `moof`s at top level | FFmpeg stops at the first `moof` (above). |
| Rename every `moof` to `free` | One scattered write per fragment (64,000 for five hours), and a crash half way leaves neither form playable. |
| `write_progressive` with a base offset of zero | Writes an `ftyp` and an `mdat` header; one sample per chunk makes the table 4 bytes a sample longer. |
| Rewrite the file as a non-fragmented MP4 at the end | Time proportional to the file (tens of gigabytes), at the moment the user pressed Stop. |

## Consequences

### Positive

- A finished recording is an ordinary MP4 for every reader, and the file is the fragmented one it
  was at every point before the last write.

### Negative / Trade-offs

- The `moov` is at the end of the file: a player that streams it must reach the end first.
- The fragments' `moof` boxes and the old `moov` stay in the file as bytes a reader skips.
- The `moov` is about 3.6 bytes a sample with 1 us timescale durations that alternate (18 MB for five
  hours of 60 fps video with sound, measured in Recoway).

## References

- `src/isobmff/progressive.rs`, `progressive_tests.rs`; ISO/IEC 14496-12 § 8.7 (sample tables).
- FFmpeg/ffprobe n7.1.1 behavior as observed; FFmpeg's source was not consulted (LGPL).
- Recoway `app/recoway/docs/design.md` § 2.1 and `crates/recoway-media/src/hybrid.rs`.
