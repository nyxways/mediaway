# ADR-0001 (web): WebCodecs Opus audio encode — generalize the AAC-only smoke surface

- **Status**: Accepted — implemented, wasm32 compile-verified only (no real browser runtime
  available in this environment)
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`web` module)

## Context

- `mediaway-encoder::web` (`crates/mediaway-encoder/src/web/`) is a `wasm32` `WebCodecs`
  backend, but it does **not** implement the facade `AudioEncoder`/`VideoEncoder` traits
  (ADR-0001 at the crate root) — `WebCodecs` sessions are inherently async/callback-driven,
  incompatible with the facade's sync `push_frame`/`poll_packet` shape (same reasoning the
  decoder side states explicitly for `VideoDecoder`, see the sibling ADR). Instead this module
  is a standalone set of `wasm-bindgen`-exported, JS-callable probe/smoke functions consumed by
  `tools/e2e-web`'s Playwright harness, proving `iso-bmff` mux/demux + real-browser `WebCodecs`
  interop end to end.
- The **video** side is already codec-parameterized: `is_webcodecs_video_codec_supported(codec)`,
  `encode_video_frames(codec, width, height, bitrate_bps, lumas, timestamps_us) ->
  EncodedVideoChunks` (returns every chunk, plus the captured `decoderConfig.description`).
- The **audio** side is hardcoded to AAC only: `audio_supported()` (internal, called by
  `is_webcodecs_av_supported`) and `encode_one_aac_buffer()` (internal, called by
  `webcodecs_av_fmp4_smoke`) both hardcode `WebAudioEncoderConfig::new("mp4a.40.2", 2,
  48_000)`, a fixed 4096-frame silence buffer, and a 128 kbps bitrate. Neither is exposed to
  JS directly, and neither is codec-parameterized. No Opus reference exists anywhere in this
  module (confirmed by grep this session, both `mediaway-encoder/src/web/*.rs` and
  `mediaway-decoder/src/web/*.rs`).
- `@mediaway/browser` (npm package, `bindings/browser/packages/browser`, workspace
  ADR-0020/0022) already handles Opus **generically** today at the TypeScript level —
  `EncodeSession.audio()` passes through whatever `WebCodecs` codec string the caller
  supplies (its own doc comment already lists `"opus"` as an example alongside
  `"mp4a.40.2"`), and its `DecodeSession` counterpart already derives `numberOfChannels`
  from an `OpusHead` (RFC 7845) `CodecPrivate`/`extraData` payload. That package is a
  **separate, deliberately independent layer** (ADR-0020's "WASM owns container, host owns
  codecs" split) — it is not backed by this crate's `wasm-bindgen` exports and is out of
  scope here. This ADR is scoped to the lower-level `mediaway-encoder::web` /
  `mediaway-decoder::web` Rust crate surface only.
- `WebCodecs`' Opus codec string is the bare literal `"opus"` (no profile/level suffix,
  unlike `"avc1.42E01E"`/`"mp4a.40.2"`).
