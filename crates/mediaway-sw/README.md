# mediaway-sw — docs

Pure Rust sans-io software codec fallbacks. No C codec FFI.  
Roadmap: [roadmap.md](docs/roadmap.md) · License: [policy.md](../../docs/ai/wiki/license/policy.md)

## Status

- **H.264 / AVC decode** — real Baseline/CAVLC/I-slice-only pixel decoder (slice header, macroblock types, CAVLC, dequant/inverse transform, intra prediction); decodes a hand-built bitstream to hand-computed exact pixel values. CABAC/P/B slices are cleanly rejected, not mishandled. See [`adr/0003`](adr/0003-cavlc-i-slice-first-decode.md).
- **AV1 encode** — real `rav1e` (BSD-2-Clause) sans-io encoder adapter, encode-only. See [`adr/0002`](adr/0002-rav1e-av1-encode.md).
- **PCM / raw** — real `PcmEncoder`/`PcmDecoder` passthrough (format/rate/channel validation, no re-encode) — `src/pcm.rs`, 11 unit tests.
