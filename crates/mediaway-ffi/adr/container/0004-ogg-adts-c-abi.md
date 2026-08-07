# ADR-0004: Ogg + ADTS container C ABI (dedicated single-stream handles)

- **Status**: Accepted
- **Date**: 2026-08-07
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi` (container module)

## Context

`adr/0003-multi-format-c-abi.md` closed the WebM gap by extending the existing
`mediaway_muxer_t`/`mediaway_demuxer_t` handles, and explicitly deferred Ogg and ADTS to a
follow-up: both are single-implicit-stream formats with **no track-registration step and no
`Open`/`Live` typestate** (`ogg::Muxer::new(serial)` / `adts::Muxer::new(sample_rate,
channels)` are immediately ready for `push_packet`), so they do not fit the shared handles'
shape at all — see that ADR's format-shape table.

This ADR implements both in one pass: after reading `mediaway-container::ogg`/`::adts`
directly, their method shapes turned out to be close enough (single-stream, no typestate,
`push_packet`/`flush`/`poll_bytes` on the mux side, `push_bytes`/`streams`/`poll_packet` on
the demux side) that splitting them into two ADRs would duplicate the same rationale twice.
The only real divergence is construction: `ogg::Muxer::new` is infallible (`const fn`);
`adts::Muxer::new` returns `Result<Self, Error>` for a non-standard sample rate.

## Decision

> Add four new dedicated opaque handle types — `mediaway_ogg_muxer_t`,
> `mediaway_ogg_demuxer_t`, `mediaway_adts_muxer_t`, `mediaway_adts_demuxer_t` — each with
> its own `_create`/`_close` plus format-appropriate mux (`push_packet`/`flush`/
> `poll_bytes`) or demux (`push_bytes`/`stream_count`/`stream_at`/`poll_packet`) functions.
> Reuses `mediaway_packet_view_t`/`mediaway_packet_t`/`mediaway_stream_info_t` and the
> existing shared frees (`mediaway_buffer_free`/`mediaway_packet_free`/
> `mediaway_stream_info_free`) — those are already codec/format-agnostic.

### 1. Dedicated handles, not new `MuxerState`/`DemuxerState` variants

Unlike WebM (which slotted into the existing `MuxerState`/`DemuxerState` enums because it
shares MP4's exact shape), Ogg/ADTS have no `add_track`/`begin` step at all — adding them as
enum variants would force every existing MP4/WebM match arm to grow an
`Err(InvalidState)`-only case for a state that structurally cannot occur, and would put
MP4/WebM-only functions (`add_video_track`, `set_decryption_key`, ...) in a position where
they *look* callable on an Ogg/ADTS handle but always fail. Separate C types make the
mismatch a compile-time error for any C/C++ caller, not a runtime status code to check.

### 2. `mediaway_adts_muxer_create` has no status side channel

`adts::Muxer::new` can fail (`Error::UnsupportedSampleRate`), but this constructor's return
type is `mediaway_adts_muxer_t *` — there is no `mediaway_status_t` slot to report *why*
construction failed, only whether it succeeded (non-null) or not (null). This matches the
existing precedent `mediaway_demuxer_create_for_format` already set (null covers both "bad
`format` argument" and "caught panic"): a constructor-only failure mode, not surfaced as a
distinct code. A caller that needs to know *why* should validate `sample_rate` against
`adts-core`'s documented standard-rate table before calling.

### 3. `mediaway_ogg_muxer_flush`/`mediaway_adts_muxer_flush` are no-ops, kept anyway

Both cores' `flush()` do nothing (`ogg::Muxer::push_packet` always emits a complete page;
`adts::Muxer::push_packet` always emits a complete frame) — but the function is still
exposed for API shape parity with `mediaway_muxer_flush`, so a caller writing
format-generic-looking mux code does not need a special case to skip calling it.

### 4. No new `MediawayStatus` variants

`status.rs` already had `From<ogg::Error>`/`From<adts::Error>` mappings in place
(collapsing to `InvalidData`/`InvalidPacket`) before this ADR — added preemptively during
`adr/0003-multi-format-c-abi.md`'s work as unused-until-now groundwork. No new status codes
were needed.

### 5. ABI version bump, and a drift bug fixed alongside

`MEDIAWAY_CONTAINER_FFI_ABI_VERSION` went `1 -> 2` (Ogg) `-> 3` (ADTS, same pass). Along the
way, `mediaway_container_ffi_abi_version()` (the Rust runtime counterpart a
dynamically-loaded consumer calls) was found still hardcoded to return `0` — it had never
been bumped when ADR-0003 moved the header macro `0 -> 1`. Fixed to `3` here, alongside this
pass's own two bumps, rather than filed separately, since both edits land in the same
`mod.rs` line.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Fold Ogg/ADTS into `mediaway_muxer_t`/`mediaway_demuxer_t` via more `MuxerState`/`DemuxerState` variants | Every MP4/WebM-only function would need an `Err(InvalidState)` arm for two variants that can never legally reach it (no typestate to violate — the "state" is just "always live"), and the enum would mix genuinely incompatible construction signatures (`new()` vs `new(serial)` vs `new(rate, channels)` vs fallible) behind one `_create_for_format(format)` entry point that cannot express per-format constructor arguments anyway |
| A `mediaway_status_t` out-parameter on `mediaway_adts_muxer_create` | Every other `_create`/`_create_for_format` constructor in this crate already reports failure as `NULL` only (`mediaway_muxer_create_for_format`, `mediaway_demuxer_create_for_format`) — adding a status out-param to exactly one constructor would be an inconsistent, one-off API shape for a single failure mode a caller can avoid entirely by validating the sample rate first |
| Split into two ADRs (Ogg, then ADTS) | The two formats' C ABI shapes and every design question above are close enough that a second ADR would restate this one's reasoning nearly verbatim; combined here, split only where they actually differ (§2) |

## Consequences

### Positive

- Ogg (Opus/Vorbis transport) and ADTS (raw AAC elementary stream) are now reachable from
  C/C++/C#/Python/Node — previously reachable only by depending on the Rust
  `mediaway-container` crate directly.
- Fixes a real, separate, already-shipped-but-unreachable-detection bug: the runtime ABI
  version function had drifted from the header macro since ADR-0003.
- Verified end-to-end: `tests/ogg_adts_container_smoke.rs` round-trips a real `OpusHead`
  identification header plus one Opus audio packet through the Ogg handles, two raw AAC
  frames through the ADTS handles (payload, pts/duration synthesis, and stream info all
  checked), plus a rejected-construction case for a non-standard ADTS sample rate.

### Negative / Trade-offs

- 4 of 8 formats (`mp4`, `webm`, `ogg`, `adts`) are now reachable from the C ABI; FLV,
  MPEG-TS, MP3, WAV remain Rust-only (see `adr/0003-multi-format-c-abi.md`'s Deferred
  section for their planned bespoke shapes).
- No language binding (C++/C#/Python/Node) wiring in this pass — C ABI (Rust FFI crate +
  hand-written header) only, same scoping as ADR-0003.

## References

- `crates/mediaway-container/src/{ogg,adts}.rs` — the two format modules' actual method
  shapes (source of truth)
- `crates/mediaway-container/src/{ogg_tests,adts_tests}.rs` — reference mux/demux round
  trips this ADR's own FFI test payloads were derived from
- `adr/0003-multi-format-c-abi.md` — WebM's C ABI extension; the format-shape table and the
  original deferral of Ogg/ADTS to this ADR
- `crates/mediaway-ffi/tests/ogg_adts_container_smoke.rs` — the round-trip verification

ADRs are **English**. Numbering is local to this `adr/` folder.
