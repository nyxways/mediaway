# ADR-0005: FLV container C ABI (out-buffer push, fixed track slots)

- **Status**: Accepted
- **Date**: 2026-08-07
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi` (container module)

## Context

`adr/0003-multi-format-c-abi.md`'s format-shape table flagged FLV as one of the four
formats (with MPEG-TS, MP3, WAV) whose Rust method shape is **deliberately** kept outside
`mediaway-container`'s shared `Mux`/`Demux` traits — `flv::Muxer` does not implement `Mux` at
all (see that crate's own module docs). `adr/0004-ogg-adts-c-abi.md` closed Ogg/ADTS (both
close enough to reuse one ADR); this ADR gives FLV its own, since its shape diverges from
Ogg/ADTS in a genuinely new way: buffer ownership.

### Why FLV doesn't fit any existing C ABI shape

- `Muxer::write_header(has_audio, has_video, out: &mut Vec<u8>)` and
  `Muxer::push_packet(packet, out: &mut Vec<u8>)` both write **directly into a
  caller-supplied buffer on every call** — there is no internal accumulation and no
  separate `poll_bytes` step the way MP4/WebM/Ogg/ADTS all have.
- `Muxer::add_track(&StreamInfo)` has a **fixed one-video/one-audio slot** (FLV's tag format
  has no track-id field at all) — `StreamInfo::id` is ignored; video vs. audio is
  distinguished by which `StreamInfo` variant is passed, not by a caller-assigned id.
- The demux side (`push_bytes`/`streams`/`poll_packet`) **does** match the shape every other
  format's demuxer already has — only the mux side is unusual.

## Decision

> Add `mediaway_flv_muxer_t`/`mediaway_flv_demuxer_t` — dedicated handles, same pattern
> `adr/0004` established for Ogg/ADTS. The demuxer side is a direct mirror of
> `mediaway_ogg_demuxer_t` (wraps `flv::Demuxer`, same 5 functions). The muxer side is new
> shape: `mediaway_flv_muxer_write_header`/`mediaway_flv_muxer_push_packet` both take
> `uint8_t **out_data, size_t *out_len` out-parameters and allocate + return a fresh owned
> buffer on every call — mirroring the Rust API's own `out: &mut Vec<u8>` parameter instead
> of inventing an internal-accumulation model the core doesn't have.

### 1. No `mediaway_flv_muxer_flush` — there is nothing to flush

Unlike Ogg/ADTS (whose `flush()` is a documented no-op kept for shape parity with
`mediaway_muxer_flush`), FLV's Rust `Muxer` has no `flush` method at all — every
`write_header`/`push_packet` call already produces its complete output synchronously.
Adding a no-op `mediaway_flv_muxer_flush` here would invent a function with nothing behind
it, unlike Ogg/ADTS where the underlying method exists and is genuinely a no-op.

### 2. `write_header`/`push_packet` allocate a fresh buffer per call, not append to one

Every prior mux handle (`mediaway_muxer_poll_bytes`, `mediaway_ogg_muxer_poll_bytes`,
`mediaway_adts_muxer_poll_bytes`) drains an internally-accumulated buffer that may span
multiple prior pushes. FLV's Rust API has no such internal buffer — `out: &mut Vec<u8>` is
the caller's, filled fresh each call. The C ABI mirrors this exactly: each function returns
only the bytes written by *that* call, requiring its own `mediaway_buffer_free`. A caller
wanting one contiguous FLV stream concatenates the returned buffers itself (same as the
existing `mediaway_muxer_poll_bytes` calling pattern already requires in a loop).

### 3. `add_video_track`/`add_audio_track` split, not one `add_track`

Mirrors the existing `mediaway_muxer_add_video_track`/`_add_audio_track` split (MP4/WebM) —
`MediawayVideoTrackInfo`/`MediawayAudioTrackInfo` already carry the right fields, and the
Rust `add_track(&StreamInfo)` dispatches on the enum variant internally anyway. `info->id`
is accepted for struct-shape consistency with the other formats but is **documented as
ignored** — FLV has no track-id concept, and silently accepting-but-ignoring a field is
already precedent-following since e.g. WebM has no analogous quirk to compare, but honest
documentation of the ignored field avoids a caller wrongly assuming ids matter here.

### 4. Status codes reuse `UnsupportedCodec`/`UnknownStream`, no new variants

`flv::Error::UnsupportedCodec` and `flv::Error::UnregisteredStream` map onto the exact same
`MediawayStatus` variants ADR-0003 added for WebM — both describe the identical situation
(unencodable codec at `add_track`; unregistered stream at `push_packet`). `flv::Error::Tag`
(the underlying `flv_core::Error` framing errors) collapses to `InvalidData`, same posture
as Ogg/ADTS's non-exhaustive-tail mapping.

### 5. ABI version bump

`MEDIAWAY_CONTAINER_FFI_ABI_VERSION` `3 -> 4`.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| An internal accumulation buffer + `mediaway_flv_muxer_poll_bytes`, matching every other muxer | Would require the FFI layer to invent state the Rust `flv::Muxer` doesn't have (it never buffers internally) — a genuine behavior difference from the wrapped core, not just an API-shape convenience |
| One `add_track` function with a `bool is_video` discriminant instead of two functions | The existing `mediaway_video_track_info_t`/`mediaway_audio_track_info_t` split already carries the right fields per media kind; a single function would need a tagged union or two info pointers where at most one is non-null — more surface, not less |
| Fold `write_header`'s output into the first `push_packet` call implicitly | Changes the wire format the caller receives silently (header bytes prepended to the first packet's bytes) instead of matching the Rust API's own two explicit calls — less predictable for a C caller counting output buffers |

## Consequences

### Positive

- FLV (AVC video + AAC/MP3 audio, the still-in-use real-world tag shape) is now reachable
  from C/C++/C#/Python/Node.
- Verified end-to-end: `tests/flv_container_smoke.rs` round-trips one AVC video packet and
  one AAC audio packet (dts/pts/keyframe/payload all checked), plus explicit
  unsupported-codec and unregistered-stream rejection tests.

### Negative / Trade-offs

- 5 of 8 formats (`mp4`, `webm`, `ogg`, `adts`, `flv`) are now reachable from the C ABI;
  MPEG-TS, MP3, WAV remain Rust-only (see `adr/0003-multi-format-c-abi.md`'s Deferred
  section for their planned bespoke shapes).
- No language binding (C++/C#/Python/Node) wiring in this pass — same scoping as every ADR
  in this series.
- A caller must free a separate buffer per `write_header`/`push_packet` call rather than
  draining one accumulated buffer — more `_free` calls than the other formats' mux APIs, but
  an honest reflection of the wrapped core's actual buffering (or lack of it).

## References

- `crates/mediaway-container/src/flv.rs` — the format module's actual method shape (source
  of truth), including the "why `Muxer` doesn't implement `Mux`" module-doc rationale
- `crates/mediaway-container/src/flv_tests.rs` — reference mux/demux round trips this ADR's
  own FFI test payloads were derived from
- `adr/0003-multi-format-c-abi.md` — the original format-shape survey flagging FLV as
  incompatible with the shared handles
- `adr/0004-ogg-adts-c-abi.md` — the dedicated-handle pattern this ADR follows for the demux
  side and the overall handle-per-format structure
- `crates/mediaway-ffi/tests/flv_container_smoke.rs` — the round-trip verification

ADRs are **English**. Numbering is local to this `adr/` folder.
