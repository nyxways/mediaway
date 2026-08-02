# SW fallback crate scaffold

- Path: `crates/mediaway-sw`
- **Pure Rust sans-io** — no C codec FFI; see [policy](policy.md)
- Stage 5 of the workspace roadmap; crate-local roadmap has its own stages
- Scope decided in [ADR-0001](../../../../crates/mediaway-sw/adr/0001-h264-baseline-decoder-first.md):
  H.264 **decode** first (not encode) — tractable/testable sooner than a rate-controlled
  encoder. Future decode/encode sessions mirror the `mediaway-decoder`/`mediaway-encoder`
  facade traits (`VideoDecoder` / `VideoEncoder`) so `mediaway-sw` is swappable with the
  Windows HW backends.
- **Implemented today:** `h264` module — Annex-B (`0x000001`/`0x00000001`) and AVCC
  (length-prefixed) NAL unit splitting, `emulation_prevention_three_byte` removal, and
  SPS/PPS header parsing (profile/level, cropped width/height, entropy mode, ref-idx
  defaults). Pure bitstream/byte-slice transforms — no IO in the core.
- PPS parsing rejects `num_slice_groups_minus1 > 0` (FMO/ASO) with a dedicated error
  rather than decoding multiple slice groups.
- **CAVLC I-slice pixel decode (2026-07-29):** real Baseline/CAVLC/I-slice-only
  bitstream-to-pixels decoder (`h264::decode::decode_i_frame`) — slice-header +
  macroblock-type parsing, a full table-driven CAVLC decoder (VLC tables
  cross-checked against FFmpeg's `h264data.c` CBP table), dequant + inverse
  4x4 transform + 4x4/2x2 inverse Hadamard, `I_16x16`/chroma-8x8 intra
  prediction, per-macroblock CAVLC neighbor (`nC`) bookkeeping. End-to-end
  test decodes a hand-built synthetic bitstream to hand-computed exact pixel
  values — independently re-derived this session (dequant: level 5 at QP 28
  → 320 via `LevelScale`/`normAdjust4x4`; DC-only Hadamard/core-transform
  spread stays uniform; `(320+32)>>6=5` residual on top of DC-mode 128
  prediction → 133) and confirmed correct, not just trusted. CABAC, P/B/SP/SI
  slices, and `I_NxN` (4x4/8x8) macroblocks are recognized and cleanly
  rejected (not implemented — see ADR-0003 for the scope-cut rationale, esp.
  why `I_NxN` was cut despite being in scope: no independent verification
  oracle for its 9 directional 4x4 modes + second neighbor-bookkeeping
  system). No deblocking filter, no multi-slice, flat scaling lists only. No
  committed real-encoder bitstream to validate against — no encoder in this
  workspace emits Baseline+CAVLC+I-only. `VideoDecoder` trait impl still
  pending (this is a free function, not yet wrapped as a session type).
- **`pcm` module (2026-07-29):** real `PcmEncoder`/`PcmDecoder` passthrough
  (`src/pcm.rs`) — validates sample format/rate/channel count, moves `Bytes`
  through unchanged (a cheap refcounted clone, not a re-encode). Mirrors the
  `mediaway_encoder::AudioEncoder` shape.
- **`av1` module (2026-07-29):** real `Av1Encoder` wrapping `rav1e`
  (BSD-2-Clause, `default-features = false` — pure Rust, no `asm`/`cc`
  toolchain) directly — `open`/`push_frame`/`poll_packet`/`flush`. 8-bit
  `PixelFormat::I420` only. `cargo deny check` clean; two narrowly-scoped
  `deny.toml` exceptions needed (a `cfg(fuzzing)`-only NCSA dep, an
  upstream-archived-but-compile-time-only proc-macro RUSTSEC ignore) — see
  `mediaway-sw/adr/0002`. No permissive-license AV1 decoder exists in this
  workspace to round-trip against, so output is validated structurally (OBU/
  sequence-header framing) not pixel-for-pixel.
