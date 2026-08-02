# Web: WebCodecs video decode (`mediaway-decoder-web`)

- Crate: `crates/mediaway-decoder-web` (platform backend). Mirrors `mediaway-encoder-web`'s
  shape (`config`/`host`/`wasm` split) and does **not** implement the `mediaway-decoder`
  facade trait — `VideoDecoder` is inherently async/callback-driven, incompatible with the
  facade's sync `push_packet`/`poll_frame` shape (same reasoning as encoder-web).
- Exports: `is_webcodecs_video_decode_supported(codec, width, height)`;
  `decode_video_chunks(codec, width, height, description, chunk_data, chunk_offsets,
  chunk_lengths, chunk_timestamps_us, chunk_is_key) -> DecodedVideoFrames`.
- `DecodedVideoFrames` exposes `frame_count`, `timestamp_us(i)`, `luma_plane(i)` — reads
  pixel data back via `VideoFrame::copyTo` (async, de-strided using the returned
  `PlaneLayout`, since `codedWidth` can exceed `width`).
- Chunks cross from `mediaway-encoder-web`'s new `encode_video_frames`/`EncodedVideoChunks`
  into this crate as flattened primitive arrays (`Vec<u8>`/`Vec<u32>`/`Vec<f64>`), never as
  shared Rust types or `web-sys` objects — the two crates compile to *separate* wasm
  modules/instances, so only plain JS values cross that boundary.

## Codec support in this Chromium test build

Verified empirically (Playwright's bundled Chromium): H.264 **encode** NOT
usably supported (see below — `isConfigSupported` over-reports it), H.264 **decode** NOT
supported; AV1 encode+decode supported (bundled libaom software encoder); VP9 **decode**
supported but **encode** NOT supported (2026-07-29 re-check via
`tests/codec-support-matrix.spec.ts` — supersedes an earlier note claiming VP9 encode also
worked, which no longer holds on this Chromium build); HEVC not supported at all on this
build (neither direction). On real Microsoft Edge (`msedge-real` project, same spec) VP9 and
AV1 both fully encode+decode; HEVC decodes but has no encode path there either.
`tools/e2e-web/tests/decode-trim-splice.spec.ts` probes codecs in preference order (VP9,
VP8, AV1, H.264) and uses the first with both encode+decode support — skips honestly only
if none work.

## Real bug found: `isConfigSupported` resolves to a dictionary, not a boolean

`VideoEncoder`/`VideoDecoder`'s `isConfigSupported()` resolve to a `{supported, config}`
dictionary, never a boolean. `mediaway-encoder-web`'s pre-existing `video_codec_supported` /
`audio_supported` read `.as_bool()` on that value — **always `None`/`false`**, regardless of
real support (masked by `webcodecs-fmp4.spec.ts` skipping for a plausible-looking reason).
Root cause: js-sys 0.3.103's typed `Promise<T>`/`JsFuture<T>` (and typed `Array<T>`) already
yield the dictionary **strongly typed** (`T`, not `JsValue`) — `.as_bool()` silently no-ops
via `Deref<Target = JsValue>`, and a further `.dyn_into::<T>()` (this crate's first attempt)
fails too, since WebIDL "dictionary" types (`VideoDecoderSupport`, `PlaneLayout`, …) have no
real JS constructor to `instanceof`-check against. **Fix:** read the getter directly off the
already-typed result, no cast — see `mediaway-encoder-web/src/wasm.rs`
(`video_codec_supported`, `audio_supported`) and `mediaway-decoder-web/src/wasm.rs`
(`is_webcodecs_video_decode_supported`, `read_luma_plane`'s `PlaneLayout` read).

## Second real bug: `isConfigSupported` over-reports H.264 encode in this build

Fixing the dictionary-cast bug above made `is_webgpu_video_frame_supported()` report `true`
for H.264, so `webcodecs-fmp4.spec.ts`'s GPU smoke test stopped skipping and actually ran for
the first time — and failed with `OperationError: Encoding error` from `VideoEncoder.encode()`
on a WebGPU-canvas-sourced `VideoFrame`. Debugging showed this is **not** specific to the
WebGPU-canvas source: a plain CPU NV12 `VideoFrame` encoded with `avc1.42E01E` fails with the
identical error in this Chromium build — `isConfigSupported` itself over-reports H.264 encode
as supported here even though no real encoder backend is reachable.

**Fix:** `video_codec_supported` (`wasm.rs`) no longer trusts `isConfigSupported` alone —
after it reports `true`, it also runs one real `configure()` + `encode()` + `flush()` of a
throwaway frame and treats any error as unsupported. Generic over the codec string, so
`is_webcodecs_video_codec_supported` (the VP9/VP8/AV1/H.264 probe loop below) gets the same
honesty for free. `webgpu_canvas_frame` itself was never at fault. Confirmed on the machine's
real installed Chrome that H.264 encode genuinely works there (this Chromium build's gap was
a test-environment limit, not a Mediaway bug) — see
[encode/web-real-chrome-bugs](../encode/web-real-chrome-bugs.md) for that session's three
further real bugs (`AudioEncoderConfig` argument order, missing decoder-config `description`,
AAC flush buffering) found only reachable on a real browser.

## fMP4 mux/demux in the browser E2E

`iso-bmff` now writes a real `vp09`/`vpcC` sample entry for VP9 (crate-local
[ADR-0002](../../../crates/iso-bmff/adr/0002-vp9-sample-entry.md)) — see
[container/mp4-sample-entries](../container/mp4-sample-entries.md) for full coverage
(HEVC/AV1 still fall back to a mislabeled `avc1`). `tools/e2e-web/tests/wasm-mux-roundtrip.spec.ts`
proves the container-level VP9 mux→demux round trip in-browser via `iso-bmff-wasm`'s pure
sans-io logic — independent of this Chromium build's real WebCodecs VP9 support.
`decode-trim-splice.spec.ts` still proves the encode→decode→trim→splice→re-encode→decode
round trip directly at the `EncodedVideoChunk` level (no container in between): combining it
with a real fMP4 round trip would require this browser's WebCodecs to actually decode the
codec `iso-bmff` also supports muxing (VP9), which is a separate, not-yet-attempted step, not
a blocked one. Real fMP4 round-trip coverage for H.264 already exists on Windows
(`mediaway-pipeline/tests/trim_and_splice_windows.rs`).
