# ADR-0003: CAVLC / I-slice-only H.264 pixel decode — `I_16x16` + `I_PCM` only, no deblocking

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-sw`

## Context

[ADR-0001](0001-h264-baseline-decoder-first.md) landed NAL framing and SPS/PPS header
parsing, explicitly leaving "slice header parsing + macroblock/CABAC or CAVLC pixel
reconstruction" as future work. This ADR is that work's scope decision: a real, first
end-to-end I-frame pixel decode, not a general-purpose H.264 decoder. A full Baseline
decoder (CAVLC + CABAC, I/P/B slices, `I_NxN` + `I_16x16` + `I_PCM`, deblocking) is a
multi-session effort; this ADR scopes the first slice down to something a single session
can deliver **correctly and testably**, per [`docs/spec/maturity-bar.md`](../../../docs/spec/maturity-bar.md)'s
bar for claiming working code.

## Decision

> **Scope this session's decode loop (`h264::decode_i_frame`) to:**
>
> 1. **Baseline profile.** Matches ADR-0001's SPS/PPS parser assumptions (no 8x8 transform,
>    no custom scaling lists retained by `Sps::parse`).
> 2. **CAVLC entropy coding only.** `pps.entropy_coding_mode == true` (CABAC) is rejected
>    with [`H264Error::UnsupportedEntropyCoding`]. CABAC's arithmetic-coding state machine
>    is a substantially different (and substantially larger) implementation; CAVLC's
>    table-driven VLCs are tractable to get bit-exact in one session and to verify via
>    hand-built bitstreams in unit tests.
> 3. **I-slices only.** P/B/SP/SI slice types are rejected with
>    [`H264Error::UnsupportedSliceType`] — no motion compensation, no reference picture
>    lists, no weighted prediction.
> 4. **`I_16x16` and `I_PCM` macroblocks only — `I_NxN` (4x4/8x8 intra) is recognized but
>    rejected** with [`H264Error::UnsupportedMbType`]. This is a scope cut *beyond* what
>    the task setup suggested as a baseline (which mentions "intra 4x4/16x16 prediction
>    mode signaling" as in-scope). The reason: correctly reconstructing `I_NxN` needs, on
>    top of everything `I_16x16` needs, (a) all 9 directional 4x4 luma prediction modes
>    (vs. `I_16x16`'s 4 whole-block modes) and (b) per-4x4-block `Intra4x4PredMode`
>    neighbour-context tracking (a second neighbour-bookkeeping structure alongside the
>    CAVLC `nC` tracking `I_16x16` already needs). Implementing that additional surface
>    without an independent way to verify the 9 directional-mode formulas bit-exactly
>    (no oracle decoder available, and hand-building a multi-mode multi-macroblock test
>    vector to exercise all 9 is a large undertaking on its own) risked exactly the
>    "confidently wrong reconstruction" outcome the task explicitly warned against.
>    `I_16x16` + `I_PCM` alone already exercises the full CAVLC pipeline (`coeff_token`,
>    level, `total_zeros`, `run_before`, the DC-block Hadamard path), the full dequant +
>    integer inverse transform, and whole-block intra prediction (including the trickier
>    chroma DC per-quadrant averaging rule) — a real, substantial, verified slice.
> 5. **4:2:0 only** (`sps.chroma_format_idc != 1` rejected with
>    [`H264Error::UnsupportedChromaFormat`]), **frame pictures only**
>    (`sps.frame_mbs_only == false` rejected with [`H264Error::UnsupportedFieldCoding`]),
>    **`pic_order_cnt_type == 0` only** (types 1/2 rejected with
>    [`H264Error::UnsupportedPicOrderCntType`] — type 0 is the overwhelmingly common case
>    for Baseline low-latency encoders).
> 6. **One slice per picture** (`first_mb_in_slice != 0` rejected with
>    [`H264Error::MultiSliceUnsupported`]) — no multi-slice picture composition.
> 7. **No deblocking filter.** Reconstructed output is visibly blockier at low bitrate /
>    high QP than a spec-complete decoder's output, especially at 4x4/16x16/8x8 block
>    boundaries. This is a real, user-visible quality gap, not a silently-skipped detail —
>    flagged here, in `h264::decode` module docs, and in the crate roadmap.
> 8. **Flat/default scaling lists assumed** (`Flat_4x4_16`, `weightScale == 16`
>    everywhere) — `Sps::parse` (ADR-0001) already only skips past `scaling_list()` bodies
>    rather than retaining their values, so this crate structurally cannot honor a stream
>    that signals custom scaling lists yet. A stream with non-default scaling lists still
>    decodes without panicking, just with numerically wrong dequantized values.
> 9. **Cropping only trims bottom/right** of the reconstructed macroblock-grid picture down
>    to `Sps::width`/`Sps::height` (assumes `crop_left == crop_top == 0`). `Sps::parse`
>    only retains the *final* cropped width/height, not the four individual crop offsets,
>    so a true top/left-anchored crop is not implemented. Covers the common case (removing
>    macroblock-alignment padding from the bottom/right, e.g. 1080p from a 1088-tall
>    macroblock grid).
>
> **New modules** (all `crates/mediaway-sw/src/h264/`, sibling `*_tests.rs` per file,
> each ≤1000 lines): `slice` (slice header parsing), `macroblock` (`mb_type`
> Table 7-11 semantics, `intra_chroma_pred_mode`), `cavlc` + `cavlc_tables` (CAVLC VLC
> tables and decode: `coeff_token`, level, `total_zeros`, `run_before`), `transform`
> (dequantization, integer inverse 4x4 transform, 4x4/2x2 inverse Hadamard, `QPc` table),
> `intra_pred` (`I_16x16` and chroma 8x8 whole-block prediction modes), `reconstruct`
> (per-macroblock CAVLC neighbour bookkeeping + pixel reconstruction), `decode`
> (top-level `decode_i_frame` orchestration). `Sps`/`Pps` gained new fields
> (`log2_max_frame_num`, `pic_order_cnt_type`, `log2_max_pic_order_cnt_lsb`,
> `chroma_format_idc`, `pic_width_in_mbs`, `pic_height_in_mbs`,
> `chroma_qp_index_offset`) that this decode loop needs and ADR-0001's parser had
> discarded — purely additive, no existing field/behavior changed.
>
> **Output type**: [`mediaway_common::VideoFrame`] with
> [`VideoFrameStorage::Cpu`]/[`PixelFormat::I420`] (packed, no row padding) — matches
> ADR-0001's plan ("only ever produces `VideoFrameStorage::Cpu` frames"). Wiring a
> `mediaway_decoder::VideoDecoder` session type around `decode_i_frame` (push/poll,
> multi-frame state, `StreamInfo`) is still future work, same as ADR-0001 deferred it.
>
> **Test vector strategy**: no encoder in this workspace can currently produce
> Baseline+CAVLC+I-only bitstreams to capture (checked `mediaway-encoder-windows`/
> `-linux`/`-quicksync`/`-vulkan`; none expose a CAVLC-vs-CABAC or I-only knob suitable for
> minting a tiny conformance-style clip). The end-to-end proof in
> `h264/decode_tests.rs` (`decode_i_frame_reconstructs_a_solid_color_one_macroblock_picture`)
> is a **hand-built synthetic bitstream** (one 16x16 macroblock, `I_16x16` DC-mode
> prediction, a single non-zero luma DC coefficient, no chroma residual), explicitly
> labeled as such in the test's own comments — not real encoder output. Every numeric CAVLC
> table (`coeff_token` VLC0/VLC1/VLC2/chroma-DC, `total_zeros`, `run_before`, the
> `Table 9-4`-style Golomb-to-CBP mapping this ADR ended up *not* needing once `I_NxN` was
> cut) and the dequant `normAdjust`/`QPc` tables were cross-checked against a second,
> independent source (a permissively-licensed encoder's literal table source, and
> FFmpeg's `h264data.c` numeric constants — read only to fact-check numbers, not copied
> as code) before being hand-transcribed into this crate's own Rust implementation; see
> the module docs on `cavlc_tables.rs` and `transform.rs` for exact citations. The chroma
> Plane-mode weighting constants (`17`/`16`/`>>5`) were **not** independently
> cross-checked this session (only the algebraically-equivalent luma Plane formula was)
> — flagged as a follow-up verification item; it is not exercised by the current
> end-to-end test (which uses DC mode only, since the test's single macroblock has no
> neighbours).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Full `I_NxN` support too (all 9 4x4 modes) | Real risk of an unverifiable, silently-wrong reconstruction path — see point 4 above; better to ship a smaller, verified surface |
| CABAC instead of / in addition to CAVLC | Arithmetic coding state machine is a much larger, riskier surface for one session; CAVLC is table-driven and independently verifiable |
| Skip the hand-built test vector, only unit-test sub-stages | The task explicitly asks for at least one real end-to-end frame decode; sub-stage unit tests exist too (`cavlc_tests.rs`, `transform_tests.rs`, `intra_pred_tests.rs`, `reconstruct_tests.rs`) but don't by themselves prove the pieces compose correctly |
| Implement deblocking now | Out of scope for "first slice"; a real, separate, non-trivial filter (needs a second post-reconstruction pass over already-decoded macroblocks with its own boundary-strength derivation) |

## Consequences

### Positive

- A real, testable, spec-traceable pixel decode lands: 119 passing tests across the
  `h264` module, including one genuine bitstream-to-pixels end-to-end decode with
  hand-verified expected output (not just "it didn't crash")
- Establishes the module shape (`slice`, `macroblock`, `cavlc`/`cavlc_tables`,
  `transform`, `intra_pred`, `reconstruct`, `decode`) future `I_NxN`/CABAC/P-slice/
  deblocking work extends, instead of starting from an empty decode loop
- `Sps`/`Pps` field additions are purely additive — no existing parsing behavior changed,
  confirmed by the full existing SPS/PPS/NAL/bitreader test suite still passing unmodified

### Negative / Trade-offs

- `I_NxN` macroblocks — common in real encoder output at higher QP / for I-slices with
  fine detail — are rejected outright; a real-world Baseline stream may well fail to
  decode with this crate today
- CABAC (the default entropy mode for Main/High profile, and available to Baseline too)
  is entirely unsupported
- No deblocking filter — visibly blockier output than a spec-complete decoder at the same
  bitrate, at every 4x4/8x8/16x16 block boundary
- Custom (non-flat) scaling lists silently produce wrong dequantized values rather than an
  error (structurally can't detect this today — `Sps::parse` doesn't retain scaling list
  presence, only skips past the bits)
- Chroma Plane-mode constants unverified against a second source this session (see
  Decision's last paragraph) — a real (if narrow) correctness risk for any stream that
  exercises chroma Plane mode

## References

- [ADR-0001](0001-h264-baseline-decoder-first.md) — staging plan this ADR continues
- [`docs/spec/maturity-bar.md`](../../../docs/spec/maturity-bar.md)
- ITU-T Rec. H.264 § 7.3.3 (slice header), § 7.3.5 (macroblock layer), § 7.3.5.3.1/.2
  (`residual_block_cavlc`), § 8.3.3/§ 8.3.4 (`I_16x16`/chroma intra prediction),
  § 8.5.9-8.5.13 (dequantization + inverse transform), § 9.2 (CAVLC parsing process),
  Table 7-11 (`mb_type` for I slices), Table 8-15 (`QPc` mapping), Table 9-5/9-7/9-8/9-9/
  9-10 (CAVLC VLC tables) — see
  [`docs/conventions/external-standards.md`](../../../conventions/external-standards.md)
  for citation policy (not reproduced here)
- Crate roadmap: [`docs/roadmap.md`](../docs/roadmap.md)
