# ADR-0005: Backfill mux track `extra_data` after the first encoded packet

- **Status**: Accepted
- **Date**: 2026-08-20
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: mediaway

## Context

`EncodeSession::open` registers the video encoder's stream as an MP4 track via
`mp4::Muxer::add_track(encoder.stream_info().clone())`, **before any frame has
been encoded**. This assumes `stream_info()` is already final at `open()` time
— true for backends whose config record (SPS/PPS-derived `avcC`, etc.) is
fully determined by open-time config (Windows WMF, Linux VA-API).

It is **not** true for `VideoToolbox` (macOS/iOS): `VTCompressionSession`
determines SPS/PPS internally, only knowable after the encoder has actually
produced its first sample (delivered asynchronously via the output callback).
`AppleVideoEncoder::stream_info()` therefore reports an empty `extra_data`
until that first callback fires — permanently, if nothing later corrects it.

`iso_bmff::mux::Muxer::push_packet` already has a self-healing path for
exactly this shape of problem: it converts each packet's own payload via
`to_avcc`, and if the payload was Annex-B-framed with in-band SPS/PPS, it
backfills the track's still-empty `extra_data` before the `moov` header is
written. This masked the gap for backends that emit Annex-B with in-band
parameter sets on keyframes (matches Windows/Linux output), but
`VideoToolbox`'s samples come back **AVCC-length-prefixed already**, with no
in-band parameter sets at all — `to_avcc`'s `is_annex_b` check fails
immediately, so the self-healing path never triggers for Apple. The track's
`extra_data` stayed empty through `push_packet` → `write_avc1` wrote its
`[1, 0x42, 0x00, 0x1e, 0xff, 0xe1, 0, 0, 1, 0, 0]` placeholder `avcC` — a
structurally valid but hollow (zero-length SPS/PPS) record. Downstream, this
crate's own `VideoDecoderConfig` requires exactly one non-empty SPS + PPS at
`open()` for H.264 (`mediaway-decoder` apple ADR-0001 § Scope); the hollow
placeholder failed that check with `DecodeError::InvalidInput`, surfacing
only in `bindings-tests-macos`'s `DecodeRoundtripTests` — the very first real
end-to-end Apple encode→mux→demux→decode round trip on real hardware, since
this crate's Apple backends carried a zero-compile-verification caveat until
that CI job existed.

## Decision

> Add `Muxer::set_track_extra_data(track_id, extra_data)` (both
> `iso_bmff::mux::Muxer<Live>` and `mediaway_container::mp4::Muxer<Live>`) —
> backfills a still-empty track's `extra_data`, a no-op once real data is
> already present or once the `moov` header is already written. Call it from
> `EncodeSession::drain()`, once per drained packet, before
> `muxer.push_packet`: by the time `encoder.poll_packet()` returns a packet,
> the encoder has necessarily finished encoding it, so `encoder.stream_info()`
> now reflects the real, finalized value for any backend (async or sync).

- In scope: video track `extra_data` only — audio codecs' config records
  (AAC's `AudioSpecificConfig`, Opus) are derivable from open-time config on
  every backend today, so `AudioTrack`/`drain_audio` are untouched.
- The fix is additive and generic — `drain()`'s new check runs for every
  platform, not just Apple, but is a cheap no-op once a backend's
  `stream_info()` already had real `extra_data` at `open()` time (Windows,
  Linux), since `set_track_extra_data` only writes when the muxer's own field
  is still empty.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Make `VideoToolboxVideoEncoder::open()` synchronously probe SPS/PPS by encoding a throwaway frame | Changes what "open" means (implicitly starts encoding); `open()` has no real pixel data to encode with anyway |
| Delay `add_track`/`begin()` until the first packet is known (lazy track registration) | Requires reshaping `mp4::Muxer`'s `Open`/`Live` typestate and `EncodeSession::open`'s return contract (a session must exist before any frame can be written) — much larger surface for the same outcome |
| Teach `to_avcc`'s self-healing path to also recognize AVCC-framed (non-Annex-B) sample payloads | Conflates "extract config from a packet's own bitstream" with "backend-reported config" — `VideoToolboxVideoEncoder` already computes the correct SPS/PPS-derived `extradata` once per session; re-deriving it a second time from AVCC-framed sample bytes is redundant and doesn't generalize past H.264 |

## Consequences

### Positive

- Apple H.264 encode→mux round trips now embed a real, non-empty `avcC` —
  unblocks `bindings-tests-macos`'s `DecodeRoundtripTests`.
- Zero behavior change for backends that already report real `extra_data` at
  `open()` time.

### Negative / Trade-offs

- One extra `stream_info()` call + pattern match per drained packet, on every
  platform — cheap (no allocation on the empty-check path) but not free.
- HEVC on Apple has the identical `VTCompressionSession`-internal-SPS/PPS
  shape as H.264 — covered by the same fix (both go through
  `VideoToolboxVideoEncoder`'s shared `finalized_info`/`stream_info()` path),
  but only H.264 was hardware-verified by the CI run that surfaced this bug.

## References

- `mediaway-encoder` [apple/adr/0001](../../mediaway-encoder/adr/apple/0001-videotoolbox-h264-cpu-upload.md)
  — VideoToolbox's async output-callback design, `finalized_info`
- `mediaway-decoder` [apple/adr/0001](../../mediaway-decoder/adr/apple/0001-videotoolbox-h264-cpu-out.md)
  § Scope — one SPS + one PPS required at `open()`
- `crates/iso-bmff/src/mux/mod.rs` — `Muxer::push_packet`'s existing in-band
  Annex-B self-healing path
