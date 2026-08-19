# ADR-0004: VA-API VP9 encode — `KEY_FRAME`-only baseline + single-forward-reference `INTER_FRAME` GOP (spec/FFmpeg-derived; **not blocked**, unlike this folder's AV1 sibling)

- **Status**: Accepted — implemented (`src/linux/vaapi/vp9.rs` + `src/linux/vaapi/vp9_gop.rs`),
  compile + clippy (`--all-targets -- -D warnings`) + test-verified on real WSL2 Linux
  (`cargo test -p mediaway-encoder --all-features --target x86_64-unknown-linux-gnu`,
  2026-08-19). The addendum's real-bindgen-confirmed `VAProfileVP9Profile0 = 19` and the
  3-step `EncSlice → EncPicture → EncSliceLP` entrypoint probe correction both held; every other
  field/struct assumption in § VA-API-specific plumbing compiled correctly against real
  `cros-libva` 0.0.13 vendored source on the first implementation pass — no further ADR-vs-reality
  mismatch found. **Not blocked** — unlike [ADR-0005](0005-vaapi-av1-key-frame-and-inter-gop.md)
  (AV1), this ADR found no `cros-libva` gap preventing implementation. See § Why VP9 does not
  share AV1's packed-header blocker. **Zero real-hardware verification** remains — see § Zero
  real-hardware verification remains the honest baseline.
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`src/linux/vaapi/`)

## Context

`mediaway-encoder::linux::vaapi` encodes H.264 only today (ADR-0001 baseline, ADR-0002 GOP); AV1
support is designed but blocked (ADR-0003). `mediaway_common::CodecKind::Vp9` already exists
workspace-wide (used by no encoder backend in this workspace yet — a real `grep` for
`CodecKind::Vp9` across `mediaway-encoder::vulkan`/`::windows` returns no encode-side match) —
`codec.rs::video_profile`/`is_supported_video_codec` (current file, `codec.rs:12-27`) have no VP9
arm, matching its H.264-only doc comment ("HEVC / AV1 / VP9 are deferred").

This ADR designs adding `CodecKind::Vp9` support to this crate: a `KEY_FRAME`-only baseline plus,
**in the same ADR**, single-forward-reference `INTER_FRAME` GOP structure — both together, mirroring
this folder's own AV1 ADR-0003 precedent for bundling baseline + GOP in one pass, but for a
**materially different, more favorable reason**: VP9 has no existing same-crate-family GOP-state
precedent to port (unlike AV1's `vulkan::av1_gop`), so this ADR's GOP design is derived directly
from a real, cited, currently-shipping reference implementation (FFmpeg's `vaapi_encode_vp9.c`,
fetched and quoted verbatim this session) rather than ported from Mediaway's own code — see
§ Why this ADR cannot be a verbatim port, and what closes that gap instead.

### Why VP9 does not share AV1's packed-header blocker

This crate's AV1 encode ADR-0003 found a real, confirmed `cros-libva` 0.0.13 gap: AV1 VA-API
encode requires the application to hand-construct and submit real `frame_header_obu()` bytes via a
packed-header buffer type `cros-libva` does not wrap. **This is not true for VP9.** Confirmed by
reading `cros-libva` 0.0.13's real vendored `src/buffer/vp9.rs` in full this session
(`C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cros-libva-0.0.13\src\buffer\vp9.rs`,
lines 1-445) and cross-checking against FFmpeg's real, current `libavcodec/vaapi_encode_vp9.c`
(fetched this session, `raw.githubusercontent.com`):

- `EncSequenceParameterBufferVP9::new(max_frame_width, max_frame_height, kf_auto, kf_min_dist,
  kf_max_dist, bits_per_second, intra_period)` (`vp9.rs:244-271`) and
  `EncPictureParameterBufferVP9::new(...)` (`vp9.rs:367-444`, the full 26-parameter constructor,
  taking `&VP9EncRefFlags`/`&VP9EncPicFlags` bitfield wrappers) are **plain C-struct field
  bags** — the same shape H.264/HEVC's encode buffers already have in this crate (ADR-0001/0002's
  own framing), unlike AV1's writer-fed struct.
- **`cros-libva`'s `EncSliceParameter` enum (`buffer.rs:436-443`) has no `VP9` variant** —
  `H264`/`HEVC`/`AV1` only. This is confirmed **not** a `cros-libva` gap, unlike AV1's missing
  packed-header variant: FFmpeg's own `vaapi_encode_vp9.c` never creates a
  `VAEncSliceParameterBufferVP9` at all — real libva has no such buffer type for VP9 encode. VP9
  encode submits exactly two buffers per session/frame
  (`VAEncSequenceParameterBufferVP9`/`VAEncPictureParameterBufferVP9`) plus the shared
  `VAEncCodedBufferType` output buffer — no third, slice-shaped buffer exists to omit.
- FFmpeg's own file comment, quoted verbatim (fetched this session, `vaapi_encode_vp9.c`): **"No
  packed headers are currently desired. They could be written, but there isn't any reason to do so
  — the one usable driver (i965) can write its own headers and there is no metadata to include."**
  This directly confirms VP9's own bitstream design lets the driver synthesize the real VP9
  `uncompressed_header()`/compressed-header bytes from the flat picture-parameter struct fields
  alone — the same driver-synthesizes-headers model this crate's H.264 path already relies on, not
  AV1's app-writes-bytes model. **This is this ADR's single most important finding**: VP9 encode is
  genuinely implementable against `cros-libva` 0.0.13 as pinned today, with **zero** new
  `cros-libva` capability needed.
