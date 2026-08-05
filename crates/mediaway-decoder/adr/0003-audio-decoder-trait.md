# ADR-0003: `AudioDecoder` streaming trait

- **Status**: Accepted
- **Date**: 2026-08-05
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder`

## Context

`WmfOpusDecoder` (Windows inbox `CMSOpusDecMFT`, `windows::wmf::opus`) and
`mediaway_sw::opus::decoder::OpusDecoder` (`unsafe-libopus`, cross-platform) have both
existed as real, hardware/unit-tested decode sessions since 2026-08-03, but neither
implemented a facade trait — `mediaway-decoder` only had [`VideoDecoder`]
([ADR-0001](0001-decoder-traits.md)). Both sessions' own module docs flagged this as an
explicit, deliberate gap. This ADR closes it.

## Decision

> Add `AudioDecoder` to the facade, mirroring `VideoDecoder`'s shape exactly
> (`stream_info` / `push_packet` / `poll_frame` / `flush`, same `DecodeError`). No
> `Box<dyn>` on the hot path (ADR-0001's rule applies equally here).

### Public surface

| Item | Role |
|------|------|
| `AudioDecoder` | `push_packet` → `poll_frame` → `flush`, `stream_info()` |
| `SwOpusAudioDecoder` | Wraps `mediaway_sw::opus::decoder::OpusDecoder`; `mediaway-decoder::audio::sw_opus` |
| `windows::WmfOpusDecoder` | Implements `AudioDecoder` directly (in `windows/wmf/opus.rs`) in addition to its existing inherent methods |

### Rules

1. Same direction as `VideoDecoder`: push compressed, poll uncompressed.
2. No shared `AudioDecoderConfig` this pass — unlike `VideoDecoderConfig` there is no
   audio `auto`-dispatch (`mediaway_pipeline::platform::AutoDecoder` is video-only today)
   to justify a unified config. `WmfOpusDecoder` keeps `windows::wmf::opus::OpusDecoderConfig`;
   `SwOpusAudioDecoder::open` takes `mediaway_sw::opus::config::OpusDecoderConfig` directly.
   These are two distinct types with the same field shape — not merged (documented
   limitation; revisit once an audio `auto`-dispatch path exists).
3. **File placement**: trait impl lives alongside the type it's implemented for, not in
   a separate file — same convention `VideoDecoder` already uses for `WmfH264Decoder`
   (`windows/wmf/h264.rs`) and `WindowsVideoDecoder` (`windows/mod.rs`).
4. **`WmfOpusDecoder` keeps its inherent methods** (`stream_info`/`push_packet`/
   `poll_frame`/`flush`) alongside the new trait impl — it already shipped as a public
   type with those inherent methods and existing tests calling them directly; removing
   them would be a breaking change for no benefit. The trait impl's method bodies just
   call the inherent ones (inherent methods always shadow trait methods of the same name
   in Rust's method resolution, so no infinite recursion).
5. **`SwOpusAudioDecoder` is a thin wrapper, not a direct `impl AudioDecoder for
   mediaway_sw::opus::decoder::OpusDecoder`** — even though the orphan rule permits a
   direct impl (the trait is local to this crate), this crate already has a real,
   working precedent for the exact symmetric case: `mediaway-encoder`'s
   `SwOpusAudioEncoder` (`crates/mediaway-encoder/src/audio/sw_opus.rs`) wraps
   `mediaway_sw::opus::encoder::OpusEncoder` the same way, with a private `const fn
   map_err(&OpusError) -> EncodeError`. `SwOpusAudioDecoder` mirrors that exactly
   (`map_err(&OpusError) -> DecodeError`) instead of adding a global `From<OpusError>
   for DecodeError` in `error.rs` — keeping `mediaway-sw`'s error type out of the
   facade's own error module, matching the encoder side's existing choice.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `impl AudioDecoder for mediaway_sw::opus::decoder::OpusDecoder` directly (no wrapper) | Legal (orphan rule), but breaks symmetry with the encoder facade's existing `SwOpusAudioEncoder` wrapper for the identical situation — picked consistency over one fewer type |
| `From<OpusError> for DecodeError` in `error.rs` | Would leak `mediaway-sw`'s error type into the facade's shared error module; the encoder side already chose a local `map_err` instead |
| Shared `AudioDecoderConfig` now | Speculative — no audio `auto`-dispatch exists yet to consume it (unlike video's `VideoDecoderConfig`) |

## Consequences

### Positive

- `WmfOpusDecoder` and `SwOpusAudioDecoder` are now interchangeable through one trait
  bound (`fn f<D: AudioDecoder>(d: &mut D)`), proven by a generic test helper used
  against both in `windows/wmf/opus_tests.rs` and `audio/sw_opus_tests.rs`.
- Matches `mediaway-encoder`'s `AudioEncoder`/`SwOpusAudioEncoder` shape exactly —
  encode and decode facades now read the same way for Opus.

### Negative / Trade-offs

- Two distinct `OpusDecoderConfig` types remain (`windows::wmf::opus::OpusDecoderConfig`
  vs. `mediaway_sw::opus::config::OpusDecoderConfig`) — callers switching backends must
  translate manually until a shared config lands.
- No audio equivalent of `WindowsVideoDecoder`'s `Backend` enum dispatch — Windows still
  has exactly one audio decode path (Opus via WMF), so there is nothing to switch over.

## References

- `VideoDecoder` trait: [ADR-0001](0001-decoder-traits.md)
- Encoder-side precedent: `mediaway-encoder` `adr/0001-encoder-traits.md`,
  `crates/mediaway-encoder/src/audio/sw_opus.rs`
- `mediaway-sw::opus`: `crates/mediaway-sw/adr/opus/0001-unsafe-libopus-encode-decode.md`
