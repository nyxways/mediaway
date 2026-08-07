# ADR-0007: MP3 (MPEG Layer III) container C ABI (fixed header, explicit padding bit)

- **Status**: Accepted
- **Date**: 2026-08-07
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi` (container module)

## Context

`adr/0003-multi-format-c-abi.md`'s format-shape survey flagged MP3 as one of the four
formats deliberately kept outside `mediaway-container`'s shared `Mux`/`Demux` traits — its
own module docs say real Layer III streams flip a per-frame padding bit to average out
fractional frame lengths (a bit-reservoir detail `mpeg_audio::Muxer::write_frame` requires
as an explicit argument), which the generic `Packet`-based trait has no slot for. This ADR
gives MP3 its own dedicated C ABI, following the per-format-ADR pattern established by
`adr/0005-flv-c-abi.md`/`adr/0006-mpeg-ts-c-abi.md`.

### Why MP3 doesn't fit any existing C ABI shape

- `Muxer::new(header: FrameHeader)` fixes bitrate/sample-rate/channel mode for the whole
  session — there is no track registration at all, closer to Ogg/ADTS's single-implicit-
  stream shape than to FLV/MPEG-TS's per-track/per-PID model.
- `Muxer::write_frame(&self, frame_body, padding: bool, out: &mut Vec<u8>)` writes directly
  into a caller-supplied buffer (the FLV/MPEG-TS out-buffer shape), but its `padding`
  parameter has no equivalent field on `mediaway_packet_view_t` — every other packet-based
  push function in this crate would need to either invent one (breaking every existing
  caller's struct layout) or silently default it, misencoding real bit-reservoir accounting.
- `Demuxer` matches the standard `push_bytes`/`streams`/`poll_packet` shape (same as
  Ogg/ADTS/FLV/MPEG-TS's demux sides) — only the mux side is unusual.

## Decision

> Add `mediaway_mp3_muxer_t`/`mediaway_mp3_demuxer_t` — dedicated handles. A new
> `mediaway_mp3_frame_header_t { version; bitrate_kbps; sample_rate; channel_mode; }` input
> struct (plus `mediaway_mpeg_version_t`/`mediaway_channel_mode_t` enums mirroring
> `mpeg_audio::MpegVersion`/`ChannelMode`) feeds `mediaway_mp3_muxer_create`.
> `mediaway_mp3_muxer_write_frame` mirrors FLV/MPEG-TS's out-buffer-per-call shape, with an
> explicit `bool padding` parameter. The demux side is a direct mirror of
> `mediaway_adts_demuxer_t` (single implicit stream, same 5 functions).

### 1. `mediaway_mp3_muxer_create` has no status side channel

Same reasoning as ADR-0004 §2 / ADR-0006 §1: `mp3::Muxer::new` can fail
(`Error::UnsupportedBitrate`/`Error::UnsupportedSampleRate`), but the constructor's return
type has no `mediaway_status_t` slot — a non-standard bitrate/sample-rate combination and a
caught panic both collapse to `NULL`. A caller that needs to know *why* should validate
against `mpeg_audio`'s documented standard bitrate/sample-rate tables before calling.

### 2. New `mediaway_mpeg_version_t`/`mediaway_channel_mode_t` enums, not a reused type

Unlike Ogg/ADTS/FLV/MPEG-TS (which all reuse `mediaway_codec_kind_t` or existing packet/
stream types), MP3's `FrameHeader` needs two genuinely new small enums with no equivalent
elsewhere in this crate's C surface — `MpegVersion` (3 values) and `ChannelMode` (4 values).
Both are `#[non_exhaustive]` in `mpeg_audio`, but this ADR only exposes the variants that
crate currently defines; a future variant would need a corresponding C ABI addition, same as
every other enum this crate mirrors from a non-exhaustive Rust source.

### 3. `write_frame` takes `&self`, not `&mut self` — no behavioral change to the wrapper

