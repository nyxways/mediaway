# ADR-0006: MPEG-TS container C ABI (construction-time streams, 90 kHz clock, finish array)

- **Status**: Accepted
- **Date**: 2026-08-07
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi` (container module)

## Context

`adr/0003-multi-format-c-abi.md`'s format-shape survey flagged MPEG-TS as one of the four
formats deliberately kept outside `mediaway-container`'s shared `Mux`/`Demux` traits — its
own module docs say `Muxer` does not implement `Mux` for the same reason `mp3`/`wav` don't:
"real callers of a 90 kHz-native mux need to pass `pts_90k`/`dts_90k` explicitly, not a
`Packet`'s arbitrary-time-base `pts` silently reinterpreted as 90 kHz." This ADR gives MPEG-TS
its own dedicated C ABI, following the per-format-ADR pattern `adr/0005-flv-c-abi.md`
established.

### Why MPEG-TS doesn't fit any existing C ABI shape

- `Muxer::new(program_number, pmt_pid, streams: &[ElementaryStream])` takes the **full
  elementary stream list upfront** — there is no `add_track` after construction at all,
  unlike every other mux handle in this crate (even FLV, whose track slots are fixed but
  still separately registered via `add_video_track`/`add_audio_track` calls).
- `write_pat_pmt`/`write_access_unit` both write directly into a caller-supplied buffer, the
  same out-buffer shape ADR-0005 already established for FLV.
- `write_access_unit(pid, data, pts_90k, dts_90k: Option<u64>, random_access, out)` takes
  **raw `pts_90k`/`dts_90k` clock values**, not a [`mediaway_common::Packet`] at all — the
  90 kHz system clock is a format-level fact, not a per-track timebase choice a `Packet`'s
  `pts`/`dts` fields could represent honestly.
- `Demuxer::finish() -> Vec<Packet>` is a genuinely new shape: no other format's demuxer has
  a method that returns more than one packet at once. MPEG-TS only confirms a PES packet's
  boundary once the *next* packet on the same PID starts, so the very last access unit per
  PID needs an explicit end-of-stream flush.

## Decision

> Add `mediaway_ts_muxer_t`/`mediaway_ts_demuxer_t` — dedicated handles, same pattern
> `adr/0004`/`adr/0005` established. A new `mediaway_ts_elementary_stream_t { pid; codec; }`
> input struct feeds `mediaway_ts_muxer_create`'s stream list.
> `mediaway_ts_muxer_write_pat_pmt`/`_write_access_unit` mirror FLV's out-buffer-per-call
> shape; `_write_access_unit` takes `pts_90k`/`has_dts`/`dts_90k` as explicit parameters
> (C has no `Option<T>`, so `Option<u64>` becomes a `bool` + `uint64_t` pair). The demux side
> adds `mediaway_ts_demuxer_finish`, returning a **new** owned-array shape (`mediaway_packet_t
> **out_packets, size_t *out_count`) with its own `mediaway_ts_demuxer_finish_free` — the
> first array-of-owned-structs return value in this crate.

### 1. `mediaway_ts_muxer_create` has no status side channel

Same reasoning as ADR-0004 §2 (`mediaway_adts_muxer_create`): `ts::Muxer::new` can fail
(`Error::InvalidPid`), but the constructor's return type has no `mediaway_status_t` slot —
an invalid PID, an unsupported codec (no `StreamType` mapping for the requested
`mediaway_codec_kind_t`), and a caught panic all collapse to `NULL`. A caller that needs to
know *why* should validate PIDs/codecs against the documented constraints before calling.

### 2. `Option<u64>` becomes `bool has_dts` + `uint64_t dts_90k`, not a sentinel value

A sentinel (e.g. `UINT64_MAX` meaning "no DTS") would silently misencode a real access unit
whose DTS happens to equal that sentinel — vanishingly unlikely for a 90 kHz clock in
practice, but an honest boolean flag has no such edge case at all and costs one parameter.

### 3. `mediaway_ts_demuxer_finish` is a new owned-array shape, with its own free function

Every other demuxer's outputs (`poll_packet`, `stream_at`) return exactly one owned value
per call. `finish()` returns `Vec<Packet>` — zero, one, or many packets in a single call,
reflecting one pending access unit per still-open PID. Reusing `mediaway_packet_free` (which
only knows how to free one `mediaway_packet_t`, not an array of them) would be a type
mismatch a C caller could pass by accident with no compiler warning; a dedicated
`mediaway_ts_demuxer_finish_free(packets, count)` makes the array ownership explicit and
matches this crate's existing `ptr, len` pair convention (`mediaway_buffer_free`).

### 4. Status codes reuse `UnknownStream`, no new variants

`ts::Error::UnknownPid` (an access unit's PID isn't registered) maps onto the same
`MediawayStatus::UnknownStream` ADR-0003 added for WebM and ADR-0005 reused for FLV —
identical situation (`push_packet`/`write_access_unit` referencing an unregistered
stream/PID). `ts::Error::InvalidPid` maps to `InvalidArgument` (only reachable via `From` for
non-exhaustive completeness, since the muxer constructor itself has no status channel per
§1). Everything else (`BadSyncByte`, `CrcMismatch`, unexpected PSI `table_id`, ...) collapses
to `InvalidData`, matching Ogg/ADTS/FLV's non-exhaustive-tail posture.

### 5. ABI version bump

`MEDIAWAY_CONTAINER_FFI_ABI_VERSION` `4 -> 5`.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Fold `mediaway_ts_elementary_stream_t` array into an incremental `add_stream` call, deferring `Muxer::new` until a `begin()`-style call | Invents a typestate the Rust `ts::Muxer` doesn't have — `Muxer::new` genuinely requires the full PMT upfront (PAT/PMT packets must describe every stream before any access unit is written); an `add_stream`/`begin` split would need to buffer stream registrations somewhere the core itself doesn't |
| A sentinel `dts_90k` value for "no DTS" instead of `has_dts` | See Decision §2 — an honest boolean has no misencoding edge case, a sentinel does (even if astronomically unlikely at 90 kHz) |
| Cap `finish()` at one packet per call (poll-style, call repeatedly until empty) | Would require the FFI layer to buffer the `Vec<Packet>` `finish()` already computed in one call across multiple C calls — extra state to manage for no benefit over returning the array directly, since `finish()` is a one-shot end-of-stream operation, not a hot per-frame poll loop |

## Consequences

### Positive

- MPEG-TS (H.264/HEVC video, AAC/MP3 audio — the still-in-use real-world subset
  `mpeg_ts_core::StreamType` supports) is now reachable from C/C++/C#/Python/Node.
- Verified end-to-end: `tests/ts_container_smoke.rs` round-trips one H.264 video access unit
  and one AAC audio access unit through PAT/PMT + `write_access_unit`/`poll_packet`
  (PID/pts/keyframe/payload all checked), a `finish()` case recovering a PES packet with no
  trailing marker, and an invalid-PID construction-rejection test.

### Negative / Trade-offs

- 6 of 8 formats (`mp4`, `webm`, `ogg`, `adts`, `flv`, `ts`) are now reachable from the C
  ABI; MP3, WAV remain Rust-only (see `adr/0003-multi-format-c-abi.md`'s Deferred section
  for their planned bespoke shapes).
- No language binding (C++/C#/Python/Node) wiring in this pass — same scoping as every ADR
  in this series.
- `mediaway_ts_demuxer_finish`'s array-of-owned-structs return is genuinely more complex to
  bind correctly than every other function in this crate (a binding author must free each
  element's `payload` *and* the array itself, in the right order) — documented explicitly in
  both the header comment and the Rust doc comment to reduce the chance of a use-after-free
  in a future binding.

## References

- `crates/mediaway-container/src/ts.rs` — the format module's actual method shape (source of
  truth), including the "why `Muxer` doesn't implement `Mux`" module-doc rationale
- `crates/mediaway-container/src/ts_tests.rs` — reference mux/demux round trips this ADR's
  own FFI test payloads were derived from
- `crates/mpeg-ts-core/src/types.rs` — `StreamType`/`ElementaryStream` definitions this
  ADR's `mediaway_ts_elementary_stream_t` mirrors
- `adr/0003-multi-format-c-abi.md` — the original format-shape survey flagging MPEG-TS as
  incompatible with the shared handles
- `adr/0005-flv-c-abi.md` — the out-buffer-per-call mux shape this ADR reuses for
  `write_pat_pmt`/`write_access_unit`
- `crates/mediaway-ffi/tests/ts_container_smoke.rs` — the round-trip verification

ADRs are **English**. Numbering is local to this `adr/` folder.
