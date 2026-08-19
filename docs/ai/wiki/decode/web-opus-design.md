# Web: Opus audio decode — first audio decode surface (implemented)

- ADR: [`crates/mediaway-decoder/adr/web/0001-webcodecs-opus-audio-decode.md`](../../../crates/mediaway-decoder/adr/web/0001-webcodecs-opus-audio-decode.md)
  (implemented 2026-08-19). Sibling encode ADR: [`encode/web-opus-design`](../encode/web-opus-design.md).
- **Verification**: `cargo build -p mediaway-decoder --target wasm32-unknown-unknown` compile-
  verified only — no real browser runtime in this environment. The planar-vs-interleaved
  `AudioData` readback byte layout (see below) is genuinely unverified against real Chrome.

## What was there before this change

`mediaway-decoder::web` had **zero audio decode of any kind** — video-only
(`is_webcodecs_video_decode_supported`, `decode_video_chunks -> DecodedVideoFrames`). Not
"AAC-only like the encoder side" — genuinely no audio surface at all (confirmed by grep:
zero `"audio"`/`"Audio"` matches, zero `feature = "audio"` gates in this module). Like the
video decode path, this module does not implement the facade `AudioDecoder` trait
(root ADR-0003) — same async/callback vs. sync-poll mismatch already documented for
`VideoDecoder`.

## Key findings from this design pass

- `AudioDecoderConfig::new(codec, number_of_channels, sample_rate)` confirmed directly from
  `web-sys` 0.3.104's `gen_AudioDecoderConfig.rs` (not assumed by analogy with the
  encoder-side fix, since that fix was specifically about a swapped-argument-order bug).
- `EncodedAudioChunkType` exists with the same `Key`/`Delta` shape as
  `EncodedVideoChunkType`; `AudioData` exposes `format()`/`allocation_size(...)`/
  `copy_to_with_u8_slice(...)`/`close()` — the audio counterpart of `VideoFrame`'s
  `allocationSize`/`copyTo`/`close` already used by `read_luma_plane`.
- `@mediaway/browser`'s `DecodeSession` already decodes Opus generically today (native
  `AudioDecoder`, `numberOfChannels` derived from `OpusHead`) — separate layer (ADR-0022),
  unaffected by this crate-level gap.

## What shipped

Added the **first** audio decode surface here: `is_webcodecs_audio_decode_supported` +
`decode_audio_chunks -> DecodedAudioData` (new type, `audio_frames.rs`), codec-parameterized
from the start (mirrors `decode_video_chunks`) — Opus is simply the first codec exercised;
AAC decode is reachable through the same function for free. Drops `chunk_is_key` from the
parallel-array params (every constructed chunk is `EncodedAudioChunkType::Key`).

Sample readback (`decode_audio_samples` in `wasm.rs`) trusts `AudioData.format()` rather than
forcing a conversion: one `copyTo` per channel for planar formats (`u8-planar`/`s16-planar`/
`s32-planar`/`f32-planar`), one call for interleaved formats, converting raw bytes to `f32`
(`pcm_bytes_to_f32`) and interleaving into a single flat buffer either way. `AudioSampleFormat`
is `#[non_exhaustive]` in `web-sys` — an unrecognized format returns an error, not a panic
(`unwrap`/`panic!` are denied outside tests in this crate).

## Open questions (unresolved by the ADR, still open after implementation)

- Exact `AudioData` readback shape — implemented per spec reading (`copyTo` takes a
  `planeIndex`, one call per channel for planar formats), but needs confirming on real Chrome
  (IDL alone isn't reliable per this crate family's history — see
  [web-video-decode](web-video-decode.md)). Not verified in this environment (no browser
  runtime here).
- Whether a full encode→mux→demux→decode Opus round trip is possible yet — blocked by the
  encoder-side ADR's `iso-bmff` Opus-mux caveat; this pass sticks to
  `EncodedAudioChunk`-level-only round trip (no container).