`mp3::Muxer::write_frame` is an immutable method (the header is fixed at construction, no
mutable session state to track) — the C wrapper still takes `mediaway_mp3_muxer_t *muxer`
(not `const`) for consistency with every other mux function's pointer constness in this
header, even though the underlying Rust call doesn't require `&mut`.

### 4. Status codes reuse `InvalidPacket`, no new variants

`mp3::Error::FrameBodyLengthMismatch` (the only variant reachable from `write_frame` itself)
maps onto the same `MediawayStatus::InvalidPacket` ADR-0001 already defined for `mp4::Error
::InvalidPacket` — identical situation (a pushed payload's size doesn't match what the
container format requires). `UnsupportedBitrate`/`UnsupportedSampleRate` are only reachable
from the status-channel-less constructor (§1); `BadSyncOrReservedField`/`UnsupportedLayer`
(demux-side framing errors) collapse to `InvalidData`, matching every other format's
non-exhaustive-tail posture.

### 5. ABI version bump

`MEDIAWAY_CONTAINER_FFI_ABI_VERSION` `5 -> 6`.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Add a `padding` field to `mediaway_packet_view_t` | Every existing caller (MP4/WebM/Ogg/ADTS/FLV) would need to initialize a field meaningless to their format — a C ABI-breaking layout change for one format's bit-reservoir detail |
| Default `padding` to `false` and drop the parameter | Real Layer III encoders alternate padding to keep the average frame rate correct for a given bitrate/sample-rate combination that doesn't divide evenly — silently forcing `false` would corrupt real-world streams' timing, not just simplify the API |
| Reuse `mediaway_codec_kind_t`'s `Mp3` variant instead of a dedicated `mediaway_mpeg_version_t`/`channel_mode_t` pair | `CodecKind::Mp3` identifies the codec, not the MPEG version (1/2/2.5) or channel mode a Layer III frame header also needs — these are orthogonal pieces of information `FrameHeader` requires that no existing C type carries |

## Consequences

### Positive

- MP3 (MPEG-1/2/2.5 Layer III, the still-in-use real-world case) is now reachable from
  C/C++/C#/Python/Node.
- Verified end-to-end: `tests/mp3_container_smoke.rs` round-trips one 128 kbps/44100 Hz
  stereo frame (payload/pts/duration/stream-info all checked), a mono-channel-count case,
  and a wrong-frame-body-length rejection test.

### Negative / Trade-offs

- 7 of 8 formats (`mp4`, `webm`, `ogg`, `adts`, `flv`, `ts`, `mp3`) are now reachable from
  the C ABI; WAV remains Rust-only (see `adr/0003-multi-format-c-abi.md`'s Deferred section
  for its planned one-shot whole-buffer shape).
- No language binding (C++/C#/Python/Node) wiring in this pass — same scoping as every ADR
  in this series.
- `mediaway-ffi`'s `Cargo.toml` now depends on `mpeg-audio` directly (previously only
  reachable transitively through `mediaway-container`) to name `FrameHeader`/`MpegVersion`/
  `ChannelMode` in the FFI layer — adds no new crate to the dependency graph, since
  `mediaway-container` already pulls it in.

## References

- `crates/mediaway-container/src/mp3.rs` — the format module's actual method shape (source
  of truth), including the "why `Muxer` doesn't implement `Mux`" module-doc rationale
- `crates/mediaway-container/src/mp3_tests.rs` — reference mux/demux round trips this ADR's
  own FFI test payload length was derived from (`FrameHeader::frame_len`)
- `crates/mpeg-audio/src/types.rs` — `MpegVersion`/`ChannelMode`/`FrameHeader` definitions
  this ADR's C enums/struct mirror
- `adr/0003-multi-format-c-abi.md` — the original format-shape survey flagging MP3 as
  incompatible with the shared handles
- `adr/0004-ogg-adts-c-abi.md` — the dedicated single-implicit-stream demux shape this ADR
  reuses for `mediaway_mp3_demuxer_t`
- `crates/mediaway-ffi/tests/mp3_container_smoke.rs` — the round-trip verification

ADRs are **English**. Numbering is local to this `adr/` folder.
