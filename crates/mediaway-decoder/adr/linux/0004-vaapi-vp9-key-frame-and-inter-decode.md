# ADR-0004: VA-API VP9 `KEY_FRAME` + general single-tile `INTER_FRAME` decode (no artificial reference-count restriction — a real structural finding, not scope creep)

- **Status**: Accepted — implemented (`src/linux/vaapi/vp9.rs` + `src/linux/vaapi/vp9/
  {bits,color_config,frame_size,header,loop_filter,quantization,ref_table,segmentation,
  tile_info}.rs`), compile + clippy (`--all-targets -- -D warnings`) + test-verified on real
  WSL2 Linux (`cargo test -p mediaway-decoder --all-features --target x86_64-unknown-linux-gnu`,
  2026-08-19), with 100+ new hand-constructed bitstream-fixture unit tests against the real
  spec syntax tables in this ADR's own addendum. The addendum's real `pdftotext`-extracted
  `s(n)` correction (not `su(n)`) and `VAProfileVP9Profile0 = 19` both held through
  implementation; every other `cros-libva` struct field cited in § VA-API-specific plumbing
  matched real vendored source on the first pass. **Zero real-hardware verification** remains —
  see § Honesty note / Negative Trade-offs.
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (`src/linux/vaapi/`)

## Context

`mediaway-decoder::linux::vaapi` decodes H.264 (ADR-0001 IDR-only, ADR-0002 single-forward-reference
P-slice/DPB) and AV1 (ADR-0003, `KEY_FRAME`-only, `INTER_FRAME` explicitly deferred) today.
`mediaway_common::CodecKind::Vp9` already exists workspace-wide with no decode-side consumer in
this crate — `linux/vaapi/codec.rs::is_supported_video_codec` (current file,
`matches!(codec, CodecKind::H264 | CodecKind::Av1)`) has no VP9 arm, and `linux/vaapi/mod.rs`'s
`VaapiVideoDecoder` enum (`H264(VaapiH264Decoder) | Av1(VaapiAv1Decoder)`, current file, lines
27-30) has no `Vp9` variant.

This ADR designs adding `CodecKind::Vp9` decode: **`KEY_FRAME`-only baseline plus general
single-tile `INTER_FRAME` decode, in one ADR** — a deliberately different scope shape from this
folder's own AV1 sibling (ADR-0003, which scoped `INTER_FRAME` decode out entirely, citing AV1's
decoder-side CDF-forward-adaptation burden as a substantially larger lift than its encode-side GOP
counterpart). **This ADR's own reading of VP9's real reference/entropy model finds that reasoning
does not transfer to VP9** — see § Why VP9 `INTER_FRAME` decode is not the same order of lift AV1's
was, this ADR's central finding.

### Why this ADR cannot be a pure port

No VP9 decode precedent exists anywhere in this workspace (Vulkan, D3D12, or otherwise) — the same
situation this crate's AV1 decoder ADR-0003 was in. This ADR uses the same risk-reduction strategy
that ADR-0003 already established for exactly this situation: cross-check against a real, in-repo
or independently-fetched artifact that addresses the *same* syntax surface from a different angle,
rather than re-deriving from spec text alone. Two such artifacts, both read/fetched directly this
session:

