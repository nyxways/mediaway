# Web: bugs found only reachable via real Chrome (not Playwright's bundled Chromium)

Playwright's bundled `channel: "chromium"` build lacks a real H.264/AAC WebCodecs backend
(see [web-gpu-frame](web-gpu-frame.md) § "Real Chrome via CDP"), so `is_webcodecs_av_supported`
gated these paths closed in every prior session — three real bugs in `mediaway-encoder::web`
sat unreachable until manually verified against the machine's installed real Google Chrome
over CDP. All three are fixed; none needed a WebGPU-side or codec-choice change.

## 1. `AudioEncoderConfig::new` argument order swapped

`web-sys` 0.3.103's `AudioEncoderConfig::new` signature is
`(codec, number_of_channels, sample_rate)` — **not** `(codec, sample_rate,
number_of_channels)`. Both `wasm.rs` call sites passed `("mp4a.40.2", 48_000, 2)`, building a
nonsensical config (48,000 channels at 2 Hz) that `isConfigSupported` correctly rejected.
This made `audio_supported()` — and therefore `is_webcodecs_av_supported()` — **always**
report `false`, on every browser, forever; not flakiness, not a real-browser-only gap.
Confirmed by reproducing the exact swapped values in raw JS (same false result) vs. the
corrected order (true). **Fix:** swap to `("mp4a.40.2", 2, 48_000)` at both sites.

## 2. `encode_video_frames` never captured the H.264 decoder config `description`

`EncodedVideoChunkMetadata.decoderConfig.description` (H.264's out-of-band SPS/PPS `avcC`
bytes) was silently dropped by the encode output callback. Invisible before because
Playwright's Chromium can't decode H.264 at all, so nothing ever called
`decode_video_chunks` with an H.264 chunk. On real Chrome (which does decode H.264),
omitting `description` made `VideoDecoder.decode()` throw outright. **Fix:** the output
closure now takes the callback's second (`metadata`) argument, reads
`metadata.decoderConfig.description` via `web-sys`'s `EncodedVideoChunkMetadata`/
`VideoDecoderConfig` bindings on the first chunk that has one, and `EncodedVideoChunks` gained
a `description` getter that JS threads straight into `decode_video_chunks`. Verified: a real
H.264 encode → decode round trip on real Chrome now decodes all frames.

## 3. A single 1024-frame `AudioData` never flushes on a real AAC encoder

`encode_one_aac_buffer`'s one-shot silence buffer (1024 frames = one AAC frame) reliably threw
`EncodingError: Flushing error` on real Chrome's platform AAC encoder — MDCT-based codecs need
more than one frame buffered (look-ahead/priming delay) before `flush()` can drain a complete
output chunk. Verified empirically: 1024 frames fails, >=2048 (two AAC frames) succeeds.
**Fix:** bumped the smoke buffer to 4096 frames (safety margin). Real A/V fMP4 smoke now
produces actual muxed bytes on real Chrome instead of throwing.

## Also fixed alongside (resource hygiene, not a correctness bug)

`VideoEncoder`/`AudioEncoder`/`VideoDecoder`/`AudioData` instances were never explicitly
`.close()`d after use in any of these wasm crates. Not the cause of bug #1 (that was verified
deterministic, not session-exhaustion-related) but still a real leak of possibly
hardware-backed codec sessions — added `.close()` calls at every call site as routine cleanup.

## Verification method note

`playwright-core`'s `chromium.connectOverCDP` **hung until Playwright's 30s connect timeout
under Bun**, every time, regardless of `--remote-allow-origins=*`; the identical script
succeeded immediately under Node.js. Use Node, not Bun, for any future manual CDP work in
this repo.
