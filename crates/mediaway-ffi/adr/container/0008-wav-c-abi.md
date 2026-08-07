# ADR-0008: WAV (RIFF/WAVE PCM) container C ABI (consuming finish, one-shot parse)

- **Status**: Accepted
- **Date**: 2026-08-07
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi` (container module)

## Context

`adr/0003-multi-format-c-abi.md`'s format-shape survey flagged WAV as the last of the four
formats deliberately kept outside `mediaway-container`'s shared `Mux`/`Demux` traits — its
own module docs say RIFF chunk sizes must be known up front, so it exposes a whole-buffer
shape (`push_packet`/`finish`, `parse`) rather than the incremental push/poll traits, which
this format cannot honestly satisfy. This ADR closes the container C ABI expansion series
(`adr/0004` through `adr/0007`) by giving WAV its own dedicated shape — the last of all 8
`mediaway-container` formats to reach the C ABI.

### Why WAV doesn't fit any existing C ABI shape — including the other dedicated ones

- `Muxer::push_packet(&mut self, packet: &Packet)` is **infallible** (no `Result` at all) —
  every other mux handle in this crate can fail per push.
- `Muxer::finish(self) -> Vec<u8>` **consumes `self` by value** — a genuinely new shape none
  of the other 7 formats have. RIFF's `RIFF`/`data` chunk sizes are written into the header
  once, at the end, when the total PCM length is finally known; there is no incremental
  `poll_bytes` because there is nothing valid to emit until the very last byte is in.
- `parse(data: &[u8]) -> Result<(StreamInfo, Packet), Error>` is a **free function**, not a
  method on any struct — WAV demux has no streaming state to hold at all (no `push_bytes`
  buffer, no `poll_packet` loop). Every other format in this crate, including the four
  already-dedicated ones (Ogg/ADTS/FLV/MPEG-TS/MP3), still has *some* demuxer struct.

## Decision

> Add `mediaway_wav_muxer_t` (mux side only — demux is not a handle) and one free function,
> `mediaway_wav_parse`. The muxer handle holds `Option<wav::Muxer>` so
> `mediaway_wav_muxer_finish` can [`Option::take`] the inner value, consuming it exactly
> once; a second `finish` call (or any `push_packet` after one) fails with
> `MEDIAWAY_STATUS_INVALID_STATE` instead of panicking on an already-moved value.
> `mediaway_wav_parse` takes a complete buffer and writes both a `mediaway_stream_info_t`
> and a `mediaway_packet_t` in one call — no handle, no `_create`/`_close` pair on the demux
> side at all.

### 1. `Option<wav::Muxer>` inside the handle, not an enum or typestate

Every prior format either has no state machine (Ogg/ADTS/FLV/MPEG-TS/MP3: always "live") or
an explicit `Open`/`Live` enum (MP4/WebM). WAV needs a third shape: "live, then consumed."
`Option::take` is the simplest Rust idiom for "this value is used exactly once, and every
access after that is a caller error" — matches this crate's poisoned-handle convention
(check a flag, refuse the call) without inventing a new typestate enum for a two-state
lifecycle that doesn't need one.

### 2. `mediaway_wav_muxer_close` is still required after `finish`

`finish` frees the *inner* `wav::Muxer` (via `Option::take`), not the *handle* `Box` itself
— the opaque pointer is still valid and must go through `mediaway_wav_muxer_close` like
every other handle in this crate, even though calling `push_packet`/`finish` on it again
after a successful `finish` now always fails. This keeps the alloc/free pairing uniform
across every muxer in the header (one `_create`, one `_close`, no exceptions) rather than
making WAV's `finish` implicitly free the handle too — a caller checking for handle-free
consistency across formats does not need a WAV-specific exception.

### 3. `mediaway_wav_parse` has no handle at all, unlike every other demux side

Reusing `mediaway_wav_demuxer_t` for a function that holds no state across calls would be
pure ceremony — `_create`/`_close` with nothing to allocate or free between them. A free
function matches the Rust API's own shape (`pub fn parse(data: &[u8]) -> Result<...>`)
exactly, and is honest about there being no streaming demux state to manage.

### 4. Status codes: `InvalidState` reused, `wav::Error` collapses entirely to `InvalidData`

`push_packet`/`finish` on an already-finished muxer reuse the same
`MediawayStatus::InvalidState` ADR-0001 defined for MP4/WebM's `Open`/`Live` typestate
violations — identical situation (a call made in a state that structurally can't accept it).
`riff_wave_core::Error`'s four variants (`NotRiffWave`, `MissingFmtChunk`,
`TruncatedFmtChunk`, `UnsupportedFormatTag`) are all parse-level, with no MP4-shaped
`InvalidTrack`/`InvalidPacket` equivalent — every variant collapses to `InvalidData`, same
posture as Ogg's `From<ogg::Error>`.

### 5. `mediaway_wav_sample_format_t`, not `mediaway_sample_format_t`

A real naming collision was caught during implementation: `common.h` already defines
`mediaway_sample_format_t` (`S16`/`S32`/`F32` — raw PCM bit depth for device/pipeline audio
capture, unrelated to WAV's container format). WAV's format concept is the RIFV `fmt`
chunk's `wFormatTag` (`Pcm`/`Float`) — a different axis entirely (bit depth vs. encoding
family). Named `mediaway_wav_sample_format_t` to avoid redefining an existing C enum, which
`gcc -fsyntax-only` on the header caught immediately as a hard conflicting-redefinition
error.

### 6. ABI version bump

`MEDIAWAY_CONTAINER_FFI_ABI_VERSION` `6 -> 7` — the last bump in this ADR series; all 8
`mediaway-container` formats are now reachable from the C ABI.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| A `mediaway_wav_demuxer_t` handle wrapping nothing, for API-shape consistency with the other 7 formats | Would add `_create`/`_close` ceremony around a function that holds no state between calls — the Rust `parse` free function has no handle to wrap in the first place |
| `finish` frees the handle itself (no separate `close` needed after) | Breaks the "every handle in this header has one `_create`, one `_close`" invariant a caller can currently rely on uniformly — one exception format is worse than one extra `_close` call |
| Reuse `mediaway_sample_format_t` from `common.h` for `sample_format` | Different real-world concept (device audio bit depth vs. WAVE `wFormatTag`) that happened to share a plausible name — `gcc -fsyntax-only` caught the conflict as a hard compile error, confirming they must stay separate types |

## Consequences

### Positive

- WAV (integer and IEEE-float PCM, the still-in-use real-world case for uncompressed audio)
  is now reachable from C/C++/C#/Python/Node.
- **All 8 `mediaway-container` formats** (`mp4`, `webm`, `ogg`, `adts`, `flv`, `ts`, `mp3`,
  `wav`) are now reachable from the C ABI — closes the series `adr/0003` through `adr/0008`
  started.
- Verified end-to-end: `tests/wav_container_smoke.rs` round-trips a PCM stereo frame and a
  float-format mono frame through mux → `parse`, plus a second-`finish`-fails case and a
  non-RIFF/WAVE-data rejection case.

### Negative / Trade-offs

- No language binding (C++/C#/Python/Node) wiring in this pass — same scoping as every ADR
  in this series, now applying to all 8 formats at once as a follow-up.
- A caller unfamiliar with the consuming-`finish` shape could plausibly expect
  `push_packet` to work again after `finish` (as it does for every other mux handle's
  `poll_bytes`/`flush`) — mitigated by returning `MEDIAWAY_STATUS_INVALID_STATE` explicitly
  rather than silently no-opping or corrupting state, and documented in both the header and
  Rust doc comments.

## References

- `crates/mediaway-container/src/wav.rs` — the format module's actual method shape (source
  of truth), including the "why this exposes the core's whole-buffer shape directly" module
  doc rationale
- `crates/mediaway-container/src/wav_tests.rs` — reference mux/parse round trip this ADR's
  own FFI test payload was derived from
- `crates/riff-wave-core/src/types.rs` — `WaveFormat`/`SampleFormat` definitions this ADR's
  `mediaway_wave_format_t`/`mediaway_wav_sample_format_t` mirror
- `adr/0003-multi-format-c-abi.md` — the original format-shape survey flagging WAV as
  incompatible with the shared handles
- `crates/mediaway-ffi/include/mediaway/common.h` — the pre-existing, unrelated
  `mediaway_sample_format_t` this ADR's naming avoids colliding with
- `crates/mediaway-ffi/tests/wav_container_smoke.rs` — the round-trip verification

ADRs are **English**. Numbering is local to this `adr/` folder.
