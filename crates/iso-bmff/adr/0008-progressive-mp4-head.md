# ADR-0008: A non-fragmented MP4 head, with edit lists

- **Status**: Accepted
- **Date**: 2026-10-13
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `iso-bmff`

## Context

A stream copy that starts at a moment which is not a keyframe has to start at the keyframe
before it: a video decodes only from there. The copied packets before the wanted moment must
not be shown. MP4 has a way to say so, the edit list (`edts`/`elst`, ISO/IEC 14496-12 § 8.6.6):
the track's presentation starts at `media_time` of its own timeline. First caller: Recoway's
checkpoint clips (a clip of "the last 5 minutes" began up to a keyframe interval early, and on
a screen that does not change, whose encoder interval counts frames, up to half a minute).

The fragmented muxer cannot carry it usefully. Measured with FFmpeg (n7.1.1, `ffprobe -v trace`)
on a 4 s clip written by `Muxer` with an `elst` of `media_time = 0.967 s` in the `moov`:

```text
[mov,mp4,m4a,3gp,3g2,mj2] track[0].edit_count = 1
[mov,mp4,m4a,3gp,3g2,mj2] advanced_editlist does not work with fragmented MP4. disabling.
start_time=-0.966667  duration=5.000001  (150 frames decoded; 121 shown were wanted)
```

FFmpeg's demuxer applies an edit list to the sample tables of a non-fragmented file (it trims the
samples before `media_time`, flags the ones needed to decode them as discard, and starts at zero)
and, for a fragmented one, only shifts the timestamps by it: the head stays and the clip starts at
a negative time. Everything built on libavformat (Chromium included) is that demuxer. The same
clip remuxed by `ffmpeg -ss 3 -c copy` into a non-fragmented file gives `start_time=0.000000`,
120 frames, with the pre-roll packets flagged `D`.

## Decision

> `isobmff::write_progressive(buf, &[ProgressiveTrack], payload_len)` writes `ftyp`, a `moov`
> with real sample tables, and the header of the `mdat` box that follows; the caller writes the
> payload bytes next.

- **Sans-io and sample-table driven:** the head is a function of the samples
  (`StblSample`: offset in the payload, size, `dts`, duration, composition offset, sync) and
  optionally an edit list per track (`EditListEntry`, any number of entries: an empty edit,
  `media_time = -1`, puts a late track's start later). It is the writing counterpart of
  `parse_stbl_samples`, and its tests read it back with that parser.
- **`moov` first** (faststart): a player can start from the first bytes. The size of the head
  is known before any payload byte is written because every table is: `stco` offsets are
  `head length + sample offset`, found in a second pass (the width of the offsets, `stco` or
  `co64`, decides the length, and is decided by the total with the widest guess). The `mdat`
  header is 8 bytes, or 16 (`largesize`) when the payload passes 4 GiB.
- **One sample to a chunk** (`stsc` of one entry): the caller interleaves tracks in the order it
  likes, and the offsets say where; the chunk table is as long as the sample table. Durations
  are `stts` runs, offsets `ctts` runs (version 1 when any is negative), sync samples `stss` (left
  out when all are).
- **Lengths:** `mdhd` is the sum of the durations, `tkhd` the edit list's total (else the media's)
  in the movie timescale (1000, `MOVIE_TIMESCALE`), `mvhd` the longest track.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| An `elst` in the fragmented `moov` (`Muxer::set_edit`, tried first and not kept) | Written and tested; FFmpeg ignores the trimming for fragmented files (above), so the clip started at -0.97 s and showed the whole head. Safari/QuickTime and Media Foundation apply it to either, but the common demuxer does not. |
| Streaming progressive muxer (`mdat` first, `moov` at the end, as `ffmpeg` does) | The point is a known set of samples; a streaming writer needs a seekable sink or a second pass, and the `moov` at the end delays a player by the whole file. |
| Re-encode the first GOP so it starts on the wanted frame | A second encoder session and decoding at clip time; the caller asked for none. |

## Consequences

### Positive

- A stream copy can start at any frame, and the demuxer that most players use shows it so.
- The file starts playing from its first bytes (`moov` before `mdat`).

### Negative / Trade-offs

- The caller must know every sample before it writes the head: a live capture is not a caller
  (that is the fragmented muxer's). A file cut off while written has no `moov`.
- The hidden head is bytes in the file, and a player that ignores edit lists shows it.
- One-sample chunks make `stco` as long as `stsz` (4 bytes a sample); a clip of 20,000 samples
  carries 80 kB of them.
- Only `Track` codecs the fragmented path knows (`write_stsd`); no encryption, no `sidx`.

## References

- `src/isobmff/progressive.rs`, `progressive_tests.rs`; ISO/IEC 14496-12 § 8.6.6 (edit list).
- FFmpeg/ffprobe behavior observed on builds n7.1.1 and N-121806 (2025); FFmpeg's source was not
  consulted (it is LGPL), the message above is the tool's own output.
