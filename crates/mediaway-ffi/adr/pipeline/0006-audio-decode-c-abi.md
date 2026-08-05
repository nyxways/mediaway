# ADR-0006: Opus audio decode C ABI + Opus wired into the existing audio encode C ABI

- **Status**: Accepted
- **Date**: 2026-08-05
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi`

## Context

`mediaway_sw::opus` (crate `mediaway-sw`) has shipped a real, hardware-independent
`OpusEncoder`/`OpusDecoder` pair since its own ADR-0001 — pure Rust over
`unsafe-libopus`, no system libopus. Two gaps remained between that Rust-level
implementation and this crate's C ABI:

1. **Encode side already had a real backend, just not reachable from C.**
   `mediaway_encoder::SwOpusAudioEncoder` implements the `AudioEncoder` trait
   (`crates/mediaway-encoder/src/audio/sw_opus.rs`) and `mediaway::platform`'s own
   `encoder_support(CodecKind::Opus)` probe already dispatches to it — but this
   crate's `pipeline::audio::open_audio_encoder`/`rust_config`
   (`adr/0003-auto-audio-encode-c-abi.md`) only ever built `Aac` configs and only
   ever opened `WindowsAudioEncoder`. A C caller could not reach Opus encode at all,
   despite the Rust implementation being real and hardware-verifiable.
2. **No decode surface existed at all.** `mediaway_sw::opus::OpusDecoder` mirrors
   `WindowsVideoDecoder`'s (`mediaway-decoder/src/windows/wmf/opus.rs`,
   `WmfOpusDecoder`) `push_packet`/`poll_frame`/`flush` shape deliberately — its own
   module docs say so — but there is no `AudioDecoder` trait yet in
   `mediaway-decoder` to unify them behind, and this crate's `pipeline` module had
   zero audio-decode C ABI (`adr/0004-auto-decode-c-abi.md` only covers video
   decode).

Motivating use case: a real-time voice-chat pipeline (mic capture → AEC/NS/AGC2 →
Opus encode → network → jitter buffer → Opus decode with packet-loss concealment →
playback) needs both directions reachable from a non-Rust host (e.g. a Unity/C#
game client). The network transport and jitter/reorder/FEC logic stay
application-side — not a media-library concern, same reasoning `mediaway`'s own
scope boundary already draws elsewhere — but Opus encode/decode themselves belong
in this crate alongside every other codec this facade already wraps.

## Decision

> Wire `CodecKind::Opus` into the existing audio encode C ABI (mechanical, no new
> types). Add `mediaway_audio_decode_session_t` to `pipeline.h` (ABI v5): single-step
> open (mirrors `mediaway_decode_session_t`'s video shape — no muxer to wire),
> `push_packet`/`poll_frame`/`flush`/`close`, wraps `mediaway_sw::opus::OpusDecoder`
> **directly** (no trait object), Opus-only v1.

### 1. Encode side — no new types, one dispatch branch

`pipeline::audio::rust_config` gains one match arm
(`MediawayPipelineCodecKind::Opus => CodecKind::Opus`) and
`open_audio_encoder` gains one unconditional branch checked before the existing
`#[cfg(windows)]` dispatch:

```rust
fn open_audio_encoder(config: &AudioEncoderConfig) -> Result<Box<dyn AudioEncoder>, EncodeError> {
    if config.codec == CodecKind::Opus {
        return Ok(Box::new(mediaway_encoder::SwOpusAudioEncoder::open(config)?));
    }
    #[cfg(windows)]
    { /* existing WindowsAudioEncoder AAC path, unchanged */ }
    #[cfg(not(windows))]
    { /* existing NoBackend fallback, unchanged */ }
}
```

