# mediaway-sw — roadmap

Pure Rust sans-io SW codecs; schedule after primary platform HW paths exist.  
No C codec FFI — [`docs/ai/wiki/license/policy.md`](../../../docs/ai/wiki/license/policy.md).  
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Crate + `docs/` / `adr/`
- [x] ADR: pure Rust codec scope + sans-io boundary — [`adr/0001-h264-baseline-decoder-first.md`](../adr/0001-h264-baseline-decoder-first.md)

### 1 — Baseline (decode first, per ADR-0001)

- [x] H.264 Annex-B / AVCC NAL unit framing (start codes, emulation-prevention removal)
- [x] H.264 SPS / PPS header parsing (profile/level, width/height, entropy mode, ref-idx defaults)
- [x] H.264 slice header + CAVLC macroblock pixel decode loop (`decode_i_frame`) —
  [ADR-0003](../adr/0003-cavlc-i-slice-first-decode.md). Baseline profile, CAVLC only,
  I-slices only, `I_16x16`/`I_PCM` macroblocks only (`I_NxN` rejected), 4:2:0 only,
  **no deblocking filter**. Real end-to-end bitstream-to-pixels decode, hand-verified
  against a synthetic test vector (no in-workspace encoder can mint a Baseline+CAVLC+
  I-only clip to capture instead — see the ADR).
- [ ] CABAC entropy decode
- [ ] `I_NxN` (4x4/8x8) intra macroblock reconstruction
- [ ] P/B-slice (inter) decode
- [ ] Deblocking filter
- [ ] Wire `decode_i_frame` as a `VideoDecoder` factory fallback (`mediaway-decoder`)
- [ ] H.264 bitstream encode (pure Rust sans-io)
- [ ] Wire as `VideoEncoder` factory fallback (`mediaway-encoder`)
- [x] PCM / raw audio passthrough encode + decode (`pcm` module — validates format/rate/channels, moves `Bytes` through unchanged)

### 2 — AV1

- [x] `rav1e` behind sans-io adapter (`av1` module) — [ADR-0002](../adr/0002-rav1e-av1-encode.md). 8-bit `PixelFormat::I420` input only; wired as `mediaway-encoder-windows`'s `AutoVideoEncoder` Software fallback (`EncodePathClass::Software`, AV1 only).

### 3 — Platform packaging

- [ ] Windows / Web / Linux delivery notes (Rust-only graph)