- Verified against the pinned `web-sys` 0.3.104 source (`Cargo.lock` resolves `web-sys` to
  0.3.104 as of this session, up from 0.3.103 alongside the wgpu 26→30 bump;
  `gen_AudioEncoderConfig.rs`): `AudioEncoderConfig::new(codec, number_of_channels,
  sample_rate)` — the same argument order already fixed for AAC (see
  `docs/ai/wiki/encode/web-real-chrome-bugs.md` bug #1). Opus needs no different call shape.
- Verified: `web-sys` 0.3.104 has **no `OpusEncoderConfig` binding at all** — no
  `gen_OpusEncoderConfig.rs` file exists anywhere in the crate source, and
  `gen_AudioEncoderConfig.rs` embeds no Opus-specific fields. The `WebCodecs` spec's optional
  `OpusEncoderConfig` dictionary (`format`, `frameDuration`, `complexity`, `packetlossperc`,
  `useinbandfec`, `usedtx`) is therefore unreachable through this crate's typed `web-sys`
  bindings today, short of hand-rolled untyped `js_sys::Reflect::set` JS-object construction.
- `iso-bmff`'s MP4 muxer (`sample_entry.rs::write_stsd`) currently routes `Codec::Opus`
  through the same `write_mp4a`/`esds` (AAC) sample-entry writer as `Codec::Aac` — there is
  no `Opus`/`dOps` box writer. This mirrors the situation HEVC/AV1 were in before `iso-bmff`
  ADR-0003 gave them real `hvc1`/`av01` writers; `iso-bmff` ADR-0003 itself already lists
  `Opus` alongside `Aac`/`WebVtt`/`Tx3g` under "codecs with no ISOBMFF brand yet." Fixing this
  is `iso-bmff`-crate-local scope, not this crate's.

## Decision

> Generalize the existing AAC-hardcoded audio smoke/probe functions in
> `mediaway-encoder::web` into codec-parameterized functions mirroring the video side's
> existing shape, and exercise Opus as the second concrete codec through that generalized
> surface. Do not add `OpusEncoderConfig` dictionary knobs (no reachable binding). Do not
> claim a real Opus fMP4 mux in this pass — validate Opus at the `EncodedAudioChunk` level
> only.

### Public surface (`mediaway-encoder::web`, both `wasm.rs` and its `host.rs` stub)

| Item | Role |
|---|---|
| `is_webcodecs_audio_codec_supported(codec, channels, sample_rate) -> bool` | New `#[wasm_bindgen]` export generalizing `audio_supported`'s internal logic (mirrors `is_webcodecs_video_codec_supported`) |
| `encode_audio_buffer(codec, channels, sample_rate, bitrate_bps, frame_count) -> Result<EncodedAudioChunks, JsValue>` | New `#[wasm_bindgen]` export generalizing `encode_one_aac_buffer` (mirrors `encode_video_frames`); returns **every** encoded chunk, not just the first (parity with the video side's own prior upgrade) |
| `EncodedAudioChunks` (new type, `chunks.rs`) | Parallel `timestamp_us` / `payload` vecs, flattened getters — same wasm-module-boundary-crossing shape as `EncodedVideoChunks` |
| `audio_supported()` / `encode_one_aac_buffer()` | Kept, now thin callers of the generalized functions with AAC's existing fixed args (`"mp4a.40.2"`, 2, 48_000, 128_000, 4096) — `is_webcodecs_av_supported()` / `webcodecs_av_fmp4_smoke()` keep their exact prior behavior |

### Rules

1. **Codec string is caller-supplied, never hardcoded to `"opus"` inside the generalized
   functions** — same pattern as `is_webcodecs_video_codec_supported`/`encode_video_frames`.
   Opus is exercised via a new `tools/e2e-web` smoke/E2E test calling these with
   `("opus", 2, 48_000, ...)`, not via a separate Opus-only code path.
2. **`frame_count`'s empirically-safe minimum is not assumed** — AAC's 4096-frame safety
   margin was found empirically for AAC's MDCT look-ahead/priming delay
   (`web-real-chrome-bugs.md` bug #3) and must not be silently reused for Opus, a different
   codec family (2.5–60 ms frames, no MDCT priming in the same sense). The Opus smoke test's
   own safe `frame_count` needs its own real-Chrome-over-CDP empirical check when
   implemented; this ADR does not pre-guess a number (see Open Questions).
3. **No `OpusEncoderConfig` dictionary** (`format`/`frameDuration`/`complexity`/
   `packetlossperc`/`useinbandfec`/`usedtx`) in this pass — `AudioEncoderConfig`'s plain
   `(codec, channels, sample_rate)` + `bitrate` is the only reachable, typed surface in
   pinned `web-sys` 0.3.104. Revisit once/if `web-sys` ships bindings.
4. **No Opus fMP4 mux claim** — `mux_av_chunks`-style container muxing stays AAC-only until
   `iso-bmff` gets a real `Opus`/`dOps` sample-entry writer (separate, `iso-bmff`-crate-local
   follow-up). The Opus smoke path validates the `WebCodecs` `AudioEncoder ->
   EncodedAudioChunk` round trip only, the same posture `decode-trim-splice.spec.ts` used to
   prove VP9's encode/decode round trip before `iso-bmff` ADR-0002 added a real VP9
   container mapping.
5. **Host-target stub (`host.rs`) mirrors every new export** with the existing
   `"wasm32 browser only"` error convention — same as `encode_video_frames`'s stub.
6. **Resource hygiene**: `AudioEncoder.close()` after every encode, matching the existing
   fix already applied to the AAC path (`web-real-chrome-bugs.md`, "Also fixed alongside").

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Opus-only new functions (`is_opus_supported`, `encode_opus_buffer`), leave AAC hardcoded as-is | Duplicates the exact shape the video side already generalized once; a 3rd audio codec later repeats the duplication again |
| Rely solely on `@mediaway/browser`'s existing generic Opus support, skip this crate entirely | Leaves the Rust wasm-crate-level backend permanently AAC-only/asymmetric with its own video side, and leaves `tools/e2e-web`'s raw-wasm harness unable to smoke-test Opus at all (the npm package has a separate test suite) |
| Implement full `OpusEncoderConfig` knobs via raw `js_sys::Reflect` object construction | No typed `web-sys` binding exists yet; untyped JS interop here is exactly the bug class (`isConfigSupported` cast bugs, swapped arg order) already found twice in this module — deferred until real bindings exist |
| Fix `iso-bmff`'s Opus MP4 sample entry now, ship a real Opus fMP4 smoke test in the same change | Real, separable `iso-bmff`-crate-local decision (own ADR, own tests) — bundling it here would blur scope and block this ADR on unrelated container work |

## Consequences

### Positive

- Audio and video probe/encode-smoke functions become symmetric in shape; adding a 3rd/4th
  audio codec later is a caller-side change only.
- `EncodedAudioChunks` gives audio the same "return every chunk" capability video already
  has, opening the door to future audio trim/splice E2E tests mirroring
  `decode-trim-splice.spec.ts`.

### Negative / Trade-offs

- No real Opus MP4 mux validation yet — a real gap in end-to-end coverage until `iso-bmff`
  gets a proper sample entry.
- Opus's safe `frame_count` minimum is unknown until measured on a real browser —
  implementation work, not resolved by this ADR.
- Slightly larger public `wasm-bindgen` surface (`is_webcodecs_audio_codec_supported`,
  `encode_audio_buffer`, `EncodedAudioChunks`) vs. staying AAC-only.

## Open Questions

- Does Chrome's real Opus `WebCodecs` encoder need a `frame_count` safety margin the way
  AAC's MDCT did? Needs a real-Chrome-over-CDP empirical check (same method as
  `web-real-chrome-bugs.md`), not guessed here.
- When (if) `web-sys` ships `OpusEncoderConfig` bindings, should `mediaway-encoder::web`
  expose them as opt-in parameters, or stay on defaults permanently for a smoke/test-only
  surface? Deferred — no upstream tracking issue exists yet to pin this ADR to.
- Should `iso-bmff` get a real `Opus`/`dOps` MP4 sample entry (separate `iso-bmff`-crate
  ADR, same shape as its ADR-0002/ADR-0003 VP9/HEVC/AV1 precedent)? Flagged here as a
  discovered gap, not decided by this ADR.

## References

- `mediaway-encoder` root ADR-0001 (`VideoEncoder`/`AudioEncoder` traits — why this module
  does not implement them)
- [`web-real-chrome-bugs.md`](../../../../docs/ai/wiki/encode/web-real-chrome-bugs.md) —
  `AudioEncoderConfig` arg-order bug, AAC flush-buffering finding
- `iso-bmff` ADR-0002 (VP9 sample entry), ADR-0003 (HEVC/AV1 sample entry) — precedent for
  "add a real sample entry later"
- [`docs/adr/0022-browser-decode-session-and-device-dx.md`](../../../../docs/adr/0022-browser-decode-session-and-device-dx.md) /
  [`web-decode-session.md`](../../../../docs/ai/wiki/decode/web-decode-session.md) —
  `@mediaway/browser`'s existing generic Opus support (separate layer)
- Sibling ADR: `crates/mediaway-decoder/adr/web/0001-webcodecs-opus-audio-decode.md`

ADRs are **English**. Numbering is local to this `adr/web/` folder.