- `EncPictureParameterBufferVP9` does carry seven `bit_offset_*`/`bit_size_segmentation` trailing
  fields (`bit_offset_ref_lf_delta`, `bit_offset_mode_lf_delta`, `bit_offset_lf_level`,
  `bit_offset_qindex`, `bit_offset_first_partition_size`, `bit_offset_segmentation`,
  `bit_size_segmentation`, `vp9.rs:390-396`) — superficially similar in *name* to AV1's
  `bit_offset_*` fields, which raised a real concern this ADR checked carefully. **Confirmed by
  reading FFmpeg's `vaapi_encode_vp9_init_picture_params` function body in full this session
  (quoted verbatim below): none of these seven fields is ever set anywhere in that function** — the
  `VAEncPictureParameterBufferVP9 vpic` struct FFmpeg builds is otherwise fully populated
  (`reconstructed_frame`, `coded_buf`, `ref_flags`, `refresh_frame_flags`, `reference_frames`,
  `pic_flags`, `luma_ac_qindex`, `filter_level`, `sharpness_level`, `log2_tile_columns`) but the
  seven `bit_offset_*`/`bit_size_segmentation` fields are left at their zero-initialized default.
  Since "the one usable driver (i965) can write its own headers" per FFmpeg's own comment, these
  fields are real libva API surface that at least one real, shipping VP9 encoder implementation
  simply never needs to populate — this ADR designs the same convention (pass `0` for all seven),
  flagged explicitly in § Scope as a deliberate "match the only known-working reference
  implementation's convention" choice, not an independently-derived one.

### Why this ADR cannot be a verbatim port, and what closes that gap instead

Unlike this crate's H.264 GOP (ADR-0002, verbatim-ported `vulkan::h264_gop::GopState`) or AV1 GOP
(ADR-0003, verbatim-ported `vulkan::av1_gop::GopState`), **no VP9 GOP-state precedent exists
anywhere in this workspace** — confirmed by a full-workspace check: no `vp9_gop.rs`/`vp9_params.rs`
exists under `mediaway-encoder::vulkan` or `::windows`, and `mediaway_common::CodecKind::Vp9`
today has no encode-side consumer at all (see § Context). This ADR's GOP design is instead a
**direct, cited cross-check against FFmpeg's own real, current, shipping VP9 VA-API encoder**
(`libavcodec/vaapi_encode_vp9.c`, fetched and quoted verbatim this session) — the same
"in-repo/real-reference-implementation-derived, not independently re-derived from spec text alone"
risk-reduction this crate's AV1 decoder sibling (`mediaway-decoder/adr/linux/0003`) already used
when no in-workspace decode precedent existed either. Quoted verbatim, the entire
`FF_HW_PICTURE_TYPE_IDR`/`FF_HW_PICTURE_TYPE_P` branch of
`vaapi_encode_vp9_init_picture_params` (the function this ADR's GOP design directly tracks;
`FF_HW_PICTURE_TYPE_B` is out of this ADR's scope, see § Scope):

```c
switch (pic->type) {
case FF_HW_PICTURE_TYPE_IDR:
    av_assert0(pic->nb_refs[0] == 0 && pic->nb_refs[1] == 0);
    vpic->ref_flags.bits.force_kf = 1;
    vpic->refresh_frame_flags = 0xff;
    hpic->slot = 0;
    break;
case FF_HW_PICTURE_TYPE_P:
    av_assert0(!pic->nb_refs[1]);
    {
        VAAPIEncodeVP9Picture *href = pic->refs[0][0]->codec_priv;
        av_assert0(href->slot == 0 || href->slot == 1);
        if (base_ctx->max_b_depth > 0) {
            hpic->slot = !href->slot;
            vpic->refresh_frame_flags = 1 << hpic->slot | 0xfc;
        } else {
            hpic->slot = 0;
            vpic->refresh_frame_flags = 0xff;
        }
        vpic->ref_flags.bits.ref_frame_ctrl_l0  = 1;
        vpic->ref_flags.bits.ref_last_idx       = href->slot;
        vpic->ref_flags.bits.ref_last_sign_bias = 1;
    }
    break;
```

Two things this quote confirms directly, both load-bearing for this ADR's design: (1) FFmpeg's own
B-frame-disabled path (`base_ctx->max_b_depth == 0`, this ADR's own scope) keeps every P frame at
`hpic->slot = 0` and `refresh_frame_flags = 0xff` — i.e. **without B-frames, FFmpeg's own real
encoder does not even need the 2-slot ping-pong** shown in the `max_b_depth > 0` branch; every
frame (`IDR` and `P` alike) writes into logical slot `0` and refreshes all 8 VP9 reference-frame
slots unconditionally. This ADR adopts the *ping-pong* branch instead (not the simpler
always-slot-0 branch) — see § Scope for why: `refresh_frame_flags = 0xff` on every P frame would
mean a P frame's own encode always overwrites slot `0` while `href->slot` (the frame it just
referenced) still logically holds slot `0` too, an aliasing case that only stays correct because
VP9 hardware encode treats `reconstructed_frame`/`reference_frames[href->slot]` as read-before-write
within one `vaEndPicture` call — this ADR's own physical `Surface` pool (§ VA-API-specific
plumbing) cannot safely alias in the same way without its own new correctness proof, so this ADR
deliberately keeps the 2-slot ping-pong shape unconditionally, even though a real B-frame-disabled
FFmpeg session does not strictly need it. (2) `ref_flags.bits.ref_last_sign_bias = 1` is set
unconditionally for every P frame — this ADR adopts the identical constant.

### Zero real-hardware verification remains the honest baseline

