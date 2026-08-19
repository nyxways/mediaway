# Web: Opus audio encode — codec-parameterized audio smoke surface (implemented)

- ADR: [`crates/mediaway-encoder/adr/web/0001-webcodecs-opus-audio-encode.md`](../../../crates/mediaway-encoder/adr/web/0001-webcodecs-opus-audio-encode.md)
  (implemented 2026-08-19). Sibling decode ADR: [`decode/web-opus-design`](../decode/web-opus-design.md).
- **Verification**: `cargo build -p mediaway-encoder --target wasm32-unknown-unknown` compile-
  verified only — no real browser runtime in this environment. `frame_count`'s Opus-safe
  minimum (see Open questions below) is genuinely unmeasured.

## What was there before this change

`mediaway-encoder::web` is **not** a facade `AudioEncoder`/`VideoEncoder` trait impl — it's
a standalone `wasm-bindgen` JS-callable probe/smoke surface for `tools/e2e-web`. Audio was
AAC-hardcoded only (`audio_supported()`, `encode_one_aac_buffer()`); zero Opus anywhere.
Video's equivalent functions were already codec-parameterized
(`is_webcodecs_video_codec_supported(codec)`, `encode_video_frames(codec, …)`).

## Key findings from this design pass

- `web-sys` (pinned `0.3.104` as of this session, up from `0.3.103` alongside the wgpu
  26→30 bump — `Cargo.lock`) has **no `OpusEncoderConfig` binding at all** — no
  `gen_OpusEncoderConfig.rs` file exists, and `gen_AudioEncoderConfig.rs` has no Opus
  fields. The `WebCodecs` optional `OpusEncoderConfig` dictionary (`frameDuration`,
  `complexity`, `packetlossperc`, `useinbandfec`, `usedtx`, …) is unreachable without raw
  untyped `js_sys::Reflect` JS-object construction — deferred.
- `AudioEncoderConfig::new(codec, number_of_channels, sample_rate)` confirmed by reading
  `gen_AudioEncoderConfig.rs` directly — same order already fixed for AAC
  ([web-real-chrome-bugs](web-real-chrome-bugs.md) bug #1); Opus needs no different call.
- `iso-bmff`'s MP4 muxer (`sample_entry.rs::write_stsd`) routes `Codec::Opus` through the
  same `write_mp4a`/`esds` (AAC) writer as `Codec::Aac` — **no real `Opus`/`dOps` sample
  entry exists**. Same "wrong sample entry" situation HEVC/AV1 were in before `iso-bmff`
  ADR-0003; that ADR's own text already lists `Opus` under "codecs with no ISOBMFF brand
  yet." A real Opus fMP4 mux therefore isn't possible today — the ADR scopes the Opus smoke
  path to `EncodedAudioChunk`-level validation only (no container), same posture VP9 used
  before `iso-bmff` ADR-0002.
- `@mediaway/browser` (npm package, `bindings/browser/packages/browser`) already handles
  Opus generically at the TS level today (`EncodeSession.audio()` passes through any
  `WebCodecs` codec string) — a separate layer (ADR-0020), unaffected by this gap.

## What shipped

Generalized the AAC-hardcoded functions into codec-parameterized ones mirroring the video
side's existing shape: `is_webcodecs_audio_codec_supported(codec, channels, sample_rate)` and
`encode_audio_buffer(codec, channels, sample_rate, bitrate_bps, frame_count) ->
EncodedAudioChunks` (new type, `chunks.rs`), returning every chunk not just the first.
`audio_supported()`/`encode_one_aac_buffer()` are now thin AAC-fixed callers of the
generalized functions — `is_webcodecs_av_supported()`/`webcodecs_av_fmp4_smoke()` keep their
exact prior behavior. Opus is exercised as the second codec through the generalized surface —
no Opus-only functions, no `OpusEncoderConfig` knobs, no Opus container-mux claim.

One real ADR-vs-reality gap found during implementation: `AudioDataInit::new`'s sample-rate
parameter is `f32`, not `f64` (unlike the `f64` timestamp params elsewhere in this module) —
required an explicit `sample_rate as f32` cast (`#[allow(clippy::cast_precision_loss)]`,
justified since encoder sample rates are always small exact integers).

## Open questions (unresolved by the ADR)

- Opus's safe `frame_count` (flush-buffering minimum) is unmeasured — AAC's 4096-frame
  margin was empirical and codec-specific, must not be reused blindly.
- Whether/when `web-sys` ships `OpusEncoderConfig`.
- Whether `iso-bmff` should get a real `Opus`/`dOps` sample entry (separate crate-local ADR).