Unconditional (no `#[cfg(windows)]`) because `mediaway-sw` is pure Rust,
cross-platform — the one real difference from the AAC path this crate already
wraps. `mediaway_audio_encode_config_opus(sample_rate, channels, time_base)` sugar
constructor added alongside the existing `mediaway_audio_encode_config_aac`, taking
`channels` as a parameter (unlike the AAC sugar's hardcoded stereo) since Opus voice
use is commonly mono. No change to `MediawayAudioEncodeConfig`'s field shape, no ABI
break on the existing struct — `codec`/`sample_format` were already general-purpose
fields, only the AAC-only dispatch was narrow.

### 2. Decode side — concrete type, not a trait object

```rust
pub struct AudioDecodeSessionHandle {
    poisoned: bool,   // push_packet/poll_frame are repeated-call APIs — same guard
                       // as DecodeSessionHandle (video decode, adr/0004 §2).
    inner: mediaway_sw::opus::decoder::OpusDecoder, // NOT Box<dyn AudioDecoder>
}
```

`DecodeSessionHandle` (video) holds `Box<dyn VideoDecoder>` because `VideoDecoder`
is a real trait with multiple real backends (Windows/Linux) to select between at
runtime. No `AudioDecoder` trait exists in `mediaway-decoder` — inventing one now,
crossing into that crate, purely to give this FFI module a `Box<dyn Trait>` to hold
would be an abstraction over a backend set of exactly one (Opus). Per this
workspace's simplicity rule ("no abstractions for one-off code"), the handle wraps
`OpusDecoder` concretely. If/when a second real audio-decode backend appears (e.g.
the already-real-but-unwired `WmfOpusDecoder`, or a future AAC decoder), that is the
point to design an `AudioDecoder` trait in `mediaway-decoder` and switch this handle
to `Box<dyn AudioDecoder>` — a mechanical follow-up, not a breaking C ABI change
(the opaque handle's C-visible shape does not change).

### 3. Config — Opus only, no `extra_data`

```c
typedef struct mediaway_audio_decode_config {
    mediaway_pipeline_codec_kind_t codec; /* Opus only today */
    uint32_t sample_rate;
    uint16_t channels;
    mediaway_rational_t time_base; /* frame duration; also decode buffer's sample cap */
} mediaway_audio_decode_config_t;
```

No `extra_data` field (unlike `mediaway_auto_video_decode_config_t`) — Opus carries
no out-of-band codec config comparable to AVCC/SPS-PPS;
`opus_decoder_create` needs only `sample_rate`/`channels`. Output PCM is always
`F32` (`opus_decode_float`) — no `sample_format` field to mismatch, unlike
`mediaway_audio_encode_config_t`.

### 4. Packet input reused, frame output new

`push_packet` reuses the existing `mediaway_decode_packet_view_t`
(`adr/0004-auto-decode-c-abi.md` §4) unchanged — it is already a generic
`Packet`-shaped borrow (`stream_id`/`pts`/`dts`/`duration`/`is_keyframe`/
`is_discard`/`payload`), not video-specific despite living in the "video decode"
ADR section. An empty `payload` (`NULL`/`len == 0`) is not rejected — it is Opus's
packet-loss-concealment hint, passed straight through to `unsafe-libopus` exactly as
`OpusDecoder::push_packet`'s own doc specifies. This directly covers the lost-frame
case a real-time voice pipeline needs; sequence tracking/reordering/jitter buffering
to decide *when* to feed an empty payload stays application-side.

`poll_frame`'s output is a new, owned `mediaway_decoded_audio_frame_t` — same
naming precedent as `mediaway_decoded_video_frame_t` vs. `mediaway_video_frame_t`
(owned decode output vs. borrowed encode input are always distinct types in this
crate, even when field-similar to `mediaway_audio_frame_view_t`).

### 5. Status codes — no new variants

`OpusError` maps onto the existing `DECODER_BACKEND_FAILURE`/`DECODER_CLOSED`/
`INVALID_INPUT` (`Backend`/`Closed`/everything else respectively) — the same three
generic buckets `adr/0004-auto-decode-c-abi.md` §6 already established for decode
errors. No `OPUS_`-specific status needed.

### 6. ABI version

`MEDIAWAY_PIPELINE_FFI_ABI_VERSION` bumps 4 → 5 (new exported symbols; pre-1.0, no
stability promise).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Add an `AudioDecoder` trait to `mediaway-decoder` now, wrap `Box<dyn AudioDecoder>` | Real architecture work crossing into another crate for a backend set of exactly one today — no second implementation exists to justify the abstraction yet (§2) |
| New `mediaway_opus_decode_packet_view_t` distinct from the video decode one | `mediaway_decode_packet_view_t` is already codec-agnostic (generic `Packet` fields); a second, field-identical type would just be header bloat with no ownership/module-boundary reason (unlike `adr/0004` §4's video-vs-container split, which *does* have such a reason) |
| Two-step open (`mediaway_auto_audio_decoder_open` → session), mirroring nothing that exists on the decode side | No muxer-wiring stage to justify it — same reasoning `adr/0004` §2 already used for video decode and `adr/0003` used for audio encode |
| Ship the network jitter buffer / sequence tracking in this crate too | Not a media-codec concern — every other capability this facade wraps stops at codec/container boundaries; sequencing/loss-recovery policy is an application decision, not something `unsafe-libopus`/`mediaway-sw` has an opinion on |

## Consequences

### Positive

- Closes the Opus decode gap this session's own investigation found: `OpusDecoder`
  was real Rust with zero C reach.
- Opus encode, previously reachable only via `mediaway::platform::encoder_support`'s
  capability *probe* (not a usable session) from C, is now actually openable and
  usable end-to-end from C — a real, not just probed, capability.
- Empty-payload PLC passthrough covers a real-time voice client's most common
  lost-packet case with no extra API surface.

### Negative / Trade-offs

- `AudioDecodeSessionHandle` wrapping a concrete type instead of a trait object is a
  deliberate, documented inconsistency with `DecodeSessionHandle` (video) — flagged
  here so it isn't mistaken for an oversight during a future audio-decode backend
  addition.
- No jitter buffer / FEC-scheduling / sequence-loss-detection surface — a real-time
  voice client still needs to build or bring its own on top of this session, same as
  it already needs its own network transport.

## References

- [`crates/mediaway-sw/adr/opus/0001-unsafe-libopus-encode-decode.md`](../../../mediaway-sw/adr/opus/0001-unsafe-libopus-encode-decode.md) — `OpusEncoder`/`OpusDecoder`
- [`crates/mediaway-encoder/src/audio/sw_opus.rs`](../../../mediaway-encoder/src/audio/sw_opus.rs) — `SwOpusAudioEncoder`
- [`adr/0003-auto-audio-encode-c-abi.md`](0003-auto-audio-encode-c-abi.md) — audio encode C ABI this extends
- [`adr/0004-auto-decode-c-abi.md`](0004-auto-decode-c-abi.md) — video decode C ABI this decode surface mirrors
- [`docs/ai/wiki/license/sw-opus.md`](../../../../docs/ai/wiki/license/sw-opus.md), [`docs/ai/wiki/audio/apm.md`](../../../../docs/ai/wiki/audio/apm.md) — wider Opus/voice-pipeline context

ADRs are **English**. Numbering is local to this `adr/` folder.
