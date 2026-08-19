# ADR-0001 (web): WebCodecs audio decode — first audio surface in `mediaway-decoder::web`, exercised via Opus

- **Status**: Accepted — implemented, wasm32 compile-verified only (no real browser runtime
  available in this environment)
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (`web` module)

## Context

- `mediaway-decoder::web` currently has **zero audio decode capability of any kind** —
  video-only (`is_webcodecs_video_decode_supported`, `decode_video_chunks ->
  DecodedVideoFrames`). No AAC, no Opus, nothing (confirmed by grep this session: zero
  `"audio"`/`"Audio"` matches anywhere under `crates/mediaway-decoder/src/web/`, and zero
  `feature = "audio"` gates in that module). This is a stronger gap than "Opus is missing" —
  audio decode was never added here at all, unlike the encoder side which at least has an
  AAC-only smoke path.
- Like the encoder side, this module does **not** implement the facade `AudioDecoder` trait
  (crate root ADR-0003) — `docs/ai/wiki/decode/web-video-decode.md` already states this
  explicitly for `VideoDecoder`: `WebCodecs` sessions are inherently async/callback-driven,
  incompatible with the facade's sync `push_packet`/`poll_frame` shape. The same reasoning
  applies to `AudioDecoder`.
- `@mediaway/browser`'s `DecodeSession` (workspace ADR-0022) already decodes Opus
  generically today at the TypeScript level (native `AudioDecoder`, codec resolved per track,
  channel count derived from an `OpusHead` `CodecPrivate` payload) — a separate,
  deliberately independent layer (ADR-0020's "WASM owns container, host owns codecs" split),
  referenced here only for context, out of this ADR's scope.
- Verified against the pinned `web-sys` 0.3.104 source (`gen_AudioDecoderConfig.rs`):
  `AudioDecoderConfig::new(codec, number_of_channels, sample_rate)` — the **same**
  `(codec, channels, sample_rate)` order as the encoder-side `AudioEncoderConfig::new`,
  confirmed by reading the binding source directly rather than assumed from the encoder-side
  fix (`web-real-chrome-bugs.md` bug #1 was specifically an argument-order swap bug, so this
  was verified independently, not copied by analogy).
- `EncodedAudioChunkType` exists in `web-sys` 0.3.104 with the same `Key`/`Delta` shape as
  `EncodedVideoChunkType`.
- `AudioData` (decoder output) exposes `format()` / `sample_rate()` / `number_of_frames()`
  / `number_of_channels()` / `allocation_size(...)` / `copy_to_with_u8_slice(...)` /
  `close()` — the audio-side counterpart of `VideoFrame`'s `allocationSize`/`copyTo`/`close`
  already used by `decode_video_chunks`'s `read_luma_plane` helper.
- Sibling ADR ([`mediaway-encoder/adr/web/0001`](../../../mediaway-encoder/adr/web/0001-webcodecs-opus-audio-encode.md))
  covers the encode-side generalization and the `iso-bmff` MP4-Opus-sample-entry gap; that
  gap means a real Opus **encode -> mux -> demux -> decode** fMP4 round trip is not
  available yet either, which bounds this ADR's own test plan (see Open Questions).

## Decision

> Add the first audio decode surface to `mediaway-decoder::web`:
> `is_webcodecs_audio_decode_supported` + `decode_audio_chunks -> DecodedAudioData`,
> generalized over `codec` from the start (not Opus-hardcoded) — mirroring
> `decode_video_chunks`'s existing codec-parameterized shape — and exercise it first with
> Opus (the gap this ADR closes), leaving AAC decode reachable through the same function
> without further Rust changes.

### Public surface (`mediaway-decoder::web`, both `wasm.rs` and its `host.rs` stub)

| Item | Role |
|---|---|
| `is_webcodecs_audio_decode_supported(codec, channels, sample_rate) -> bool` | Mirrors `is_webcodecs_video_decode_supported` |
| `decode_audio_chunks(codec, channels, sample_rate, description, chunk_data, chunk_offsets, chunk_lengths, chunk_timestamps_us) -> Result<DecodedAudioData, JsValue>` | Mirrors `decode_video_chunks`'s flattened parallel-array shape crossing the wasm-module boundary (no `chunk_is_key` param — see Rules) |
| `DecodedAudioData` (new type, new `audio_frames.rs` sibling to `frames.rs`) | `chunk_count`, `timestamp_us(i)`, `sample_count(i)`, `channel_count(i)`, `samples(i) -> Vec<f32>` |

### Rules

1. **`codec` is caller-supplied from the start (never hardcoded)** — Opus is simply the
   first codec exercised (`tools/e2e-web`); AAC decode becomes reachable "for free" through
   the same function once a caller wants it, with no separate `decode_aac_chunks`.
2. **`chunk_is_key` is dropped** from the audio version of the parallel-array parameter list
   — `WebCodecs`' `EncodedAudioChunkType` exists (`Key`/`Delta`), but Opus/AAC packets are
   independently decodable per-packet in practice; every constructed `EncodedAudioChunk`
   uses `EncodedAudioChunkType::Key`. Document this simplification in rustdoc on
   `decode_audio_chunks`; revisit only if a real codec needs `Delta` chunks.
3. **Sample readback trusts the browser's reported format** — copy via
   `AudioData::copy_to_with_u8_slice`/`allocation_size` using the frame's own `format()`
   (no forced resample/format conversion), the same "de-stride using the browser's own
   reported layout" posture `read_luma_plane` already uses for `VideoFrame`'s
   `PlaneLayout`, adapted to `AudioData`'s planar-vs-interleaved distinction. The exact
   bytes-to-`f32` interpretation (one `copyTo` per channel for planar formats, per the
   `WebCodecs` spec's `AudioData.copyTo(planeIndex, …)` shape) is implementation detail for
   the actual change, not fixed by this ADR beyond "trust the reported format" (see Open
   Questions).
4. **`AudioData::close()` after every readback** — same hygiene convention as
   `frame.close()` in `read_luma_plane`.
5. **Host-target stub (`host.rs`) mirrors both new exports** with the existing
   `"wasm32 browser only"` error convention.
6. **No shared `AudioDecoderConfig` Rust type introduced here** — mirrors `mediaway-decoder`
   root ADR-0003 rule 2's existing precedent (no unifying config without an audio
   `auto`-dispatch to justify one); this module keeps using `web_sys::AudioDecoderConfig`
   directly, the same way `decode_video_chunks` uses `web_sys::VideoDecoderConfig` directly.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `decode_opus_chunks` (Opus-only, hardcoded codec string) | Repeats the exact mistake being avoided on the encoder side; `decode_video_chunks` already proves the generalized shape costs nothing extra |
| Implement via the facade `AudioDecoder` trait (root ADR-0003) | Same async/callback-vs-sync-poll mismatch already documented for `VideoDecoder`; would need a fundamentally different buffering/polling adapter design — out of scope |
| Skip Rust-crate-level audio decode entirely, point users at `@mediaway/browser` | Leaves `mediaway-decoder::web` permanently audio-blind and `tools/e2e-web`'s raw-wasm harness unable to prove any audio decode round trip at all |

## Consequences

### Positive

- Closes a real, total gap (zero audio decode existed) rather than only adding Opus to an
  existing surface.
- Symmetric with `decode_video_chunks`'s already-established codec-generalized shape; costs
  nothing extra to also unlock AAC decode later.

### Negative / Trade-offs

- New wasm-module-boundary type (`DecodedAudioData`) and cross-boundary flattening logic to
  write and test, mirroring `DecodedVideoFrames`'s existing complexity.
- No `EncodedAudioChunkType::Delta` support (rule 2) — a real limitation if a future codec
  needs it.

## Open Questions

- Exact `AudioData` readback shape (interleaved vs. planar, per-channel `copyTo` calls) —
  deferred to implementation; `WebCodecs`' `AudioData.copyTo` takes a `planeIndex` and
  copies one plane per call, so a genuinely planar multi-channel decode needs one call per
  channel. Needs confirming against real Chrome, not just the IDL, given this crate family's
  history of real-browser-only bugs (`web-real-chrome-bugs.md`).
- Should the sibling encode ADR's mux caveat (`iso-bmff` writes `mp4a`/`esds` for
  `Codec::Opus`, not a real `Opus`/`dOps` box) block a full encode->mux->demux->decode Opus
  round trip test, or is `EncodedAudioChunk`-level-only round trip (no container) acceptable
  for this pass? Recommend the latter (matches the encoder-side ADR's own scope decision),
  flagged here since it affects this crate's test plan too.

## References

- `mediaway-decoder` root ADR-0001 (`VideoDecoder`), ADR-0003 (`AudioDecoder` — the facade
  trait this wasm module deliberately does not implement)
- [`web-video-decode.md`](../../../../docs/ai/wiki/decode/web-video-decode.md) — existing
  `decode_video_chunks` shape + the async/sync-mismatch reasoning already stated for video
- Sibling ADR: `crates/mediaway-encoder/adr/web/0001-webcodecs-opus-audio-encode.md`
- [`docs/adr/0022-browser-decode-session-and-device-dx.md`](../../../../docs/adr/0022-browser-decode-session-and-device-dx.md) /
  [`web-decode-session.md`](../../../../docs/ai/wiki/decode/web-decode-session.md) —
  `@mediaway/browser`'s existing generic Opus decode (separate layer)

ADRs are **English**. Numbering is local to this `adr/web/` folder.