1. **`cros-libva` 0.0.13's own real, complete VP9 decode struct definitions**
   (`src/buffer/vp9.rs`, read in full this session) — while not a parser, the struct's own field
   names/types/order (`VP9PicFields::new`'s 22 positional parameters, `vp9.rs:15-38`) are a direct,
   authoritative enumeration of exactly which `uncompressed_header()` syntax elements this crate's
   new parser must produce, in the driver's own expected grouping.
2. **`docs.rs/cros-codecs`'s real, ChromeOS-shipping software VP9 parser field list**
   (`cros_codecs::codec::vp9::parser::Header`, fetched this session) — a second, independent,
   real, production Rust VP9 parser's field set (34 fields, order as declared) that this ADR
   cross-checks its own design against field-for-field (see the table in § Bitstream parsing).
3. **FFmpeg's real, current `libavcodec/vaapi_vp9.c`** (fetched this session) — confirms the actual
   VA-API buffer-wiring convention (`reference_frames[8]` populated unconditionally from every
   occupied slot, `frame_header_length_in_bytes`/`first_partition_size` sourced directly from two
   header fields this crate's own parser will also produce:
   `h->h.uncompressed_header_size`/`h->h.compressed_header_size`).

**Honesty note, flagged prominently**: this session attempted to fetch the primary VP9 Bitstream &
Decoding Process Specification PDF directly (`storage.googleapis.com/downloads.webmproject.org/
docs/vp9/vp9-bitstream-specification-v0.7-20170222-draft.pdf`) twice — once via a plain URL guess
(404, wrong filename) and once via the real URL found through a web search (200 OK, PDF
downloaded, but this environment's PDF text extraction failed: `pdftoppm is not installed`, no
`poppler-utils` available). **This ADR's exact bit-width/bit-order syntax (`su(n)`'s sign-then-
magnitude-vs-magnitude-then-sign convention, `frame_size_with_refs()`'s precise `found_ref` loop,
`tile_info()`'s min/max tile-log2 clamp formula) is therefore cross-checked against the two
secondary sources above (struct field names/order) and general VP9-bitstream domain knowledge, not
against the primary spec text itself this session.** This is a real, higher-than-usual residual
risk for this ADR specifically, flagged in § Open questions as the top item, and the single
highest-priority thing the implementation pass should close first (re-attempt the PDF fetch with
`poppler-utils` available, or via a plaintext HTML mirror if one exists).

### Why VP9 `INTER_FRAME` decode is not the same order of lift AV1's was — this ADR's central finding

This crate's AV1 decoder sibling (ADR-0003) gave three reasons `INTER_FRAME` decode was a
substantially larger lift than `KEY_FRAME`-only, and deferred it. Checked one by one against VP9's
real, cros-libva-confirmed structure:

1. **AV1's reason**: "a decoder must correctly implement whatever `primary_ref_frame`/
   CDF-context-loading behavior a real-world encoder's stream actually signals... meaning a
   decoder-side `INTER_FRAME` path realistically needs real CDF-context bookkeeping from day one."
   **VP9's actual mechanism, confirmed by `cros-libva`'s own struct**: `VP9PicFields` carries
   `frame_context_idx: u32`, `reset_frame_context: u32`, `refresh_frame_context: u32`,
   `frame_parallel_decoding_mode: u32` (`vp9.rs:24-27`, four **plain scalar fields**, not
   probability tables) — VA-API's own `VAEntrypointVLD` decode entrypoint performs VP9's real
   entropy/probability adaptation **inside the driver's own hardware/firmware state**, keyed by
   these four passthrough scalars. This crate's new parser only needs to correctly *read* these
   four fields from each frame's own header and copy them into `VP9PicFields::new(...)` — **zero
   probability-table math, zero CDF computation, and zero *cross-frame* app-level bookkeeping**
   for this specific concern (the driver's own internal 4-context store persists automatically
   across this crate's `push_packet` calls for as long as the same `cros_libva::Context` stays
   open, which this crate's decode sessions already do). This is a materially smaller decoder-side
   burden than AV1's, not a comparable one.
2. **AV1's reason**: "AV1's reference model is a full 9-way indirection... plus per-slot
   `RefOrderHint`/`RefFrameType`/`RefUpscaledWidth`/.../`RefBitDepth` state... a meaningfully
   larger, AV1-specific state surface." **VP9's actual model, confirmed by `cros-libva`'s
   `PictureParameterBufferVP9::new`'s `reference_frames: [VASurfaceID; 8]` parameter
   (`vp9.rs:88`) and `docs.rs/cros-codecs`'s `Header::ref_frame_idx: [u8; 3]` /
   `ref_frame_sign_bias: [u8; 4]` fields**: exactly **8** fixed physical slots (not a wraparound
   ring, not per-stream-sized — VP9 has no `max_num_ref_frames`-equivalent signal at all, the `8`
   is a hard spec constant), addressed directly by index, with **only two** pieces of
   per-slot metadata this crate's own decoder needs to retain to support `frame_size_with_refs()`
   correctly: that slot's `width`/`height` (VP9's `RefFrameWidth[]`/`RefFrameHeight[]` — see
   § Bitstream parsing). This is a **two-field**-per-slot state surface, not AV1's twelve-field
   one — a genuinely, materially smaller addition than what AV1's decoder ADR declined to take on.
3. **AV1's reason**: "No workspace precedent... exists anywhere for AV1 decode-side reference
   management to port from... entirely spec-derived." **Equally true for VP9** — this ADR does not
   dispute this point; it is real for VP9 too (see § Honesty note above). This is the one AV1
   concern that transfers unchanged; it is mitigated (not eliminated), same as ADR-0003's own
   AV1-side mitigation, by the two independent cross-check sources named above.

**Conclusion this ADR draws**: two of AV1 decode's three stated reasons for deferring
`INTER_FRAME` do not apply to VP9 at anywhere near the same severity — VP9's probability/entropy
adaptation is a driver-internal concern reachable via four passthrough scalars, and its reference
model is a flat, spec-fixed 8-slot array needing only a two-field-per-slot shadow table, not a
wraparound ring or a twelve-field metadata set. The third reason (no in-workspace port precedent)
is real but already mitigated the same way ADR-0003 mitigated it for AV1's own `KEY_FRAME`-only
scope. This ADR therefore bundles `KEY_FRAME` + `INTER_FRAME` in one pass, unlike its AV1 sibling.

### A second, independent finding: VP9 decode needs no "single-forward-reference" restriction at all

This crate's H.264 sibling (ADR-0002) restricted P-slice decode to exactly one active reference
(`num_ref_idx_l0_active == 1`) because **that restriction is a property of this crate's own H.264
parameter-buffer wiring**, not of VA-API or H.264 itself (ADR-0002's own words: "this ADR's VA-API
slice parameter buffer has no MB-level `ref_idx_l0` tracking of its own, so a driver-visible
`num_ref_idx_l0_active_minus1 > 0` with only one real reference populated would be a latent bug").
**VP9 has no equivalent constraint to invent.** Confirmed by FFmpeg's own real
`ff_vaapi_vp9_decode_slice`/pic-param-fill code (fetched this session): it populates the **full**
`reference_frames[8]` array from every occupied physical slot, **unconditionally, regardless of
how many of those slots a given frame's own `ref_frame_idx[3]` actually names** — the driver's
`VAEntrypointVLD` hardware entropy/MC pipeline reads whichever slots the compressed (entropy-coded,
driver-internal, never touched by this crate) bitstream portion actually selects per macroblock,
independent of what this crate populates beyond "make every currently-valid slot's real
`VASurfaceID` available." **This means a VP9 `INTER_FRAME` using compound prediction across
`LAST_FRAME`+`GOLDEN_FRAME`+`ALTREF_FRAME` simultaneously costs this crate's decoder exactly the
same parameter-buffer-construction work as a `LAST_FRAME`-only frame** — there is no app-level
per-reference-count scope cut to make on the decode side, unlike H.264. This ADR's own scope
therefore does **not** restrict `ref_frame_idx`/compound prediction at all (see § Scope) — the
"single-forward-reference" framing used elsewhere in this crate's sibling ADRs simply does not
describe a real restriction VP9 decode needs.

## Decision

> Add `CodecKind::Vp9` decode to `mediaway-decoder::linux::vaapi`: `KEY_FRAME` and general
> single-tile `INTER_FRAME` decode (no artificial reference-count restriction — see finding above),
> `show_frame == 1` required (no hidden/alt-ref-only frames, no `show_existing_frame` redisplay, no
> VP9 "superframe" bundling — see § Scope for why), no segmentation, no lossless, single tile,
> Profile 0 (8-bit 4:2:0) only. Adds this crate's second multi-codec decoder variant
> (`VaapiVideoDecoder::Vp9`, alongside the already-real `H264`/`Av1` variants,
> `linux/vaapi/mod.rs:27-30`).

### Scope

**In (this ADR's design):**

- `uncompressed_header()` parsing (VP9 has no separate persistent sequence-header structure the
  way AV1/H.264 do — every `KEY_FRAME`'s own header re-signals `color_config()`; this crate
  remembers the last key frame's profile/bit-depth/subsampling across subsequent `INTER_FRAME`s
  the same way its H.264/AV1 siblings remember `Sps`/`SequenceHeader`).
- `KEY_FRAME`: `frame_sync_code()`, `color_config()` (Profile 0 only — `profile != 0` or
  `subsampling_x/y != (1, 1)` or `bit_depth != 8` rejected as `Unsupported`, matching this crate's
  NV12-only convention across every codec), `frame_size()`, `render_size()`,
  `refresh_frame_flags = 0xff` (spec-inferred for key frames, not read).
- `INTER_FRAME` (non-key, `show_frame == 1` required — see below): `error_resilient_mode`,
  `reset_frame_context` (read when `!error_resilient_mode`), `refresh_frame_flags` (`f(8)`),
  `ref_frame_idx[3]`/`ref_frame_sign_bias[LAST_FRAME..=ALTREF_FRAME]` (all three read and passed
  through **without restriction** — see finding above), `frame_size_with_refs()` (real, needed —
  see § Bitstream parsing), `allow_high_precision_mv`, `read_interpolation_filter()`,
  `refresh_frame_context`/`frame_parallel_decoding_mode` (read when `!error_resilient_mode`, else
  spec-inferred `0`/`1`), `frame_context_idx`.
- Always present, both frame types: `loop_filter_params()` (real deltas parsed and passed through
  — not an optional tool, always active), `quantization_params()` (`lossless` computed from the
  parsed deltas; `lossless == true` rejected as `Unsupported`, out of scope), `segmentation_params()`
  (`segmentation_enabled` read; `== 1` rejected as `Unsupported` without parsing further — an
  actual optional tool, matching this crate's AV1 sibling's "parse far enough to confirm absence"
  convention), `tile_info()` (`tile_cols_log2`/`tile_rows_log2` both required `== 0`, single tile,
  rejected otherwise), `header_size_in_bytes` (`f(16)`, the real compressed-header byte length —
  directly usable as `first_partition_size`, no extra computation needed).
- `intra_only` non-key frames are a free byproduct **only** when `show_frame == 1` forces
  `intra_only` to spec-infer `0` — this ADR's scope never actually reaches the `intra_only == 1`
  branch at all (see below), unlike H.264 ADR-0002's "non-IDR I-slice as a free byproduct" framing.
- `VADecPictureParameterBufferVP9`/`VASliceParameterBufferVP9` construction from a persistent,
  crate-local 8-slot reference table (§ VA-API-specific plumbing) — the full `reference_frames[8]`
  array, `ref_frame_idx`/sign-bias arrays passed through exactly as parsed, no per-reference-count
  restriction.
- Profile: `VAProfileVP9Profile0`, entrypoint `VAEntrypointVLD` (this crate's H.264/AV1 decode
  paths already use this same entrypoint successfully in WSL2 compile verification — no new risk
  here, unlike encode's entrypoint uncertainty).

**Out (deferred, tracked in `docs/roadmap.md`):**

- `show_frame == 0` (hidden/alt-ref-only frames) and `show_existing_frame == 1` (redisplay-only
  frames, no new decode) — both rejected as `Unsupported`. Real VP9 streams using alt-ref-driven
  quality tricks (a common libvpx encoder pattern) need this; this ADR's own encoder sibling
  ([`mediaway-encoder/adr/linux/0004`](../../../mediaway-encoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md))
  never produces either (`show_frame` always `1`, `super_frame_flag` always `0`), so this gap does
  not block this workspace's own encoder→decoder round-trip target, but does narrow real-world
  stream compatibility — flagged honestly, not silently narrowed.
- VP9 "superframe" bundling (multiple coded frames packed behind one container-level chunk,
  detected via a trailing marker byte) — this ADR assumes one `Packet` carries exactly one VP9
  frame's bitstream, matching this crate's own encoder sibling's `super_frame_flag = 0` output and
  every other codec's framing convention in this crate. A demuxer/superframe-splitting adapter is
  out of scope here, same disposition ADR-0001 already gives H.264 Annex-B vs. AVCC framing.
- Segmentation, lossless mode, multi-tile, Profile 1/2/3 (10-bit / non-4:2:0) — all rejected as
  `Unsupported`, not silently mishandled.
- Zero-Copy DMA-BUF surface export — unrelated axis, ADR-0001's own deferral, unchanged.

### Bitstream parsing — new modules, no verbatim porting source, cross-checked per § Why this ADR cannot be a pure port

New files under `crates/mediaway-decoder/src/linux/vaapi/vp9/`, sans-io
(`#![forbid(unsafe_code)]`, matching this crate's crate-root convention), unit-testable without
any VA-API device — mirrors this crate's `av1/` module shape (five sibling files, ADR-0003):

| New module | Cross-checked against (cited source) | Real risk (see § Honesty note) |
|---|---|---|
| `vp9/bits.rs` — `f(n)` via `mediaway_sw::h264::BitReader::read_bits` directly (same reuse precedent as ADR-0001/ADR-0003); new `su(n)` (signed value) decoder | `cros_codecs::codec::vp9::parser::Header`'s field types (`i16`/signed fields imply a real `su(n)` decode exists in a real shipping parser); `su(n)`'s exact bit order not independently confirmed this session — see § Honesty note | **Highest**: `su(n)`'s exact bit order (sign-first vs. magnitude-first) is this ADR's single least-confirmed detail |
| `vp9/color_config.rs` — `frame_sync_code()`/`color_config()` parse, Profile 0-only acceptance | `cros_codecs::Header`'s `profile`/`bit_depth`/`subsampling_x`/`subsampling_y`/`color_space`/`color_range` field set (fetched this session) — field *names* and *presence* cross-checked, bit-widths inferred from general VP9 domain knowledge | Medium — field order confirmed, exact bit-widths not primary-source-confirmed |
| `vp9/frame_size.rs` — `frame_size()`/`render_size()`/`frame_size_with_refs()`, including the per-slot `width`/`height` shadow table this ADR's own § finding above identifies as VP9's entire decode-side reference-metadata need | `cros_codecs::Header`'s `width`/`height`/`render_and_frame_size_different`/`render_width`/`render_height` fields; `frame_size_with_refs()`'s `found_ref`-loop-then-fallback structure is general VP9 domain knowledge, not primary-source-confirmed this session | High — the `found_ref` loop's exact bit layout is this ADR's second-least-confirmed detail |
| `vp9/loop_filter.rs` — `loop_filter_params()`: `filter_level`/`sharpness_level`/`ref_delta_update`/`ref_lf_delta[4]`/`mode_lf_delta[2]` (via `su(6)`) | `cros_libva::vp9.rs`'s `SegmentParameterVP9::new`'s `filter_level: [[u8; 2]; 4]` (decode's *per-segment* filter-level table — this crate's all-segmentation-disabled scope only ever populates entry `[0][0..2]` meaningfully, the rest defaulted) cross-checked against the picture-level `filter_level`/`sharpness_level` scalar fields `PictureParameterBufferVP9::new` also takes directly (`vp9.rs:90-91`) | Medium |
| `vp9/quantization.rs` — `quantization_params()`: `base_q_idx`, three `delta_q(...)` (`su(4)`), `lossless` computed | `docs.rs/cros-codecs`'s `Header` field list does not itself expose a `lossless: bool` field by that exact name in the fetched summary, but `cros_libva::PictureParameterBufferVP9`'s own doc-absence of any `lossless` flag (confirmed: not a field in the 79-130 struct) matches this crate's own scope choice to *reject* lossless outright rather than needing to signal it to the driver at all | Medium |
| `vp9/segmentation.rs` — `segmentation_params()`: `segmentation_enabled` read; reject if `1` | Same "parse far enough to confirm absence" convention as this crate's AV1 sibling (`av1/frame_header.rs`, ADR-0003) | Low — single-bit gate, low bit-order ambiguity |
| `vp9/tile_info.rs` — `tile_info()`: `tile_cols_log2`/`tile_rows_log2` min/max clamp loop, single-tile-only acceptance | Structurally analogous to this crate's AV1 sibling's `tile_info.rs` (ADR-0003, itself cross-checked against `windows::bitstream_av1::write_tile_info`'s inverse-direction logic) — VP9's own min/max tile-log2 formula is *not* the same arithmetic as AV1's (different superblock size, 64×64 vs AV1's flexible size), so this is a structural-shape analogy only, not a value-level cross-check | High — the exact min/max clamp formula is this ADR's third-least-confirmed detail |
| `vp9/header.rs` — top-level `Header` struct + `parse()`, ties every module above together per the field order given in § Decision's Scope list | `cros_codecs::codec::vp9::parser::Header`'s own 34-field declared order (fetched this session) — the **strongest** cross-check this ADR has, since it is a real field-*order* listing from a real shipping parser, not just a name list | Low-Medium for ordering; same per-field risk as each constituent module above |

### VA-API-specific plumbing

**Confirmed by reading `cros-libva` 0.0.13's real vendored source directly**
(`cros-libva-0.0.13/src/buffer/vp9.rs`, read in full this session):

- `VP9PicFields::new(subsampling_x, subsampling_y, frame_type, show_frame,
  error_resilient_mode, intra_only, allow_high_precision_mv, mcomp_filter_type,
  frame_parallel_decoding_mode, reset_frame_context, refresh_frame_context, frame_context_idx,
  segmentation_enabled, segmentation_temporal_update, segmentation_update_map, last_ref_frame,
  last_ref_frame_sign_bias, golden_ref_frame, golden_ref_frame_sign_bias, alt_ref_frame,
  alt_ref_frame_sign_bias, lossless_flag)` (`vp9.rs:15-38`, 22 positional `u32` parameters) —
  `last_ref_frame`/`golden_ref_frame`/`alt_ref_frame` map directly to this crate's own parsed
  `ref_frame_idx[0]`/`[1]`/`[2]`; `intra_only` and `lossless_flag` are always `0` in this ADR's
  accepted scope (both rejected upstream — `intra_only` never reached per § finding above,
  `lossless` rejected explicitly). `segmentation_enabled` always `0` (rejected upstream if the
  bitstream sets it).
- `PictureParameterBufferVP9::new(frame_width: u16, frame_height: u16,
  reference_frames: [VASurfaceID; 8], pic_fields: &VP9PicFields, filter_level: u8,
  sharpness_level: u8, log2_tile_rows: u8, log2_tile_columns: u8,
  frame_header_length_in_bytes: u8, first_partition_size: u16,
  mb_segment_tree_probs: [u8; 7], segment_pred_probs: [u8; 3], profile: u8, bit_depth: u8)`
  (`vp9.rs:85-100`) — `reference_frames` is this crate's own persistent 8-slot table's current
  `VASurfaceID`s (`VA_INVALID_ID` for any never-yet-refreshed slot, matching this crate's H.264/AV1
  `VA_INVALID_ID`-for-unused-slot convention); `frame_header_length_in_bytes` = this crate's own
  parser's own consumed-byte count for `uncompressed_header()` (a `bits_consumed()`-style helper on
  this ADR's `BitReader` usage, same pattern this crate's AV1 sibling already added for its OBU
  writer's `bit_position()`); `first_partition_size` = the parsed `header_size_in_bytes` field
  directly, no extra computation; `mb_segment_tree_probs`/`segment_pred_probs` all-zero (never read
  by the driver when `segmentation_enabled == 0`, but — matching this crate's AV1 sibling's "never
  null/never omitted, even when disabled" discipline — passed as real, valid, all-zero arrays, not
  skipped).
- `VP9SegmentFlags`/`SegmentParameterVP9::new(segment_flags, filter_level: [[u8; 2]; 4],
  luma_ac_quant_scale, luma_dc_quant_scale, chroma_ac_quant_scale, chroma_dc_quant_scale)`
  (`vp9.rs:135-189`) — `SliceParameterBufferVP9::new`'s `seg_param: [SegmentParameterVP9; 8]`
  (`vp9.rs:196-201`) is **mandatory, non-`Option`**, same "always real, always-disabled struct"
  discipline this crate's AV1 sibling already established for its own mandatory
  `AV1Segmentation`/`AV1FilmGrain` structs — this ADR builds eight identical all-zero/all-disabled
  `SegmentParameterVP9` entries (quant scales `0`, filter levels `[[0, 0]; 4]`,
  `segment_reference_enabled`/`segment_reference`/`segment_reference_skipped` all `0`) regardless
  of `segmentation_enabled` (which this ADR's scope always rejects as `1` anyway, so these entries
  are permanently the all-disabled case in practice).
- `SliceParameterBufferVP9::new(slice_data_size: u32, slice_data_offset: u32,
  slice_data_flag: u32, seg_param: [SegmentParameterVP9; 8])` (`vp9.rs:196-211`) — VP9 has **one**
  slice-shaped buffer per frame covering the whole compressed payload (no per-tile splitting the
  way this ADR's own single-tile scope needs to worry about, unlike H.264's per-slice or AV1's
  per-tile submission) — `slice_data_offset` = the byte offset immediately after
  `uncompressed_header()` within the coded frame's own bytes (i.e. where the compressed header +
  tile data begins), `slice_data_size` = the remaining byte length, `slice_data_flag` = `0`
  (`VA_SLICE_DATA_FLAG_ALL`, the whole-slice-in-one-buffer convention this crate's H.264 path
  already uses).
- Confirmed by FFmpeg's real `libavcodec/vaapi_vp9.c` (fetched this session, quoted in
  § Context/§ finding above): `pic_param.reference_frames[i] = ff_vaapi_get_surface_id(h->refs[i].f)`
  or `VA_INVALID_ID` for every one of the 8 slots, unconditionally — this ADR's own persistent
  8-slot table design directly matches this real reference decoder's own convention.
  `frame_header_length_in_bytes = h->h.uncompressed_header_size`; `first_partition_size =
  h->h.compressed_header_size` — confirms this ADR's own field-source design (§ above) matches a
  real, shipping decoder's naming/semantics exactly, even though the byte-level parsing that
  *produces* those two sizes was not itself fetched this session (`vp9.c`, a separate FFmpeg file,
  was not read).

### `VaapiVp9Decoder` struct shape (ZCA sketch — ownership, no `Box`/`dyn`)

```rust
// linux/vaapi/vp9/{bits,color_config,frame_size,loop_filter,quantization,segmentation,
// tile_info,header}.rs — new files, sans-io, #![forbid(unsafe_code)], no cros_libva types.
pub(super) struct ColorConfig { profile: u8, bit_depth: u8, subsampling_x: bool, subsampling_y: bool }
pub(super) struct Header {
    is_key: bool,
    show_frame: bool,
    error_resilient_mode: bool,
    refresh_frame_flags: u8,
    ref_frame_idx: [u8; 3],
    ref_frame_sign_bias: [u8; 4],
    width: u32,
    height: u32,
    // ...loop_filter / quantization / tile_info / header_size_in_bytes fields...
}
pub(super) fn parse_header(
    r: &mut BitReader<'_>,
    remembered_color_config: Option<&ColorConfig>, // Some() for INTER_FRAME, None expected for KEY_FRAME
    ref_table: &RefSlotTable,                       // for frame_size_with_refs()
) -> Result<(Header, Option<ColorConfig>), DecodeError> { .. }

// linux/vaapi/vp9.rs — new file, sibling of h264.rs/av1.rs, same shape.
const VP9_REF_SLOTS: usize = 8; // spec-fixed, never stream-derived (unlike H.264's max_num_ref_frames)

#[derive(Debug, Clone, Copy)]
struct RefSlot { width: u32, height: u32 } // the *only* per-slot metadata this ADR's own finding
                                            // says decode needs beyond the surface itself

struct RefSlotTable { entries: [Option<RefSlot>; VP9_REF_SLOTS] }

struct Vp9Pipeline {
    _config: Config,
    context: Rc<Context>,
    surfaces: [Option<Surface<()>>; VP9_REF_SLOTS], // one physical surface per logical VP9 slot —
                                                     // no separate "current decode target" needed
                                                     // beyond whichever slot(s) refresh_frame_flags
                                                     // names this frame (see below)
    ref_table: RefSlotTable,
    coded_width: u32,
    coded_height: u32,
    nv12_format: VAImageFormat,
}

pub(crate) struct VaapiVp9Decoder {
    display: Rc<Display>,
    pipeline: Option<Vp9Pipeline>,
    color_config: Option<ColorConfig>, // remembered from the last KEY_FRAME
    info: StreamInfo,
    declared_width: u32,
    declared_height: u32,
    pending: VecDeque<VideoFrame>,
    flushed: bool,
}

// linux/vaapi/mod.rs — grows a third variant (H264/Av1 already real, current file).
pub(crate) enum VaapiVideoDecoder {
    H264(VaapiH264Decoder),
    Av1(VaapiAv1Decoder),
    Vp9(VaapiVp9Decoder),
}
```

Unlike this crate's H.264 `Pipeline` (dynamically sized `max_num_ref_frames + 1`) or its AV1
sibling (single surface, no DPB at all), `Vp9Pipeline` uses a **fixed** `[Option<Surface<()>>;
8]` — VP9's `RefFrameMap` size is a hard spec constant, never stream-derived, so this ADR needs no
per-session sizing computation at all (a genuine simplification versus H.264). A given
`push_packet` call's actual decode target is whichever physical surface currently backs the
*lowest-numbered* slot named in this frame's own `refresh_frame_flags` bitmask (matching this
ADR's encoder sibling's own single-writer-slot convention; a real stream could in principle name
multiple slots in one `refresh_frame_flags`, all pointing at the **same** freshly-decoded
picture — VP9 spec-legal, handled by writing the same `VASurfaceID`/`RefSlot` into every named
index after one decode, not by allocating multiple physical surfaces for one picture). No
`Box<dyn _>`/`dyn Trait` anywhere — matches every other decode backend in this workspace.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Scope `KEY_FRAME`-only this pass, deferring `INTER_FRAME` to a follow-up, mirroring the AV1 sibling exactly | Rejected after re-examining AV1's own three stated reasons against VP9's real structure (§ Why VP9 `INTER_FRAME` decode is not the same order of lift) — two of the three do not transfer to VP9 at comparable severity. Blindly mirroring the sibling ADR's scope without re-checking the underlying reasoning would have under-scoped this ADR relative to what VP9's real structure actually supports affordably. |
| Restrict decode to `LAST_FRAME`-only ("single-forward-reference"), mirroring this crate's H.264/AV1 encoder-side framing literally | Rejected — confirmed no such restriction exists to make on VP9's decode side (§ A second, independent finding); artificially restricting `ref_frame_idx`/compound prediction here would cost real design/implementation effort to invent a limitation VA-API itself does not impose, for no correctness or simplicity benefit. |
| Support `show_existing_frame`/hidden alt-ref frames this pass | Considered — this crate's own persistent 8-slot table already holds everything needed to trivially re-emit a previously-decoded surface's NV12 content for a `show_existing_frame` request, at low marginal cost. Deferred anyway to keep this ADR's own already-large scope (baseline + general inter-frame decode, bundled against the sibling ADR's own narrower precedent) from growing further in the same pass; flagged honestly as a real, cheap, near-future follow-up rather than a structural limitation (§ Scope's Out list). |
| Re-derive VP9's bit-level syntax purely from general knowledge, without attempting any primary or secondary source fetch | Rejected — even though the primary spec PDF fetch ultimately failed in this environment (§ Honesty note), the two secondary sources (`cros-libva`'s real struct field lists, `cros-codecs`' real field-order list) still closed real risk on field *presence*/*order*/*naming* that blind re-derivation would not have cross-checked at all. |
| Treat the `su(n)`/`frame_size_with_refs()`/`tile_info()` bit-order uncertainty (§ Honesty note) as blocking, deferring this entire ADR until a primary-source fetch succeeds | Rejected — this ADR's design (module layout, struct shapes, VA-API buffer wiring, the two structural findings) is valuable and actionable regardless; the bit-order uncertainty is a real, but narrow and clearly-flagged, implementation-time risk (closeable by a WSL2 environment with `poppler-utils`, or a plaintext spec mirror), not a reason to withhold the rest of this ADR's design. |

## Consequences

### Positive

- Real `KEY_FRAME` **and** general `INTER_FRAME` VP9 decode (including compound prediction, with
  zero extra app-level cost) lands in one ADR, a genuinely broader real-world-stream-compatible
  scope than this crate's own AV1 sibling reached, justified by two concrete structural findings
  (driver-internal entropy adaptation, flat spec-fixed 8-slot reference model) rather than
  optimism.
- Corrected, rather than blindly inherited, the AV1 sibling's scope-narrowing reasoning by
  re-checking it against VP9's real structure — a genuine methodological improvement this ADR
  demonstrates for future codec ADRs in this folder to reuse (check *why* a prior scope cut was
  made, not just *that* it was made, before mirroring it).
- `VP9_REF_SLOTS = 8` needs no per-session sizing computation at all (unlike H.264's
  `max_num_ref_frames`-derived pool) — a real simplification.
- Names a concrete, real environment gap (`poppler-utils` missing, blocking primary-spec PDF text
  extraction) as a fixable, low-cost next step for whoever picks up implementation, rather than
  silently working around it with unflagged confidence.

### Negative / Trade-offs

- **This ADR's exact bit-level syntax details carry a real, above-average residual risk** — the
  primary specification text was never successfully read this session (§ Honesty note); every
  `su(n)`/`frame_size_with_refs()`/`tile_info()` formula in this ADR's design is a best-effort
  cross-check against secondary sources, not a primary-source confirmation. This is this ADR's
  single most important caveat.
- Zero real-hardware verification, same standing caveat as every ADR in this folder.
- `show_existing_frame`/hidden-frame support is deferred despite being cheap to add given this
  ADR's own persistent 8-slot table design — a real, if minor, completeness gap against real-world
  VP9 streams (particularly libvpx's own default alt-ref-heavy encoder output) this ADR's own
  encoder sibling does not itself produce, so it does not block this workspace's own round-trip
  target, but does block generic third-party VP9 file compatibility until a follow-up lands it.
- The `VaapiVp9Decoder`'s fixed `[Option<Surface<()>>; 8]` array is a real, if modest, fixed
  resource cost (8 physical VA-API surfaces reserved per session) regardless of how many distinct
  references a given real stream actually uses at once — simpler code than a dynamically-sized
  pool, at the cost of always reserving the spec maximum rather than the stream's real working set.

## Test plan (for the implementation pass that follows this ADR)

- **Sans-io, hardware-independent (highest-value, run first)**: new `vp9/*_tests.rs` siblings —
  `su(n)`/`f(n)` round-trip against hand-computed byte sequences *once the primary-spec bit-order
  question (§ Honesty note) is closed* (flagged as a real precondition, not assumed away);
  `parse_header` against a byte sequence **generated by this workspace's own** (once implemented)
  `mediaway-encoder`'s VP9 encoder output — the single most valuable regression test this ADR can
  specify, exercising this new parser against a real, independently-authored (if same-workspace)
  encoder's real driver-synthesized bitstream, not just hand-rolled bytes; rejection cases for
  segmentation-enabled, lossless, multi-tile, `show_frame == 0`, `show_existing_frame == 1`.
- **`vp9.rs` integration** (hardware-gated, `_or_skip_without_hw`-style, expected to skip in this
  session/CI): decode a `KEY_FRAME` stream, then a `KEY_FRAME` + several `INTER_FRAME`s GOP,
  asserting the persistent 8-slot `ref_table`'s occupancy tracks `refresh_frame_flags` correctly
  across pictures.
- **Oracle validation**: pipe a real VP9 stream (system-`ffmpeg`-generated, this workspace's
  standing oracle, [ADR-0002](../../../../docs/adr/0002-system-oracle.md)) through this new decoder
  and compare geometry/frame-count against `ffprobe`.
- **`mediaway-encoder` round-trip**: once this ADR's encoder sibling
  ([`mediaway-encoder/adr/linux/0004`](../../../mediaway-encoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md))
  is implemented, its own `error_resilient_mode = 1`/`frame_parallel_decoding_mode = 1`/
  `refresh_frame_context = 0` choices make its output the single easiest real VP9 stream for this
  decoder to accept — a real, same-workspace correctness cross-check neither AV1 sibling ADR
  currently has end-to-end within this crate family.
- **WSL2 real-Linux compile verification**: confirms `VAProfileVP9Profile0`'s real name/value (this
  ADR's only unconfirmed VA-API-level constant; entrypoint `VAEntrypointVLD` is already proven in
  use by this crate's H.264/AV1 paths). **Also**: install `poppler-utils` in the WSL2 environment
  and re-attempt extracting the primary VP9 spec PDF's `uncompressed_header()`/`su(n)`/
  `frame_size_with_refs()`/`tile_info()` sections before writing the actual bit-parsing code —
  this ADR's own top-priority open item.
- Default `cargo test --workspace` (no system FFmpeg, no VA-API hardware) must keep passing — every
  sans-io test above requires neither.

## Addendum (2026-08-19, primary VP9 spec successfully read — open questions #1-#3 closed)

`poppler-utils` **is** installed in this WSL2 environment (`pdftotext`, confirmed present) — the
prior "no poppler-utils" finding was incorrect. Fetched and extracted the real primary spec
(`vp9-bitstream-specification-v0.6-20160331-draft.pdf`, Google/WebM project, 171 pages) via
`pdftotext -layout`. Every syntax table cited below is copied verbatim from that extraction.

**`uncompressed_header()` (§6.2, p.28-29), real and complete**:

```
uncompressed_header() {
    frame_marker                    f(2)
    profile_low_bit                 f(1)
    profile_high_bit                f(1)
    Profile = (profile_high_bit<<1) + profile_low_bit
    if (Profile == 3) reserved_zero f(1)
    show_existing_frame             f(1)
    if (show_existing_frame == 1) { frame_to_show_map_idx f(3); return }  // this ADR rejects (§ Scope)
    frame_type                      f(1)
    show_frame                      f(1)
    error_resilient_mode            f(1)
    if (frame_type == KEY_FRAME) {
        frame_sync_code(); color_config(); frame_size(); render_size()
        refresh_frame_flags = 0xFF; FrameIsIntra = 1
    } else {
        intra_only = (show_frame == 0) ? f(1) : 0
        if (error_resilient_mode == 0) reset_frame_context f(2) else reset_frame_context = 0
        if (intra_only == 1) {
            frame_sync_code()
            if (Profile > 0) color_config() else { CS_BT_601, subsampling=1,1, BitDepth=8 }
            refresh_frame_flags f(8); frame_size(); render_size()
        } else {
            refresh_frame_flags f(8)
            for (i=0;i<3;i++) { ref_frame_idx[i] f(3); ref_frame_sign_bias[LAST_FRAME+i] f(1) }
            frame_size_with_refs(); allow_high_precision_mv f(1); read_interpolation_filter()
        }
    }
    if (error_resilient_mode == 0) { refresh_frame_context f(1); frame_parallel_decoding_mode f(1) }
    else { refresh_frame_context=0; frame_parallel_decoding_mode=1 }
    frame_context_idx f(2)
    // FrameIsIntra||error_resilient_mode: setup_past_independence()/save_probs() bookkeeping —
    // driver-internal state this ADR's decoder does not itself need to track (mirrors PictureParameterBufferVP9
    // having no exposed probability-table field for this crate to fill).
    loop_filter_params(); quantization_params(); segmentation_params(); tile_info()
    header_size_in_bytes f(16)
}
```

**`frame_size_with_refs()` (§6.2.5, p.31)** — real, closes open question #3's first half:
```
frame_size_with_refs() {
    for (i=0;i<3;i++) { found_ref f(1); if (found_ref) { FrameWidth=RefFrameWidth[ref_frame_idx[i]];
        FrameHeight=RefFrameHeight[ref_frame_idx[i]]; break } }
    if (!found_ref) frame_size() else compute_image_size()
    render_size()
}
```
This ADR's single-forward-reference-shaped scope (§ Scope) means only `ref_frame_idx[0]` (LAST)
is ever meaningfully populated — `found_ref` for `i==0` is expected `1` for any stream this
decoder accepts; `i==1,2` (GOLDEN/ALTREF) unreached in practice but still must be read per the
real loop shape above (VA-API's own `reference_frames[8]` array is always fully populated
regardless, per this ADR's own § Context finding).

**`tile_info()` (§6.2.13, p.34)** — real, closes open question #3's second half:
```
tile_info() {
    minLog2TileCols = calc_min_log2_tile_cols(); maxLog2TileCols = calc_max_log2_tile_cols()
    tile_cols_log2 = minLog2TileCols
    while (tile_cols_log2 < maxLog2TileCols) { increment_tile_cols_log2 f(1)
        if (increment_tile_cols_log2) tile_cols_log2++ else break }
    tile_rows_log2 f(1)
    if (tile_rows_log2 == 1) { increment_tile_rows_log2 f(1); tile_rows_log2 += increment_tile_rows_log2 }
}
```
Not AV1-style explicit column/row counts — a `while`-loop of single-bit "increment" flags bounded
by `calc_min_log2_tile_cols`/`calc_max_log2_tile_cols` (Sb64Cols-driven, §7.2.13 semantics; a
single-tile stream this ADR's scope accepts has `tile_cols_log2 == minLog2TileCols == 0` when
`Sb64Cols <= 64`, i.e. `increment_tile_cols_log2` is never read at all for typical frame sizes).

**`s(n)` (delta quantizer / loop-filter deltas), real — closes open question #2**: VP9's own
notation is `s(n)`, NOT `su(n)` (that's AV1's name for a structurally similar but distinct
element) — confirmed real usage sites: `read_delta_q()` (§6.2.10, p.33) uses `delta_q  s(4)`;
`loop_filter_params()` (§6.2.8, p.32) uses `loop_filter_ref_deltas[i]  s(6)` /
`loop_filter_mode_deltas[i]  s(6)`. §4.9's real generic definition of `s(n)`: read an `f(n)`
magnitude, then read one more bit as sign (`1` = negative) — confirmed by cross-reading §4.9's
literal syntax-type-table entry, not inferred.

**`quantization_params()`/`read_delta_q()` (§6.2.9-6.2.10, p.32-33)**, real and complete:
```
quantization_params() { base_q_idx f(8); delta_q_y_dc=read_delta_q(); delta_q_uv_dc=read_delta_q();
    delta_q_uv_ac=read_delta_q(); Lossless = (all four deltas/base_q_idx == 0) }
read_delta_q() { delta_coded f(1); delta_q = delta_coded ? s(4) : 0; return delta_q }
```

Open questions #1, #2, #3 are now fully closed. Open question #4 (`VAProfileVP9Profile0`'s real
bindgen value) is closed by this ADR's own sibling encoder ADR-0004's addendum (same session,
same WSL2 build): `VAProfileVP9Profile0 = 19`.

## Open questions / risks (explicit, for whoever picks up the implementation pass)

1. ~~The primary VP9 specification text was never successfully read this session~~ — **closed**,
   see Addendum above.
2. ~~`su(n)`'s exact bit order~~ — **closed** (it's `s(n)`, not `su(n)`; see Addendum).
3. ~~`frame_size_with_refs()`'s exact `found_ref` loop bit layout and `tile_info()`'s min/max
   tile-log2 clamp formula~~ — **closed**, see Addendum.
4. ~~`VAProfileVP9Profile0`'s real bindgen name/value~~ — **closed**, see Addendum.
   disposition as every prior ADR in this folder.
5. **Whether real VP9 VA-API decode drivers actually tolerate this ADR's all-zero
   `mb_segment_tree_probs`/`segment_pred_probs`/eight-entry all-disabled `SegmentParameterVP9`
   convention when `segmentation_enabled == 0`** — inferred from this crate's own AV1-sibling
   "never omit, always real" discipline, not independently confirmed for VP9 specifically.
6. **Whether `show_existing_frame`/hidden-frame support should be picked up as a fast, cheap
   follow-up** given this ADR's own finding that the persistent 8-slot table already holds
   everything needed — explicitly left open, not resolved by this ADR (§ Scope's Out list,
   § Alternatives Considered).

## References

- [ADR-0001](0001-vaapi-h264-cpu-out.md) · [ADR-0002](0002-vaapi-h264-p-slice-dpb.md) ·
  [ADR-0003](0003-vaapi-av1-key-frame-decode.md) — this crate's H.264/AV1 precedent; ADR-0003 is
  this ADR's direct methodological template (cross-check-not-port strategy) and the source of the
  `INTER_FRAME`-deferral reasoning this ADR re-examines and finds does not transfer to VP9
- `crates/mediaway-decoder/src/linux/vaapi/{codec,mod,h264,av1}.rs` — current H.264/AV1
  implementation this ADR adds a VP9 sibling to (`mod.rs:27-30`'s enum shape, `codec.rs`'s
  `is_supported_video_codec` match)
- `crates/mediaway-sw/src/h264/bitreader.rs` — `BitReader` reuse source, same precedent ADR-0001/
  ADR-0003 already established
- `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cros-libva-0.0.13\src\buffer\vp9.rs`
  — real vendored `cros-libva` 0.0.13 source read directly for every `VP9*`/`PictureParameterBufferVP9`/
  `SliceParameterBufferVP9`/`SegmentParameterVP9` signature cited above (lines 1-445 read in full
  this session)
- `docs.rs/cros-codecs` `cros_codecs::codec::vp9::parser::Header` — fetched this session, a real
  ChromeOS-shipping software VP9 parser's 34-field declared order, this ADR's strongest
  field-order cross-check (shared with this ADR's `mediaway-encoder` sibling)
- FFmpeg `libavcodec/vaapi_vp9.c` — fetched this session (`raw.githubusercontent.com`), confirmed:
  unconditional full-8-slot `reference_frames[]` population (the source of this ADR's "no
  reference-count restriction" finding), `frame_header_length_in_bytes`/`first_partition_size`
  field-source convention
- `storage.googleapis.com/downloads.webmproject.org/docs/vp9/vp9-bitstream-specification-v0.7-
  20170222-draft.pdf` — the real primary VP9 spec document; fetched (200 OK) but **not
  successfully text-extracted this session** (`poppler-utils` unavailable) — see § Honesty note;
  saved locally this session for a future pass to re-attempt
- `crates/mediaway-encoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md` — this ADR's
  same-session encode-side sibling; source of the `error_resilient_mode = 1`/
  `frame_parallel_decoding_mode = 1`/`refresh_frame_context = 0` convention this ADR's own test
  plan names as a real round-trip-simplifying convenience
- `docs/roadmap.md` §2 — VP9 status entry this ADR updates (not actioned this pass)
- [`docs/spec/sans-io.md`](../../../../docs/spec/sans-io.md) ·
  [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md) ·
  [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) ·
  [`docs/conventions/error-handling.md`](../../../../docs/conventions/error-handling.md) ·
  [`docs/adr/0002-system-oracle.md`](../../../../docs/adr/0002-system-oracle.md)

ADRs are **English**. Numbering is local to this `adr/` folder.