Same standing caveat as every ADR in this folder: `Display::open()`/`vaInitialize` has never
succeeded against real hardware in this environment (Windows dev box; WSL2's own VA-API is broken,
`vainfo` segfaults, no real GPU exposed). This ADR ships design-only, no `.rs` files this pass — the
implementation pass that follows must run real WSL2 `cargo check`/`cargo clippy --all-targets -- -D
warnings`/`cargo test` before claiming even compile correctness for the `VAProfileVP9Profile0`
constant name/value this ADR assumes (§ Open questions).

## Decision

> Add `CodecKind::Vp9` to `mediaway-encoder::linux::vaapi`: `KEY_FRAME`-only baseline (default,
> `gop_size <= 1`) plus single-forward-reference `INTER_FRAME` GOP (`gop_size > 1`,
> capability-gated), by (1) designing a new, FFmpeg-cross-checked (not ported — no precedent
> exists) `linux/vaapi/vp9_gop.rs` GOP state machine using VP9's real fixed 8-slot reference model
> with a 2-slot physical ping-pong; (2) wiring `cros-libva::EncSequenceParameterBufferVP9`/
> `EncPictureParameterBufferVP9`/`VP9EncRefFlags`/`VP9EncPicFlags` directly (plain C-struct field
> bags, no packed-header submission needed); (3) introducing this backend's first
> multi-codec **encoder** dispatch enum (`linux/vaapi/mod.rs` today exports only
> `VaapiVideoEncoder` directly — this ADR is what actually wires the dispatcher AV1's own ADR-0003
> only sketched, never wired, since AV1 stayed blocked). **This ADR is not blocked** — see
> § Why VP9 does not share AV1's packed-header blocker.

### Scope

