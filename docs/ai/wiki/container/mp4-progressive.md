# Non-fragmented MP4 head with edit lists (`iso-bmff`)

`crates/iso-bmff/src/isobmff/progressive.rs` — `write_progressive(buf, tracks, payload_len)`.
[ADR-0008](../../../../crates/iso-bmff/adr/0008-progressive-mp4-head.md).

- **What:** `ftyp` + `moov` (real `stts`/`ctts`/`stss`/`stsc`/`stsz`/`stco`|`co64`, `elst` per track)
  + the `mdat` header, for samples all known before the file is written. The caller writes the payload
  next, samples at the `offset`s it gave. The mirror of `parse_stbl_samples`.
- **Why it exists:** a stream copy that starts at the keyframe before the wanted moment hides the head
  with an edit list. FFmpeg's demuxer trims by an edit list **only in a non-fragmented file**
  (`advanced_editlist does not work with fragmented MP4. disabling.`); in a fragmented one it shifts
  timestamps (start -0.97 s, head shown). So `Muxer::set_edit` on the fragmented muxer was not kept.
- **Layout:** `moov` before `mdat` (starts playing from the first bytes); one sample per chunk; `co64`
  and a 16-byte `mdat` header past 4 GiB; `mvhd` timescale 1000 (`MOVIE_TIMESCALE`, an `elst`
  entry's `segment_duration`); `media_time` in the track's own timescale; `-1` for an empty edit.
- **Costs:** the caller must know every sample up front; a file cut off while written has no `moov`;
  4 bytes of `stco` a sample.
- **First caller:** Recoway's checkpoint clips (`app/recoway/crates/recoway-media/src/checkpoint.rs`).