**In (this ADR's design):**

- VP9 Profile 0 (8-bit 4:2:0, matches NV12 — this crate's only supported chroma/bit-depth
  convention across every codec so far), single tile column and row
  (`log2_tile_columns`/`log2_tile_rows`, computed via FFmpeg's own exact formula — see § VA-API-specific
  plumbing — always `0` for any resolution under `VP9_MAX_TILE_WIDTH = 4096` px wide, which covers
  every resolution this crate's existing `validate()` already accepts), no segmentation (VP9 VA-API
  encode has **no segmentation fields at all** in `EncPictureParameterBufferVP9` — confirmed absent
  from the real struct read this session, unlike decode's mandatory `seg_param` array — so "no
  segmentation" costs this ADR nothing to guarantee), no lossless mode, no compound prediction /
  B-frames (permanent non-goal, matching this crate's own H.264/AV1 GOP scope cuts).
- `KEY_FRAME`-only baseline: every pushed frame independent, `ref_flags.force_kf = 1`,
  `refresh_frame_flags = 0xff`, `pic_flags.frame_type = 0` (VP9's `frame_type` bit: `0` = key
  frame). Reproduces `VideoEncoderConfig::gop_size <= 1`'s existing cross-backend contract.
- Single-forward-reference `INTER_FRAME` GOP (`gop_size > 1`, capability-gated): `LAST_FRAME`-only
  prediction (`ref_flags.ref_frame_ctrl_l0 = 1`, `ref_last_idx` = the ping-pong slot **not** just
  written), 2-slot physical ping-pong (`hpic->slot = !href->slot`), `refresh_frame_flags = 1 <<
  slot | 0xfc` for a P frame — the exact FFmpeg branch quoted above, adopted verbatim-in-spirit
  (cross-checked, not byte-copied Rust, since the destination types differ).
- `error_resilient_mode = 1`, `frame_parallel_decoding_mode = 1`, `refresh_frame_context = 0`,
  `frame_context_idx = 0`, `reset_frame_context` set on every `KEY_FRAME` — this ADR's own
  deliberate choice (not directly shown in the FFmpeg snippet fetched this session, which does not
  set `pic_flags` beyond `frame_type`/`show_frame`), made for the same reason
  `vulkan::av1_params::build_inter_frame_picture_info` already gave for AV1's own
  `PRIMARY_REF_NONE`: an encoder can always choose the simplest legal encoding, and doing so here
  keeps this workspace's own eventual VP9 **decoder** (a real round-trip target, see this ADR's
  `mediaway-decoder` sibling) from ever needing to track adaptive frame-context state against this
  crate's own output, even though a real third-party VP9 stream might not make the same choice
  (the decoder sibling ADR still designs for that general case — see its own § Scope).
- `bit_offset_*`/`bit_size_segmentation` fields (seven total) always `0` — matches the only known
  real, shipping reference implementation's own convention (FFmpeg/i965), not independently
  derived; flagged in § Open questions as unconfirmed against any *other* real driver.
- `EncSequenceParameter` sent once per session (VP9 has no per-frame sequence header the way H.264
  resends SPS at IDR boundaries — `max_frame_width`/`max_frame_height`/`kf_auto = 0` (VP9 does not
  use libva's own automatic-keyframe-insertion feature; this crate's own `GopState` decides
  keyframe cadence itself, mirroring H.264/AV1's identical choice) are static for the whole
  session).

**Out (deferred):**

- Zero-Copy DMA-BUF surface import — unrelated axis, ADR-0001's own deferral, unchanged.
- Segmentation (moot for encode — the struct has no such fields at all), lossless mode, compound
  prediction, B-frames, multi-reference beyond `LAST_FRAME` (`GOLDEN_FRAME`/`ALTREF_FRAME` never
  populated), multi-tile, `Profile 1`/`2`/`3` (10-bit / non-4:2:0) — all permanent non-goals for
  this pass, matching this crate's own H.264/AV1 scope cuts.
- VBR/CBR rate control (`VideoEncoderConfig::rate_control` stays unread by this backend, same
  disposition every other codec in this crate/workspace gives it). CQP-only, fixed
  `luma_ac_qindex` per frame-type (mirrors FFmpeg's own `q_idx_idr`/`q_idx_p` split, cited in
  § VA-API-specific plumbing).
- `kf_auto`/libva-driven automatic keyframe insertion — this crate's `GopState` always drives
  keyframe cadence explicitly, matching H.264/AV1.

## Real caveat found this session: VP9 VA-API encode driver support is narrow, unlike decode

**This is the concrete, real gotcha this ADR was specifically asked to look for, distinct from
AV1's structural blocker.** FFmpeg's own source comment (quoted in full above) names **only i965**
("the one usable driver") as a working VP9 VA-API encode target. i965 is Intel's older
classic driver, largely superseded on modern Linux distributions by iHD (`intel-media-driver`) for
newer hardware/codecs — VP9 *encode* support across iHD/AMD Mesa/NVIDIA VA-API-via-NVENC-shim is
**not** confirmed by anything read this session, and general industry knowledge (not independently
sourced this session, flagged in § Open questions) is that VP9 hardware *encode* remains
meaningfully less universal across GPU vendors/drivers than VP9 hardware *decode* (VP9 decode is
near-ubiquitous — driven by broad web-video decode demand YouTube/etc. created; VP9 *encode* never
reached the same adoption breadth, notably absent or limited on several recent-generation
consumer GPU encode blocks compared to their H.264/HEVC/AV1 encode support). This ADR's own
`open_cpu` design therefore must **probe, never assume**, config/entrypoint availability before
claiming VP9 encode support (see § VA-API-specific plumbing) — the same "probe first" discipline
ADR-0002 already established for `VAConfigAttribEncMaxRefFrames`, applied here to the coarser
"does a VP9 encode config exist on this driver at all" question, which is a real possibility of
outright failure this ADR's H.264/AV1 siblings do not carry to the same degree.

### VA-API-specific plumbing

**Confirmed by reading `cros-libva` 0.0.13's real vendored source directly** (line numbers refer to
`cros-libva-0.0.13/src/buffer/vp9.rs`, read in full this session):

- `EncSequenceParameterBufferVP9::new(max_frame_width: u32, max_frame_height: u32, kf_auto: u32,
  kf_min_dist: u32, kf_max_dist: u32, bits_per_second: u32, intra_period: u32)`
  (`vp9.rs:246-266`) — sent once per session at `open_cpu` time (VP9 has no per-picture sequence
  header). `kf_auto = 0`, `bits_per_second = 0` (CQP, unread by this backend, matching every other
  codec's convention), `intra_period`/`kf_min_dist`/`kf_max_dist` set defensively from
  `effective_gop_size` even though `kf_auto = 0` means the driver should not act on them
  (mirrors this crate's own AV1 ADR-0003 defensive-but-inert field convention for driver fields
  whose real necessity under this crate's own explicit-per-picture-control scope is unconfirmed).
- `EncPictureParameterBufferVP9::new(frame_width_src, frame_height_src, frame_width_dst,
  frame_height_dst, reconstructed_frame: VASurfaceID, reference_frames: [VASurfaceID; 8],
  coded_buf: VABufferID, ref_flags: &VP9EncRefFlags, pic_flags: &VP9EncPicFlags,
  refresh_frame_flags: u8, luma_ac_qindex, luma_dc_qindex_delta, chroma_ac_qindex_delta,
  chroma_dc_qindex_delta, filter_level, sharpness_level, ref_lf_delta: [i8; 4],
  mode_lf_delta: [i8; 2], bit_offset_*, bit_size_segmentation, log2_tile_rows, log2_tile_columns,
  skip_frame_flag, number_skip_frames, skip_frames_size)` (`vp9.rs:371-402`, the full 26-parameter
  constructor) — `frame_width_dst`/`frame_height_dst` always equal `frame_width_src`/
  `frame_height_src` in this scope (no scaled-reference encode); `reference_frames: [VASurfaceID;
  8]` filled `VA_INVALID_ID` everywhere except index `ref_flags.ref_last_idx` for a P frame (a P
  frame only ever populates **one** of the eight array slots — matches FFmpeg's own loop, quoted
  above, which only writes `vpic->reference_frames[slot]` for slots this frame's `pic->refs[][]`
  actually names); `ref_lf_delta`/`mode_lf_delta` all-zero (loop-filter deltas disabled, matching
  this crate's H.264/AV1 all-disabled-optional-tool convention); `skip_frame_flag = 0`,
  `number_skip_frames = 0`, `skip_frames_size = 0` (VP9's own "encode as a run of skipped/duplicate
  frames" feature — unrelated to this ADR's scope, always disabled).
- `VP9EncRefFlags::new(force_kf, ref_frame_ctrl_l0, ref_frame_ctrl_l1, ref_last_idx,
  ref_last_sign_bias, ref_gf_idx, ref_gf_sign_bias, ref_arf_idx, ref_arf_sign_bias, temporal_id)`
  (`vp9.rs:277-302`) — `KEY_FRAME`: `force_kf = 1`, every other field `0`. `INTER_FRAME`:
  `ref_frame_ctrl_l0 = 1` (one L0 reference active), `ref_last_idx` = the ping-pong slot **not**
  being refreshed this frame, `ref_last_sign_bias = 1` (matches FFmpeg's unconditional choice,
  quoted above), `ref_frame_ctrl_l1`/`ref_gf_idx`/`ref_gf_sign_bias`/`ref_arf_idx`/
  `ref_arf_sign_bias`/`temporal_id` all `0` (no `GOLDEN_FRAME`/`ALTREF_FRAME`/temporal-layering use
  in this scope).
- `VP9EncPicFlags::new(frame_type, show_frame, error_resilient_mode, intra_only,
  allow_high_precision_mv, mcomp_filter_type, frame_parallel_decoding_mode, reset_frame_context,
  refresh_frame_context, frame_context_idx, segmentation_enabled, segmentation_temporal_update,
  segmentation_update_map, lossless_mode, comp_prediction_mode, auto_segmentation,
  super_frame_flag)` (`vp9.rs:317-355`) — `frame_type`: `0` for `KEY_FRAME`, `1` for `INTER_FRAME`
  (VP9's own bit convention — confirmed the *inverse* of "is this a key frame" naming, matching
  cros-codecs' `Header::frame_type: FrameType` semantics read this session); `show_frame = 1`
  always (no alt-ref-only invisible-frame case in this scope); `error_resilient_mode = 1`,
  `frame_parallel_decoding_mode = 1`, `refresh_frame_context = 0`, `frame_context_idx = 0` (this
  ADR's own § Scope choice, above); `reset_frame_context` set on `KEY_FRAME` only (VP9's 2-bit
  field: `0`/`1` = no reset, `2` = reset one context, `3` = reset all four — this ADR uses `3` on
  every `KEY_FRAME`, `0` otherwise, the simplest legal choice); `intra_only = 0`
  (`KEY_FRAME`/`INTER_FRAME` only, no intra-only-but-not-key frames this scope);
  `allow_high_precision_mv = 0`, `mcomp_filter_type = 0` (`EIGHTTAP`, VP9's spec-default motion
  compensation filter — arbitrary but legal choice, unconfirmed against any real driver
  preference); `segmentation_enabled = 0` and every segmentation-adjacent flag `0` (matches
  § Scope: "no segmentation fields exist" was about the *struct*, not this bitfield — this bit
  field genuinely exists here and must be explicitly `0`); `lossless_mode = 0`,
  `comp_prediction_mode = 0` (no compound prediction), `auto_segmentation = 0`,
  `super_frame_flag = 0` (VP9 "superframe" — multiple frames packed into one coded buffer for
  alt-ref use — out of scope, always `0`).
- `log2_tile_columns` — computed via FFmpeg's own exact formula, cited and reused verbatim (not
  independently re-derived): `num_tile_columns = (frame_width_src + VP9_MAX_TILE_WIDTH - 1) /
  VP9_MAX_TILE_WIDTH; log2_tile_columns = if num_tile_columns == 1 { 0 } else { ilog2(num_tile_columns
  - 1) + 1 }`, `VP9_MAX_TILE_WIDTH: u32 = 4096` (FFmpeg's own literal, `vaapi_encode_vp9.c`,
  fetched this session). Always `0` for every resolution this crate currently accepts (well under
  4096px wide). `log2_tile_rows` — **not set anywhere in the FFmpeg function body fetched this
  session** (only `log2_tile_columns` is computed there); this ADR sets it `0` defensively
  (single tile row, matching this crate's own single-tile-everything scope), flagged as unconfirmed
  against FFmpeg's *full* file (only the `init_picture_params`/`init_sequence_params`/`configure`
  functions were fetched and quoted this session, not the entire file) in § Open questions.
- Profile: `VAProfileVP9Profile0` — confirmed as the correct real libva enum name for 8-bit 4:2:0
  by FFmpeg's own profile table (`{ AV_PROFILE_VP9_0, 8, 3, 1, 1, VAProfileVP9Profile0 }`, fetched
  this session), but (same disposition as every prior ADR in this folder) its concrete bindgen
  numeric value is not independently confirmed against this workspace's real WSL2 build this
  session (§ Open questions).
- Entrypoint: **unconfirmed this session, flagged as this ADR's second-highest risk after driver
  availability itself.** This crate's H.264 path uses `VAEntrypointEncSlice`; VP9's only
  FFmpeg-confirmed working driver (i965) is architecturally the same "classic" driver generation as
  H.264's own `VAEntrypointEncSlice` convention, making `VAEntrypointEncSlice` the more likely
  correct choice for VP9 too (unlike AV1's ADR-0003, which reasoned the *opposite* — that AV1
  encode is dominated by the newer "low power" `VAEntrypointEncSliceLP` entrypoint on modern
  Intel/AMD silicon). This ADR designs `open_cpu` to probe `VAEntrypointEncSlice` first, falling
  back to `VAEntrypointEncSliceLP` only if the first probe finds no VP9 encode config at all — the
  same query-first-never-assume style this crate already established (`Display::
  query_config_entrypoints(profile)`), with the *opposite* try-order from the AV1 sibling, reasoned
  (not independently driver-confirmed) from the i965-only-driver finding above.

### `VaapiVp9Encoder` struct shape (ZCA sketch — ownership, no `Box`/`dyn`)

```rust
// linux/vaapi/vp9_gop.rs — new file, sans-io, no cros_libva types. Not a verbatim port (no
// precedent exists) — cross-checked against FFmpeg's vaapi_encode_vp9.c, quoted above.
pub(super) const VP9_MAX_TILE_WIDTH: u32 = 4096;
pub(super) const WORKSPACE_PING_PONG_SLOTS: usize = 2; // physical surfaces; VP9's own 8 *logical*
                                                        // ref-frame slots never need more than 2
                                                        // physical buffers for single-forward-ref

#[derive(Debug, Clone, Copy)]
pub(super) struct DpbSlot { pub(super) width: u32, pub(super) height: u32 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameRequest { Auto, ForceKey }

#[derive(Debug, Clone, Copy)]
pub(super) struct FrameDecision {
    pub(super) is_key: bool,
    pub(super) setup_slot: usize,          // 0 or 1 — which physical surface this frame writes
    pub(super) refresh_frame_flags: u8,    // 0xff for KEY_FRAME; `1 << setup_slot | 0xfc` for P
    pub(super) reference_slot: Option<usize>, // physical slot index of this P frame's LAST_FRAME
}

#[derive(Debug)]
pub(super) struct GopState {
    gop_size: u32,
    frames_since_key: u32,
    ping_pong: [Option<DpbSlot>; WORKSPACE_PING_PONG_SLOTS],
    last_written: usize,
}
impl GopState {
    pub(super) fn new(gop_size: u32) -> Self { .. }
    pub(super) fn decide(&mut self, request: FrameRequest) -> FrameDecision { .. }
}

// linux/vaapi/vp9.rs — new file, sibling of video.rs (H.264).
pub(crate) struct VaapiVp9Encoder {
    context: Rc<Context>,
    _config: Config,
    info: StreamInfo,
    width: u32,
    height: u32,
    entrypoint: cros_libva::VAEntrypoint::Type, // EncSlice probed first, EncSliceLP fallback
    nv12_bytes: usize,
    surfaces: [Option<Surface<()>>; WORKSPACE_PING_PONG_SLOTS], // fixed 2, not Vec — ping-pong only
    gop: GopState,
    effective_gop_size: u32,
    pending: VecDeque<Packet>,
    flushed: bool,
}

// linux/vaapi/mod.rs — NEW dispatch enum (this ADR is what actually wires it; AV1's ADR-0003 only
// sketched the idea, never wired since AV1 stayed blocked).
pub(crate) enum VaapiVideoEncoderSession {
    H264(VaapiVideoEncoder),
    Vp9(VaapiVp9Encoder),
}
```

No `Box<dyn _>`/`dyn Trait` anywhere. `[Option<Surface<()>>; 2]` (a fixed array, not `Vec`/
`SmallVec`) is a deliberate, tighter choice than H.264/AV1's `Vec`-backed pools — justified because
this ADR's own ping-pong design (cross-checked against FFmpeg's real `max_b_depth == 0` branch)
never needs more than 2 physical surfaces by construction, unlike H.264's SPS-configurable
`max_num_ref_frames` or AV1's 8-frame-wide ring. `codec.rs` grows a `CodecKind::Vp9` arm
(`video_profile` → `VAProfileVP9Profile0`, `is_supported_video_codec` includes `Vp9`), following
the existing `match` shape exactly.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Re-derive VP9 encode field values from the VP9 spec text independently, without FFmpeg cross-check | Rejected — `vaapi_encode_vp9.c` is a real, current, shipping reference implementation; fetching and quoting it directly (this session) closes real risk (`refresh_frame_flags`/`ref_flags` bit conventions, the `bit_offset_*` no-op convention, the tile-column formula) a spec-only re-derivation would have had to guess at or re-derive from first principles. |
| Treat the missing `EncSliceParameter::VP9` variant as a `cros-libva` gap needing a fork/PR, mirroring AV1's ADR-0003 | Rejected after confirming, via FFmpeg's own encoder source, that real libva VP9 encode has no slice-parameter buffer at all — the absence is correct, not a gap. Flagged explicitly in § Why VP9 does not share AV1's packed-header blocker so a future reader does not conflate the two ADRs' very different `cros-libva`-completeness findings. |
| Keep every P frame at logical slot `0` (FFmpeg's own simpler `max_b_depth == 0` branch), instead of the 2-slot ping-pong | Rejected — see § Why this ADR cannot be a verbatim port point (1): FFmpeg's own simpler branch relies on a same-picture read-before-write aliasing guarantee this crate's own `Surface`/`Picture` lifecycle (a `take()`-then-`Some()` pattern, unlike FFmpeg's own internal buffer management) cannot cheaply prove safe without its own new correctness argument. The ping-pong branch avoids ever aliasing a frame's own destination surface with its own reference surface, at the cost of one extra physical `Surface` — a cheap, clearly-correct trade this ADR takes deliberately. |
| Support `GOLDEN_FRAME`/`ALTREF_FRAME` (up to 3 active references) or B-frames this increment | Rejected — mirrors this crate's own H.264/AV1 permanent single-forward-reference scope cut; a self-contained `LAST_FRAME`-only increment is independently useful and independently verifiable, matching this workspace's staged-growth pattern. |
| `error_resilient_mode = 0` with real adaptive frame-context forwarding across frames | Rejected — an encoder can always choose the simplest legal encoding (same reasoning `vulkan::av1_params` already used for AV1's `PRIMARY_REF_NONE`); `error_resilient_mode = 1` costs this ADR nothing (still fully spec-legal, real GOP structure) and keeps this workspace's own eventual VP9 decoder's job simpler against this crate's own output, without narrowing what a *general* decoder (this crate's own decoder sibling ADR, or any third-party decoder) must still support against arbitrary real-world streams. |
| Try `VAEntrypointEncSliceLP` first, mirroring AV1's ADR-0003 probe order | Rejected — AV1's LP-first reasoning was specific to modern Intel/AMD AV1 encode blocks; VP9's only FFmpeg-confirmed working driver (i965) is architecturally the classic generation, making the non-LP entrypoint the more likely first match for VP9 specifically. Both orders cost one extra probe call either way; this ADR picks the order reasoned more likely to succeed on the first try for this specific codec, not a blanket policy. |

## Consequences

### Positive

- **Not blocked** — a real, complete, implementable design against `cros-libva` 0.0.13 exactly as
  pinned today, unlike this folder's AV1 sibling. This is the single most valuable finding of this
  ADR: VP9 encode can actually ship once an implementation pass picks this up.
- FFmpeg's own real, shipping encoder cross-check closes real risk on bit-convention details
  (`refresh_frame_flags`, `ref_flags` sign-bias/idx wiring, the `bit_offset_*` no-op convention,
  the tile-column formula) a spec-only design would have had to guess.
- Found and named a real, concrete, non-blocking caveat (narrow real-world VP9 encode driver
  support, i965-only per FFmpeg's own comment) before an implementation pass could discover it the
  hard way against real, possibly-unsupporting hardware.
- Introduces this backend's first multi-codec **encoder** dispatch enum, closing a real structural
  gap AV1's own ADR-0003 identified but could not itself resolve (since AV1 stayed blocked, nothing
  ever needed the dispatcher to exist for real).
- `gop_size <= 1` design mirrors this crate's own established default-path-byte-identical
  discipline for every existing/future caller.

### Negative / Trade-offs

- Zero real-hardware verification, compounded by a real, named driver-support caveat this pass
  cannot itself resolve (no real VP9-encode-capable driver available in this environment to probe
  against).
- The `bit_offset_*`/`bit_size_segmentation` all-zero convention is validated against exactly one
  real driver's own comment (i965, via FFmpeg) — whether every other real VP9 VA-API encode driver
  also tolerates all-zero values here is unconfirmed.
- `log2_tile_rows`'s real intended value is inferred (`0`), not confirmed against FFmpeg's full
  file (only three functions were fetched and quoted this session).
- The `VAEntrypointEncSlice`-first probe order is reasoned, not independently driver-confirmed —
  same residual risk class as every entrypoint-probing decision in this crate's sibling ADRs.
- The 2-slot ping-pong design costs one extra physical `Surface` versus FFmpeg's own simplest
  `max_b_depth == 0` branch — a deliberate, cheap trade (§ Alternatives), not a regression, but
  worth naming as a real, if small, resource-cost difference from the reference implementation.

## Test plan (for the implementation pass that follows this ADR)

- **Sans-io, hardware-independent (highest-value, run first)**: `linux/vaapi/vp9_gop_tests.rs` —
  `GopState::new(1)` reproduces all-`KEY_FRAME` forever; `GopState::new(3)` produces `K P P K P P
  K` cadence over 7 `decide()` calls; `setup_slot` strictly alternates `0`/`1` on every `P` frame
  cadence (never repeats consecutively); `refresh_frame_flags` is `0xff` on every `KEY_FRAME` and
  `(1 << setup_slot) | 0xfc` on every `P` frame; `reference_slot` is `None` on every `KEY_FRAME` and
  always the *other* ping-pong slot on every `P` frame.
- **`vp9.rs` integration** (hardware-gated, `_or_skip_without_hw`-style, expected to skip in this
  session/CI): `KEY_FRAME`-only push, then a `gop_size = 3` GOP push sequence; assert the VP9
  entrypoint probe correctly reports `Unsupported` (not a panic) on a driver exposing no VP9 encode
  config at all — the realistic failure mode given § Real caveat found this session.
- **Oracle validation**: pipe an encoded VP9 stream through system `ffprobe`/`ffmpeg -i` (this
  workspace's standing oracle, [ADR-0002](../../../../docs/adr/0002-system-oracle.md)) — validates
  the driver-synthesized bitstream against a real, independent VP9 parser.
- **`mediaway-decoder` round-trip** (once this ADR's decode-side sibling,
  [`mediaway-decoder/adr/linux/0004`](../../../mediaway-decoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md),
  is implemented): this crate's own `error_resilient_mode = 1`/`frame_parallel_decoding_mode = 1`/
  `refresh_frame_context = 0` choices should make this the single easiest real VP9 stream for this
  workspace's own decoder to accept, a real, if delayed, correctness cross-check neither AV1 sibling
  currently has (AV1 encode stays blocked; AV1 decode's own round-trip target is the D3D12 encoder,
  a different backend entirely).
- **WSL2 real-Linux compile verification**: confirms `VAProfileVP9Profile0`'s real name/value and
  `EncSequenceParameterBufferVP9`/`EncPictureParameterBufferVP9`/`VP9EncRefFlags`/`VP9EncPicFlags`
  field assumptions against the real bindgen output — every item in § Open questions.
- Default `cargo test --workspace` (no system FFmpeg, no VA-API hardware) must keep passing — the
  GOP-state sans-io tests above require neither.

## Addendum (2026-08-19, confirmed via real WSL2 bindgen output + FFmpeg's real generic probe order)

```
pub const VAProfileVP9Profile0: Type = 19;
pub const VAProfileVP9Profile1: Type = 20;
pub const VAProfileVP9Profile2: Type = 21;
pub const VAProfileVP9Profile3: Type = 22;
pub const VAEntrypointEncSlice: Type = 6;
pub const VAEntrypointEncPicture: Type = 7;
pub const VAEntrypointEncSliceLP: Type = 8;
```

`cros_libva::VAProfile::VAProfileVP9Profile0` is the correct reference path. All three entrypoint
constants confirmed present and exactly the values this ADR's own § Alternatives Considered table
already cited.

**Real correction to this ADR's own probe-order design**: § "This ADR designs `open_cpu` to probe
`VAEntrypointEncSlice` first, falling back to `VAEntrypointEncSliceLP`" is a 2-step ladder. Reading
FFmpeg's real generic `libavcodec/vaapi_encode.c` (`vaapi_encode_entrypoints_normal[]`, fetched and
read this session) shows FFmpeg's own real, battle-tested probe order for **every** VA-API-encoded
codec (not VP9-specific) is a **3-step** ladder: `VAEntrypointEncSlice` → `VAEntrypointEncPicture`
→ `VAEntrypointEncSliceLP` (low-power only tried when explicitly requested). `VAEntrypointEncPicture`
is confirmed (`vaapi_encode_vp9.c`, fetched this session) to be exactly what VP9 encode actually
uses in practice — that codec struct defines no `.slice_params_size`/`.init_slice_params` at all
(unlike H.264/HEVC, which define both), consistent with VP9 having no slice concept and
`cros-libva`'s own `EncSliceParameter` enum correspondingly having no `VP9` variant. **This ADR's
implementation pass should widen its probe ladder to three steps** (`EncSlice` →
`EncPicture` → `EncSliceLP`), not two — `VAEntrypointEncPicture` was missing from the original
design entirely, not just unconfirmed.

## Open questions / risks (explicit, for whoever picks up the implementation pass)

1. **Real-world VP9 VA-API encode driver availability beyond i965** — the single highest-priority
   open item; general industry knowledge (not independently sourced this session) suggests
   meaningfully narrower hardware/driver support than VP9 decode, but no specific current-generation
   driver capability matrix was fetched/confirmed this session.
2. **`VAProfileVP9Profile0`'s real bindgen name/value** — inferred from FFmpeg's own profile table,
   not confirmed against this workspace's real WSL2 bindgen output this session.
3. **`VAEntrypointEncSlice` vs `VAEntrypointEncSliceLP` real-world driver support for VP9
   specifically** — reasoned from the i965-classic-driver finding, not independently confirmed.
4. **`log2_tile_rows`'s real intended value** — inferred `0`, not confirmed against FFmpeg's full
   file (only `init_picture_params`/`init_sequence_params`/`configure` were fetched this session).
5. **Whether the `bit_offset_*`/`bit_size_segmentation` all-zero convention holds on drivers other
   than i965** — validated against exactly one real driver's own source comment.
6. **`mcomp_filter_type = 0` (`EIGHTTAP`)'s real driver-preference neutrality** — an arbitrary but
   spec-legal default choice, not cross-checked against FFmpeg's own value for this field (not
   fetched this session — FFmpeg's own `vpic->interpolation_filter` equivalent, if set, was outside
   the three functions quoted).

## References

- [ADR-0001](0001-vaapi-cros-libva-h264-cpu-upload.md) · [ADR-0002](0002-vaapi-h264-p-frame-gop.md)
  · [ADR-0005](0005-vaapi-av1-key-frame-and-inter-gop.md) — this crate's H.264/AV1
  baseline/GOP precedent; ADR-0003 is this ADR's direct structural template (same "baseline + GOP
  in one ADR" shape) and the source of the packed-header-blocker contrast this ADR explains VP9
  does **not** share
- `crates/mediaway-encoder/src/linux/vaapi/{codec,mod,video}.rs` — current H.264-only
  implementation this ADR adds a VP9 sibling to (`codec.rs:12-27`'s `match` shape; `mod.rs`'s
  current single-type export, which this ADR's dispatcher enum changes)
- `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cros-libva-0.0.13\src\buffer\vp9.rs`
  — real vendored `cros-libva` 0.0.13 source read directly for every `VP9*`/`Enc*BufferVP9`
  signature cited above (lines 1-445 read in full this session)
- `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cros-libva-0.0.13\src\buffer.rs`
  — `BufferType`/`EncSliceParameter` enum (lines 298-322, 436-443), confirming the missing
  `EncSliceParameter::VP9` variant is correct (no real libva VP9 encode slice buffer exists), not a
  gap
- FFmpeg `libavcodec/vaapi_encode_vp9.c` — fetched this session (`raw.githubusercontent.com`),
  quoted verbatim: `vaapi_encode_vp9_init_picture_params` (full function body, the IDR/P branch
  quoted in full above), `vaapi_encode_vp9_init_sequence_params`, `vaapi_encode_vp9_configure`,
  the "No packed headers are currently desired... the one usable driver (i965)..." comment, the
  `VP9_MAX_TILE_WIDTH`/`num_tile_columns` tile-column formula, the `AV_PROFILE_VP9_0`/
  `AV_PROFILE_VP9_2` → `VAProfileVP9Profile0`/`VAProfileVP9Profile2` profile table
- `docs.rs/cros-codecs` `cros_codecs::codec::vp9::parser::Header` — fetched this session, a real
  ChromeOS-shipping software VP9 parser's field list, cross-checked against `cros-libva`'s own VP9
  decode struct field names for the reference-model shape (`refresh_frame_flags: u8`,
  `ref_frame_idx: [u8; 3]`, `ref_frame_sign_bias: [u8; 4]`, `frame_context_idx`/
  `reset_frame_context`/`refresh_frame_context`/`frame_parallel_decoding_mode`) — this ADR's
  decode-side sibling cites the same source for its own bitstream-parser design
- `crates/mediaway-decoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md` — this ADR's
  same-session decode-side sibling; the two ADRs' § Scope choices are cross-checked against each
  other (this ADR's `error_resilient_mode = 1` choice named as a real round-trip-simplifying
  convenience for that sibling, not a requirement it imposes)
- `docs/roadmap.md` §2 — VP9 status entry this ADR updates (not actioned this pass — a wiki/roadmap
  update is separate follow-up work, see this response's own final summary)
- [`docs/spec/sans-io.md`](../../../../docs/spec/sans-io.md) ·
  [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md) ·
  [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) ·
  [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md) ·
  [`docs/adr/0002-system-oracle.md`](../../../../docs/adr/0002-system-oracle.md)

ADRs are **English**. Numbering is local to this `adr/` folder.
