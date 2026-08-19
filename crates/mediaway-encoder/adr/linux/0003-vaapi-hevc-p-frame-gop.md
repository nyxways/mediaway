# ADR-0003: VA-API HEVC single-forward-reference P-frame GOP encode (port from `vulkan/hevc_gop.rs`, fresh VA-API plumbing)

- **Status**: Proposed
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`src/linux/vaapi/`)

## Context

[ADR-0001](0001-vaapi-cros-libva-h264-cpu-upload.md) scoped `mediaway-encoder::linux::vaapi` to
H.264 Constrained Baseline only (`codec.rs::video_profile` returns `EncodeError::Unsupported`
for anything but `CodecKind::H264`, `codec.rs::is_supported_video_codec` matches only `H264`).
[ADR-0002](0002-vaapi-h264-p-frame-gop.md) (this same session) added real single-forward-reference
P-frame GOP encode for H.264 only — HEVC was explicitly out of scope there ("HEVC / AV1 encode —
VA-API supports them; this crate does not yet", ADR-0001's own Scope table, unchanged by
ADR-0002). This ADR adds HEVC encode to this backend, **directly at the P-frame-GOP capability
level** (not a separate "HEVC all-IDR Stage 1" pass first) — matching the task's own instruction
and the fact that this crate's H.264 GOP work (ADR-0002) already established every piece of
VA-API session/surface-pool/capability-gating machinery an HEVC encoder needs; restarting at an
IDR-only HEVC stage would just be an unnecessary intermediate checkpoint, not a real scope
reduction (every design question an IDR-only HEVC encoder would face is a strict subset of this
ADR's own).

### Why this ADR ports `mediaway-encoder::vulkan::hevc_gop`'s `GopState`, but *not* `hevc_params.rs`'s `StdVideoH265*` construction

`crates/mediaway-encoder/src/vulkan/hevc_gop.rs` is a **real, hardware-verified** (RTX 4090,
[`adr/vulkan/0002`](../vulkan/0002-vulkan-gop-rate-control.md)'s 2026-08-05 "second same-day
follow-up" implementation update: "`vulkan HEVC GOP VideoEncoder ok: 3 IDR + 4 P packets, cadence
matched gop_size=3`") implementation of the single-forward-reference P-frame GOP decision state
machine (IDR-vs-P cadence, `PicOrderCntVal` sequencing, one-slot-lookback DPB ring) this task
needs. Reading it in full (see § Precise porting plan) confirms it is **already GPU-API-agnostic**
— its own module doc states it reuses `h264_gop::WORKSPACE_DPB_CAP` "directly rather than
redeclared here — it is a genuinely codec-agnostic constant", and its `DpbSlot`/`Dpb`/`GopState`/
`FrameDecision` types hold zero `vk::*`/GPU-handle fields, mirroring
[`mediaway-encoder`'s own ADR-0002](0002-vaapi-h264-p-frame-gop.md) precedent for porting
`h264_gop.rs` verbatim into this same crate's `linux/vaapi/gop.rs` this same session. `hevc_gop.rs`
is **simpler** than `h264_gop.rs`: HEVC's `StdVideoEncodeH265ReferenceInfo` carries only
`PicOrderCntVal` (no `FrameNum` equivalent), and `StdVideoEncodeH265PictureInfo` has no
`idr_pic_id` field to sequence — so `hevc_gop::GopState`/`FrameDecision` are smaller structs than
their H.264 siblings (`is_idr`, `poc`, `setup_slot`, `reference` — no `frame_num`, no
`idr_pic_id`), a real, structural simplification this ADR's own port inherits directly.

**`hevc_params.rs`'s `StdVideoH265*` construction is a different matter — it is *not* a usable
porting source for this ADR**, because VA-API's own `cros-libva` HEVC encode buffer types
(confirmed by reading `cros-libva` 0.0.13's real vendored source directly, see § VA-API-specific
plumbing) have a **structurally smaller field set** than Vulkan Video's `StdVideoH265*` structs:
`EncSequenceParameterBufferHEVC` has **no** `log2_max_pic_order_cnt_lsb_minus4`,
`seq_parameter_set_id`, or `max_num_ref_frames` field at all (every one of
`bindings::VAEncSequenceParameterBufferHEVC`'s real fields is covered 1:1 by a constructor
parameter, confirmed by reading `cros-libva`'s `EncSequenceParameterBufferHEVC::new` body — there
is no hidden hardcoded default hiding a field this ADR would otherwise need to choose). This is a
genuine, cited, VA-API-vs-Vulkan-Video API-shape difference, not an oversight: VA-API's HEVC
encode entrypoint has the **driver itself** synthesize the real VPS/SPS/PPS NAL units into the
coded output buffer from a much coarser set of caller-supplied hints (profile/level/tier, CU/TU
size range, bitrate, GOP cadence integers) — unlike Vulkan Video encode, where **this crate**
builds the exact `StdVideoH265SequenceParameterSet` bits (including
`log2_max_pic_order_cnt_lsb_minus4`) that end up, byte-for-byte, in the encoded bitstream. This
ADR therefore designs **fresh** VA-API `EncSequenceParameterBufferHEVC`/
`EncPictureParameterBufferHEVC`/`EncSliceParameterBufferHEVC` construction (§ VA-API-specific
plumbing), citing `hevc_params.rs` only for the **numeric CU/TU size-range choice** that
transfers directly regardless of which codec API constructs the parameter set (see that section).

### Zero real-hardware verification remains the honest baseline

Re-confirmed: [ADR-0001](0001-vaapi-cros-libva-h264-cpu-upload.md) and
[ADR-0002](0002-vaapi-h264-p-frame-gop.md) make **no** hardware-verification claim for
`mediaway-encoder::linux::vaapi` — every VA-API call path is "written and compile-verified on
Linux (WSL2 Ubuntu 24.04 via `cargo check`/`cargo test`/`cargo clippy`, real `libva-dev` 1.20.0
headers/bindgen output)" but `Display::open()`/`vaInitialize` has never succeeded against real
hardware in this environment. This ADR ships the same way: **design only, no `.rs` files written
this pass.** The implementation pass that follows this ADR must run `cargo check -p
mediaway-encoder --target x86_64-unknown-linux-gnu` / `cargo clippy --all-targets -- -D warnings`
(WSL2 Ubuntu, real `libva-dev`) before claiming even compile correctness for the new
`VAProfileHEVCMain` enum variant name and the several HEVC-encode-specific field semantics this
ADR infers from FFmpeg's real source (see § Open questions) — flagged explicitly as unconfirmed,
not silently assumed, mirroring both H.264 VA-API sibling ADRs' identical disposition.

## Decision

> Extend `mediaway-encoder::linux::vaapi` to encode **HEVC Main profile, single-forward-reference
> P-frame GOP structures** (exactly one active L0 reference, no B-frames, no reference-list
> reordering, no long-term references, no tiles/WPP/SCC) by porting
> `mediaway-encoder::vulkan::hevc_gop::GopState` (+ `Dpb`/`DpbSlot`/`FrameDecision`/
> `FrameRequest`) verbatim into a new, crate-local, sans-io `linux/vaapi/hevc_gop.rs` (reusing
> `super::gop::WORKSPACE_DPB_CAP` directly, mirroring `hevc_gop.rs`'s own reuse of
> `h264_gop::WORKSPACE_DPB_CAP` — a genuinely codec-agnostic constant), then wiring its
> `FrameDecision` output into **freshly designed** (not ported — see § Context)
> `EncSequenceParameterBufferHEVC`/`EncPictureParameterBufferHEVC`/`EncSliceParameterBufferHEVC`
> fields, grounded in FFmpeg's real `libavcodec/vaapi_encode_h265.c` conventions (fetched and read
> this session, cited by exact field/value below) rather than re-derived from the ITU-T H.265
> spec text independently. A second, parallel `VaapiHevcVideoEncoder` type is added
> (`linux/vaapi/hevc.rs`); the two codec-specific encoder types are unified behind a new
> `VaapiVideoSession` enum (no `Box<dyn VideoEncoder>` — see § ZCA shape) so
> `LinuxVideoEncoder::open` can dispatch on `config.codec` without changing this crate's existing
> "no `dyn`, one concrete backend type behind `Option`" shape.

### Precise porting plan: which `hevc_gop.rs` items map to which new VA-API-side items

New file `crates/mediaway-encoder/src/linux/vaapi/hevc_gop.rs`, sans-io (no `cros_libva` types),
unit-testable without any VA-API device, mirroring `linux/vaapi/gop.rs`'s own shape:

| New (`linux/vaapi/hevc_gop.rs`) | Ported from (cited source) | Change from source |
|---|---|---|
| `use super::gop::WORKSPACE_DPB_CAP;` | `vulkan/hevc_gop.rs:23` (`use super::h264_gop::WORKSPACE_DPB_CAP;`) | Verbatim reuse pattern — this crate's own `linux/vaapi/gop.rs` already declares `pub(super) const WORKSPACE_DPB_CAP: usize = 4` (ADR-0002); the HEVC module reuses it the same way the Vulkan HEVC module reuses `h264_gop`'s constant, for the identical "genuinely codec-agnostic, not codec-specific" reasoning |
| `DpbSlot { poc: i32, is_idr: bool }` | `vulkan/hevc_gop.rs:42-45` | Verbatim — already zero GPU-handle fields |
| `Dpb { slots: [Option<DpbSlot>; WORKSPACE_DPB_CAP], next_slot: usize }` (+ `Default`) | `vulkan/hevc_gop.rs:49-61` | Verbatim |
| `FrameRequest { Auto, ForceIdr }` | `vulkan/hevc_gop.rs:72-75` | Verbatim — `ForceIdr` stays an unwired hook, same disposition as `linux/vaapi/gop.rs::FrameRequest` |
| `FrameDecision { is_idr, poc, setup_slot, reference: Option<(usize, DpbSlot)> }` | `vulkan/hevc_gop.rs:80-85` | Verbatim — no `frame_num`/`idr_pic_id` fields (HEVC has neither concept at this ADR's scope) |
| `GopState { gop_size, frames_since_idr, poc, dpb, last_written }` | `vulkan/hevc_gop.rs:92-98` | Verbatim |
| `GopState::new(gop_size)` | `vulkan/hevc_gop.rs:106-114` | Verbatim |
| `GopState::decide(request) -> FrameDecision` | `vulkan/hevc_gop.rs:116-157` | Verbatim — zero Vulkan-specific logic already; `poc` increments by exactly `1` per frame (not `2 * frame_num` like H.264's `pic_order_cnt_type = 2` choice) and resets to `0` at every IDR |

No new fields or methods — this crate's single-forward-reference design only ever needs
`decision.reference`'s one `(usize, DpbSlot)` pair, same reasoning as
[ADR-0002](0002-vaapi-h264-p-frame-gop.md)'s identical porting-table note for `h264_gop.rs`.

### VA-API-specific plumbing (fresh design, not ported — see § Context)

**Confirmed by reading `cros-libva` 0.0.13's real vendored source directly**
(`C:\Users\User\.cargo\registry\src\...\cros-libva-0.0.13\src\buffer\hevc.rs`, not
paraphrased):

- `PictureHEVC::new(picture_id: VASurfaceID, pic_order_cnt: i32, flags: u32)` (`hevc.rs:15-22`) —
  **structurally simpler than `PictureH264::new`**: no `frame_idx` parameter at all (HEVC has no
  `frame_num` concept), `pic_order_cnt` is a single signed value (not a top/bottom-field pair).
- `EncSequenceParameterBufferHEVC::new(general_profile_idc: u8, general_level_idc: u8,
  general_tier_flag: u8, intra_period: u32, intra_idr_period: u32, ip_period: u32,
  bits_per_second: u32, pic_width_in_luma_samples: u16, pic_height_in_luma_samples: u16,
  seq_fields: &HEVCEncSeqFields, log2_min_luma_coding_block_size_minus3: u8,
  log2_diff_max_min_luma_coding_block_size: u8, log2_min_transform_block_size_minus2: u8,
  log2_diff_max_min_transform_block_size: u8, max_transform_hierarchy_depth_inter: u8,
  max_transform_hierarchy_depth_intra: u8, pcm_sample_bit_depth_luma_minus1: u32,
  pcm_sample_bit_depth_chroma_minus1: u32, log2_min_pcm_luma_coding_block_size_minus3: u32,
  log2_max_pcm_luma_coding_block_size_minus3: u32, vui_fields: Option<HevcEncVuiFields>,
  aspect_ratio_idc: u8, sar_width: u32, sar_height: u32, vui_num_units_in_tick: u32,
  vui_time_scale: u32, min_spatial_segmentation_idc: u16, max_bytes_per_pic_denom: u8,
  max_bits_per_min_cu_denom: u8, scc_fields: &HevcEncSeqSccFields)` (`hevc.rs:793-826`) — **no**
  `log2_max_pic_order_cnt_lsb_minus4`/`seq_parameter_set_id`/`max_num_ref_frames` field (see §
  Context). `pic_width_in_luma_samples`/`pic_height_in_luma_samples` take raw pixel dimensions
  directly (`u16`, unlike H.264's macroblock-count `mb_width`/`mb_height`) — this ADR's
  `validate()` still requires 8-pixel (minimum-CB-size) alignment, mirroring
  `mediaway-encoder::vulkan::hevc_params::CtuAlignedExtent::from_pixels`'s identical 8-pixel gate,
  since VA-API itself does not enforce this for us.
  - `HEVCEncSeqFields::new(chroma_format_idc, separate_colour_plane_flag, bit_depth_luma_minus8,
    bit_depth_chroma_minus8, scaling_list_enabled_flag, strong_intra_smoothing_enabled_flag,
    amp_enabled_flag, sample_adaptive_offset_enabled_flag, pcm_enabled_flag,
    pcm_loop_filter_disabled_flag, sps_temporal_mvp_enabled_flag, low_delay_seq,
    hierachical_flag)` (`hevc.rs:687-703`, `[sic]` "hierachical" is the real, misspelled
    `cros-libva` field name) — `chroma_format_idc = 1`, `bit_depth_{luma,chroma}_minus8 = 0`
    (4:2:0 8-bit, matches every other backend in this workspace), `scaling_list_enabled_flag = 0`,
    `pcm_enabled_flag = 0` (no scaling lists / PCM, matching this crate's H.264 sibling's
    equally narrow scope), `amp_enabled_flag = 1`, `strong_intra_smoothing_enabled_flag = 1`
    (both real, spec-legal quality features with no scope cost to enabling — mirrors
    `mediaway-encoder::vulkan::hevc_params`'s SPS flag choices where applicable),
    `sample_adaptive_offset_enabled_flag = 0` (SAO adds a per-slice `slice_sao_luma_flag`/
    `_chroma_flag` decode-side parsing burden this ADR's own encode output should not impose on
    any decoder consuming it, including this workspace's own future VA-API HEVC *decoder* — see
    the sibling decode ADR's own finding that SAO is exactly the kind of flag a decoder must
    parse correctly or desync; disabling it here keeps this encoder's simplest possible output
    shape), `sps_temporal_mvp_enabled_flag = 0` (this crate never uses temporal MVP; disabling
    avoids a decoder needing to handle the `slice_temporal_mvp_enabled_flag` slice-header bit),
    `low_delay_seq = 1` (matches this design's own "no reordering, no B-frames" shape exactly —
    an honest, correct value, not just a default), `hierachical_flag = 0` (no hierarchical GOP —
    single-forward-reference, flat P-frame chain).
- `EncPictureParameterBufferHEVC::new(decoded_curr_pic: PictureHEVC, reference_frames:
  [PictureHEVC; 15], coded_buf, collocated_ref_pic_index: u8, last_picture: u8, pic_init_qp: u8,
  diff_cu_qp_delta_depth: u8, pps_cb_qp_offset: i8, pps_cr_qp_offset: i8, num_tile_columns_minus1:
  u8, num_tile_rows_minus1: u8, column_width_minus1: [u8; 19], row_height_minus1: [u8; 21],
  log2_parallel_merge_level_minus2: u8, ctu_max_bitsize_allowed: u8,
  num_ref_idx_l0_default_active_minus1: u8, num_ref_idx_l1_default_active_minus1: u8,
  slice_pic_parameter_set_id: u8, nal_unit_type: u8, pic_fields: &HEVCEncPicFields,
  hierarchical_level_plus1: u8, va_byte_reserved: u8, scc_fields: &HevcEncPicSccFields)`
  (`hevc.rs:954-978`) — **confirmed field semantics from FFmpeg's real
  `libavcodec/vaapi_encode_h265.c` `vaapi_encode_h265_init_picture_params`** (fetched and read
  this session):
  - `decoded_curr_pic = PictureHEVC::new(surface_id, decision.poc, 0)` — `flags: 0` (FFmpeg:
    `.flags = 0` for the current picture's own entry — only `ReferenceFrames[]` entries carry RPS
    flags).
  - `pic_fields.bits.idr_pic_flag = u32::from(decision.is_idr)`; `pic_fields.bits.coding_type =
    if decision.is_idr { 1 } else { 2 }` — FFmpeg's real numeric convention: `1` = Intra, `2` =
    Predictive, `3` = Bipredictive (never used here — no B-frames); `pic_fields.bits.
    reference_pic_flag = 1` unconditionally — every picture in this single-forward-reference
    design is a candidate reference for the next one (mirrors this crate's own H.264
    `reference_pic_flag` gating precedent from ADR-0002, simplified since HEVC encode has no
    "session running in GOP mode at all vs not" ambiguity the way H.264's SPS-level
    `intra_period`/`log2_max_frame_num_minus4` byte-identity requirement created — see §
    Consequences for why `gop_size <= 1` still stays simple here).
  - `nal_unit_type = if decision.is_idr { 19 } else { 1 }` — FFmpeg: `HEVC_NAL_IDR_W_RADL` (`19`,
    matches `HevcNalUnitType::Idr`'s own `19 | 20` acceptance range already established in this
    workspace's Vulkan decode module) for IDR, `HEVC_NAL_TRAIL_R` (`1`, a reference-picture
    trailing picture) for P — **not** `HEVC_NAL_TRAIL_N` (`0`, non-reference), since every P
    picture here is a reference (see `reference_pic_flag` above).
  - `reference_frames`: one real entry (`reference_frames[0] = PictureHEVC::new(ref_surface_id,
    ref_slot.poc, VA_PICTURE_HEVC_RPS_ST_CURR_BEFORE)` when `decision.reference.is_some()`) — the
    referenced picture is always temporally *before* the current one in this design (no
    reordering, no B-frames), so `RPS_ST_CURR_BEFORE` is always the correct flag (FFmpeg's own
    conditional `ref->display_order < pic->display_order ? RPS_ST_CURR_BEFORE : RPS_ST_CURR_AFTER`
    always resolves to the `BEFORE` arm here) — the other 14 entries stay
    `invalid_picture_hevc()`.
  - `hierarchical_level_plus1 = 0`, `collocated_ref_pic_index` /
    `num_ref_idx_l0_default_active_minus1 = 0` / `num_ref_idx_l1_default_active_minus1 = 0` /
    `num_tile_columns_minus1 = 0` / `num_tile_rows_minus1 = 0` / `column_width_minus1`/
    `row_height_minus1` all-zero (no tiles), `log2_parallel_merge_level_minus2 = 0`,
    `ctu_max_bitsize_allowed = 0xFF` (spec-legal "no limit" sentinel value, inferred from typical
    VA-API sample-code convention — **not independently confirmed against a real driver or
    FFmpeg source this session**, flagged in § Open questions), `slice_pic_parameter_set_id = 0`.
- `EncSliceParameterBufferHEVC::new(slice_segment_address: u32, num_ctu_in_slice: u32,
  slice_type: u8, slice_pic_parameter_set_id: u8, num_ref_idx_l0_active_minus1: u8,
  num_ref_idx_l1_active_minus1: u8, ref_pic_list0: [PictureHEVC; 15], ref_pic_list1: [PictureHEVC;
  15], ..., max_num_merge_cand: u8, slice_qp_delta: i8, ..., slice_fields: &HevcEncSliceFields,
  pred_weight_table_bit_offset: u32, pred_weight_table_bit_length: u32)` (`hevc.rs:1072-1100`) —
  `slice_type`: FFmpeg's real numeric convention **matches this workspace's own decode-side
  `HevcSliceType::from_raw`** (`0 = B, 1 = P, 2 = I`, ITU-T H.265 Table 7-7 directly — confirmed
  independently by two sources this session, FFmpeg's encoder and this crate's own decoder
  convention, not a coincidence): `if decision.is_idr { 2 } else { 1 }`. `ref_pic_list0[0] =
  reference_frames[0]` (the same `PictureHEVC` value built above, copied — **not** an index the
  way VA-API HEVC *decode*'s `RefPicList` field is, a genuine, cited, decode-vs-encode structural
  asymmetry — see the sibling decode ADR's own § VA-API-specific plumbing for the contrasting
  index-based shape), `ref_pic_list1` stays all-invalid (no B-frames). `max_num_merge_cand = 5`
  (spec maximum, matches `mediaway-encoder::vulkan::hevc_params::build_p_slice_segment_header`'s
  identical choice), `slice_qp_delta = 0` (this crate's `FIXED_QP` is carried via
  `EncPictureParameterBufferHEVC::pic_init_qp` directly, same convention as this crate's H.264
  sibling), `pred_weight_table_bit_offset`/`_bit_length = 0` (no weighted prediction).

### `VaapiVideoSession` dispatch — enum, not `Box<dyn VideoEncoder>` (ZCA shape)

```rust
// linux/vaapi/mod.rs — changed
mod codec;
mod gop;       // unchanged: H.264 GopState (ADR-0002)
mod hevc;      // NEW: VaapiHevcVideoEncoder
mod hevc_gop;  // NEW: HEVC GopState (this ADR's porting table)
mod video;     // unchanged: VaapiVideoEncoder (H.264)

pub(crate) enum VaapiVideoSession {
    H264(video::VaapiVideoEncoder),
    Hevc(hevc::VaapiHevcVideoEncoder),
}

impl VaapiVideoSession {
    pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        match config.codec {
            CodecKind::H264 => Ok(Self::H264(video::VaapiVideoEncoder::open(config)?)),
            CodecKind::Hevc => Ok(Self::Hevc(hevc::VaapiHevcVideoEncoder::open(config)?)),
            _ => Err(EncodeError::Unsupported),
        }
    }
}

// impl VideoEncoder for VaapiVideoSession: each method matches on `self` and delegates —
// two concrete, closed variants, no `dyn`/`Box` anywhere in this dispatch.
```

- `crates/mediaway-encoder/src/linux/mod.rs`'s `LinuxVideoEncoder.inner` field type changes from
  `Option<vaapi::VaapiVideoEncoder>` to `Option<vaapi::VaapiVideoSession>`; its `open()` body's
  `vaapi::VaapiVideoEncoder::open(config)` call becomes `vaapi::VaapiVideoSession::open(config)`.
  Every other line of `linux/mod.rs` (the `Option`-wrapping "closed after move" sentinel pattern
  ADR-0001 already established, the `#[cfg(not(target_os = "linux"))]` stub arm) is unchanged —
  this is a one-field-type, one-call-site edit, not a restructuring.
- `hevc.rs::VaapiHevcVideoEncoder` mirrors `video.rs::VaapiVideoEncoder`'s field shape closely
  (`context: Rc<Context>`, `_config: Config`, `info: StreamInfo`, `width`/`height: u32`,
  `nv12_bytes: usize`, `bits_per_second: u32`, `surfaces: Vec<Option<Surface<()>>>`,
  `gop: hevc_gop::GopState`, `pending: VecDeque<Packet>`, `flushed: bool`) — **no**
  `mb_width`/`mb_height` (HEVC's `EncSequenceParameterBufferHEVC` takes raw pixel dimensions, see
  above), **no** `effective_gop_size`/`supports_p_frames` fields: unlike H.264's SPS, which is
  sent only on IDR frames and must carry session-wide `intra_period`/`log2_max_frame_num_minus4`
  values that differ between "GOP mode active" and "GOP mode disabled" (forcing `video.rs` to
  track `effective_gop_size` across the whole session so a single `FrameDecision` alone cannot
  disambiguate), HEVC's `EncSequenceParameterBufferHEVC` has **no** GOP-cadence-sensitive field
  this ADR's design needs to vary between "mode active" and "disabled" beyond `intra_period`/
  `intra_idr_period`/`ip_period` themselves — computed fresh from `effective_gop_size` each time
  the SPS is actually sent (once per IDR), with no separate stored "is GOP mode active" bit
  needed since the SPS is cheap to recompute from `self.effective_gop_size: u32` alone (still a
  field, just not one that changes any *other* per-frame decision the way H.264's
  `reference_pic_flag` gating did — see the `reference_pic_flag = 1` unconditional choice above).
  Capability gate: reuses `super::video::probe_supports_p_frames`-equivalent logic against
  `VAProfileHEVCMain` (a codec-parameterized version of ADR-0002's existing
  `probe_supports_p_frames`, or a second call with a different profile argument — implementation
  detail, either is compile-cheap; this ADR does not mandate which).
- No `Box<dyn _>`/`dyn Trait` anywhere in this design — `VaapiVideoSession`'s two variants and
  `hevc_gop::GopState`/`Dpb`/`DpbSlot` are closed, concrete types, matching every other
  encode/decode backend in this workspace.

### Codec/profile mapping

`linux/vaapi/codec.rs::video_profile` gains one arm:

```rust
CodecKind::Hevc => Ok(cros_libva::VAProfile::VAProfileHEVCMain),
```

(`VAProfileHEVCMain = 17`, confirmed against the real `intel/libva` `va.h` header fetched this
session — the exact `cros_libva::VAProfile::Type` **enum variant name** existing with that value
in this crate's real WSL2-generated bindgen output is **not** independently confirmed this
session, flagged in § Open questions, mirroring ADR-0002's identical disposition for
`VAConfigAttribEncMaxRefFrames`). `is_supported_video_codec` gains `CodecKind::Hevc` to its
`matches!` arm. Entrypoint stays `VAEntrypointEncSlice` (`= 6`, confirmed against the real
`va.h`) — the same entrypoint this crate's H.264 path already uses; `VAEntrypointEncSliceLP`
(`= 8`, "low power" encode, some drivers' preferred HEVC entrypoint) is a real, deferred
alternative this ADR does not implement (see § Scope).

## Scope

**In (this ADR):**

- HEVC Main profile, `VAEntrypointEncSlice`, CQP rate control, CPU NV12 upload only — the exact
  same input/rate-control axes this crate's H.264 path already supports, applied to a second
  codec.
- `linux/vaapi/hevc_gop.rs`: verbatim port of `vulkan::hevc_gop::GopState` (+ `Dpb`/`DpbSlot`/
  `FrameDecision`/`FrameRequest`), reusing `super::gop::WORKSPACE_DPB_CAP`.
- `linux/vaapi/hevc.rs`: `VaapiHevcVideoEncoder`, structurally mirroring `video.rs`'s session
  shape and `push_frame` sequencing (steps 2-7 of ADR-0002's own numbered list apply unchanged,
  codec-agnostic at that level of description).
- `VideoEncoderConfig::gop_size` read the same way ADR-0002 already wired it for H.264: `1`
  (default) or an unsupporting driver falls back to all-IDR; `> 1` requests real P-frame GOP,
  capability-gated on `VAConfigAttribEncMaxRefFrames` queried against `VAProfileHEVCMain` (the
  same codec-agnostic attribute ADR-0002 already confirmed `= 13`).
- `VaapiVideoSession` enum dispatch in `linux/vaapi/mod.rs`, replacing the single concrete
  `VaapiVideoEncoder` export with a two-variant enum; `linux/mod.rs::LinuxVideoEncoder` updated
  to match.

**Out (deferred, tracked in `docs/roadmap.md`):**

- `VAEntrypointEncSliceLP` (low-power encode) — a real, driver-dependent alternative entrypoint
  some hardware prefers for HEVC; not implemented, not capability-probed, this pass.
- HEVC Main 10 (10-bit) / Range Extensions / Screen-Content-Coding profiles — 8-bit 4:2:0 Main
  only, matching this crate's H.264 sibling's identical bit-depth scope.
- Tiles, WPP (`entropy_coding_sync_enabled_flag`), multiple slices per picture.
- B-frames, multi-reference, reference-list reordering, long-term references — permanent
  non-goals, matching `hevc_gop.rs`'s own scope framing and this crate's H.264 sibling ADR-0002's
  identical narrowing.
- VBR/CBR rate control (`VideoEncoderConfig::rate_control` stays unread by this backend for both
  codecs — same disposition Vulkan gives HEVC/AV1 today).
- Zero-Copy DMA-BUF surface import — unrelated axis, ADR-0001's own deferral, untouched.
- AV1 VA-API encode — a separate, not-yet-designed follow-up; this ADR does not extend
  `VaapiVideoSession` beyond H.264/HEVC.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Re-derive HEVC's GOP/POC state machine from the ITU-T H.265 spec text independently | Rejected — `vulkan/hevc_gop.rs` is real, hardware-verified, already-debugged, and already GPU-API-agnostic; re-deriving independently re-risks the same bug classes for no benefit, mirroring ADR-0002's identical reasoning for `h264_gop.rs`. |
| Port `vulkan/hevc_params.rs`'s `StdVideoH265*` construction as if it mapped onto `cros-libva`'s HEVC encode buffers | Rejected — a real, cited, structural mismatch (see § Context): VA-API's `EncSequenceParameterBufferHEVC` has no `log2_max_pic_order_cnt_lsb_minus4`/`seq_parameter_set_id`/`max_num_ref_frames` field at all, because the *driver* synthesizes the real VPS/SPS/PPS bitstream for HEVC encode, unlike Vulkan Video encode where this crate builds those bits itself. Attempting a 1:1 struct-field port would either invent fields that do not exist on the VA-API side or silently drop real Vulkan-side correctness lessons that do not apply here — fresh, cited design against the real `cros-libva`/FFmpeg source is the honest path. |
| Import `crate::vulkan::hevc_gop::GopState` directly (same crate) | Rejected for the same reason ADR-0002 rejected the equivalent H.264 import: creates compile-time coupling between two conceptually independent, separately-shippable platform backends; porting (not importing) matches this crate's own established precedent, now applied twice in the same session. |
| Add a third top-level type (`VaapiHevcVideoEncoder`) directly to `LinuxVideoEncoder` as a second `Option` field, or a separate `LinuxHevcVideoEncoder` public type | Rejected — two parallel `Option` fields (only one ever `Some`) is a real footgun (an invalid "both `Some`" or "both `None`-after-open" state becomes representable), and a second public encoder type duplicates `LinuxVideoEncoder`'s entire `VideoEncoder` impl boilerplate for no benefit; a closed `VaapiVideoSession` enum inside the existing `vaapi` module, with `LinuxVideoEncoder` unchanged in shape (still one `Option<T>` field, just a different `T`), is the smaller, ZCA-clean diff. |
| Support `VAEntrypointEncSliceLP` with a fallback ladder this increment | Rejected — a second entrypoint axis (capability-probe both `EncSlice` and `EncSliceLP`, choose one) adds real design surface (do rate-control/GOP semantics differ between them? Unconfirmed) this ADR does not need to resolve to deliver a real, useful HEVC GOP encode capability; deferred as a named follow-up, not silently dropped. |
| Enable `sample_adaptive_offset_enabled_flag` / `sps_temporal_mvp_enabled_flag` in the encoded SPS (real quality/compression wins real encoders commonly take) | Rejected this increment — both add real per-slice decode-side parsing burden (`slice_sao_luma_flag`/`_chroma_flag`, `slice_temporal_mvp_enabled_flag`) that this workspace's own sibling VA-API HEVC *decoder* (design in the same session, `mediaway-decoder` ADR-0003) would then be forced to handle correctly to round-trip this encoder's own output — disabling both keeps this encoder's default output the simplest possible shape for any decoder (including this workspace's own) to consume correctly, at a real but modest compression-efficiency cost. A future ADR could revisit this once cross-decoder correctness for SAO-enabled streams is independently verified. |

## Consequences

### Positive

- HEVC joins H.264 as a real, GOP-capable VA-API encode codec in this crate, reusing every piece
  of session/surface-pool/capability-gating machinery ADR-0001/ADR-0002 already built.
- `GopState`/`FrameDecision` logic is a **cited port** of already-hardware-verified code (Vulkan
  HEVC GOP encode, RTX 4090) — meaningfully lower bug risk than fresh spec-derivation, and
  *simpler* than the H.264 port (no `frame_num`/`idr_pic_id` bookkeeping at all).
- `VaapiVideoSession`'s enum dispatch adds a second codec with zero `Box<dyn>`/`dyn Trait`
  introduced anywhere in this crate's encode path, and a minimal, mechanical change to
  `linux/mod.rs` (one field type, one call site).
- Found and documented a real, structural VA-API-vs-Vulkan-Video API-shape asymmetry
  (`EncSequenceParameterBufferHEVC`'s missing POC/ref-count fields) that would have produced a
  broken porting attempt if assumed away.
- Deliberately disables SAO/temporal-MVP in this encoder's own SPS to keep its output the
  simplest possible shape for the sibling decode ADR's own VA-API HEVC decoder to round-trip
  correctly — a cross-crate-aware design choice made explicit, not accidental.

### Negative / Trade-offs

- **Zero real-hardware verification**, same caveat class as every VA-API backend in this
  workspace so far — now with HEVC-specific field semantics (`coding_type`, `nal_unit_type`,
  RPS flags) inferred from FFmpeg's real source, not independently confirmed against a real
  driver.
- `ctu_max_bitsize_allowed`'s exact "no limit" sentinel value is inferred from general VA-API
  sample-code convention, not confirmed against FFmpeg or a real driver this session (§ Open
  questions).
- Disabling SAO/temporal-MVP is a real compression-efficiency cost versus what a fully-tuned
  HEVC encoder would emit — an honest trade for cross-decoder simplicity, not a free win.
- `VAEntrypointEncSliceLP` fallback remains undesigned; on drivers that only expose the
  low-power entrypoint for HEVC (real on some hardware), this backend's HEVC path degrades to
  `EncodeError::Backend` at `open()` rather than falling back — same class of gap ADR-0001 already
  accepts for its single fixed H.264 profile choice.

## Test plan (for the implementation pass that follows this ADR)

- **Sans-io, hardware-independent (highest-value, run first)**: `linux/vaapi/hevc_gop_tests.rs` —
  mirrors `linux/vaapi/gop_tests.rs`'s H.264 coverage shape: `GopState::new(1)` reproduces
  all-IDR forever (`poc` stays `0`, `reference: None`); `GopState::new(3)` produces `I P P I P P
  I` `is_idr` cadence over 7 `decide()` calls; `poc` increments by exactly `1` per call and
  resets to `0` at every IDR; `decision.reference` is `None` on every IDR and `Some` on every P
  frame, always pointing at the immediately preceding `decide()` call's `setup_slot`.
- **`hevc.rs` integration** (hardware-gated, `_or_skip_without_hw`-style, expected to skip in
  this session/CI without real `/dev/dri/renderD*`): a `gop_size = 3`, 7-frame push sequence;
  scan the resulting HEVC Annex-B packets' NAL types (`HEVC_NAL_IDR_W_RADL = 19` /
  `HEVC_NAL_TRAIL_R = 1`, mirroring this ADR's own `nal_unit_type` choice) for the expected
  `I P P I P P I` keyframe cadence via `Packet::is_keyframe`.
- **Codec-profile smoke**: `codec.rs::video_profile(CodecKind::Hevc)` returns
  `Ok(VAProfile::VAProfileHEVCMain)`; `is_supported_video_codec(CodecKind::Hevc)` is `true`.
- **Oracle validation**: pipe a `gop_size > 1` encoded HEVC Annex-B stream through system
  `ffprobe`/`ffmpeg -i` ([ADR-0002 workspace-wide](../../../../docs/adr/0002-system-oracle.md))
  to confirm the stream is structurally decodable by a real, independent decoder — this is this
  ADR's primary correctness bar, since this workspace's own sibling VA-API HEVC decoder (designed
  the same session) is a **new, unverified, sans-io-only-tested** module, not yet a trustworthy
  oracle for this encoder's output the way it eventually should become.
- **WSL2 real-Linux compile verification** (available this workspace, per
  `docs/ai/wiki/platform/linux-encode.md`): `cargo check`/`cargo test --lib`/`cargo clippy
  --all-targets -- -D warnings` for `mediaway-encoder` on a real Linux target via WSL2 Ubuntu with
  real `libva-dev` — confirms `VAProfile::VAProfileHEVCMain`'s real enum variant name and every
  `EncSequenceParameterBufferHEVC`/`EncPictureParameterBufferHEVC`/`EncSliceParameterBufferHEVC`
  field assumption this ADR cites from `cros-libva`'s crates.io source against the real bindgen
  output. Must be run before this ADR's implementation pass is considered even compile-verified.
- Default `cargo test --workspace` (no system FFmpeg, no VA-API hardware) must keep passing —
  every new sans-io test above requires neither.

## Open questions / risks (explicit, for whoever picks up the implementation pass)

1. **`cros_libva::VAProfile::VAProfileHEVCMain`'s real enum variant existing with that exact
   name** in this crate's real WSL2-generated bindgen output — the *value* (`17`) and the
   *header-level name* are confirmed against the real `intel/libva` `va.h` (fetched this
   session), but the generated Rust binding's exact identifier was not independently confirmed
   against this session's own `bindings.rs` (unlike `VAConfigAttribEncMaxRefFrames`, which
   ADR-0002 already confirmed byte-for-byte from a real WSL2 build this same session).
2. **`ctu_max_bitsize_allowed`'s "no limit" sentinel value** (`0xFF`, inferred from general
   VA-API sample-code convention) — not confirmed against FFmpeg's real
   `vaapi_encode_h265_init_picture_params` source (that function was fetched and read this
   session for every *other* field cited above, but this specific field's exact value was not
   captured in the fetch's output).
3. **Whether `VAEntrypointEncSlice` (vs. `VAEntrypointEncSliceLP`) is universally available for
   `VAProfileHEVCMain`** on real drivers this workspace targets (Intel iHD / Mesa / AMD) — some
   real hardware/driver combinations only expose the low-power entrypoint for HEVC encode;
   unconfirmed against a real driver this session, flagged as a real risk to this ADR's "opens
   successfully" bar on some hardware, not just a missing nice-to-have.
4. **Whether the driver-synthesized HEVC SPS/PPS this encoder never directly inspects actually
   honors every `EncSequenceParameterBufferHEVC` field this ADR sets** (in particular
   `intra_period`/`intra_idr_period`/`ip_period` for GOP cadence, and the CU/TU size-range fields
   carried over from `hevc_params.rs`'s already-hardware-verified-on-Vulkan choice) — VA-API
   HEVC encode gives the driver more synthesis latitude than H.264's older, more caller-dictated
   API; unconfirmed against a real driver.
5. **Whether disabling SAO (§ Alternatives Considered) measurably affects this ADR's own oracle
   test's compression-ratio expectations** (if any are added) — flagged as a design trade-off,
   not independently measured this session.

## Addendum (2026-08-19, confirmed via real WSL2 bindgen output)

Open question #1 is now closed. `cros_libva::VAProfile::VAProfileHEVCMain` (module path, not an
enum variant — `cros-libva` re-exports raw bindgen's `pub mod VAProfile { pub const
VAProfileHEVCMain: Type = 17; ... }` at crate root) is confirmed to exist in this crate's real
WSL2 bindgen output, with the same value (`17`) already cited from `va.h`, and confirmed to be
the exact same reference shape this crate's own existing H.264 code already uses
(`cros_libva::VAProfile::VAProfileH264ConstrainedBaseline`, `linux/vaapi/codec.rs:18`) — no
guesswork needed, this ADR's `codec.rs` addition is a direct pattern match.

Open question #2 (`ctu_max_bitsize_allowed`'s sentinel value) is partially closed: the field
itself is confirmed present as a real `u8` in the bindgen output (same build), but its "no limit"
sentinel numeric convention (`0xFF`, this ADR's own inference) remains unconfirmed against a real
driver or FFmpeg source — still open for the implementation pass to verify empirically if a real
VA-API HEVC driver is available.

Open question #3 (`VAEntrypointEncSlice` vs `VAEntrypointEncSliceLP` universal availability)
remains open — bindgen confirms both entrypoint constants exist (`= 6`/`= 8`), but which one a
given real driver actually requires for HEVC is not resolvable from bindgen output alone.

## References

- [ADR-0001](0001-vaapi-cros-libva-h264-cpu-upload.md) — this crate's binding choice, H.264-only
  original scope
- [ADR-0002](0002-vaapi-h264-p-frame-gop.md) — this crate's H.264 P-frame GOP sibling, same
  session, the porting-methodology and `VaapiVideoSession`-predecessor precedent this ADR mirrors
  and extends
- `crates/mediaway-encoder/src/vulkan/hevc_gop.rs` — `GopState`/`Dpb`/`DpbSlot`/`FrameDecision`/
  `FrameRequest` porting source (hardware-verified RTX 4090, see `adr/vulkan/0002`'s 2026-08-05
  "second same-day follow-up" implementation update)
- `crates/mediaway-encoder/src/vulkan/hevc_params.rs` — cited **only** for the CU/TU coding-block/
  transform-block size-range numeric choice (`CB_MIN_LOG2_MINUS3`/`CB_DIFF_LOG2`/
  `TB_MIN_LOG2_MINUS2`/`TB_DIFF_LOG2`/`TRANSFORM_HIERARCHY_DEPTH`, lines 74-78) — **not** a
  porting source for VA-API's own parameter-set construction (see § Context for why)
- [`crates/mediaway-encoder/adr/vulkan/0002-vulkan-gop-rate-control.md`](../vulkan/0002-vulkan-gop-rate-control.md)
  — porting source's own ADR; HEVC hardware-verification history
- `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cros-libva-0.0.13\src\buffer\hevc.rs`
  — real vendored `cros-libva` 0.0.13 source read directly for every `PictureHEVC`/
  `EncSequenceParameterBufferHEVC`/`EncPictureParameterBufferHEVC`/`EncSliceParameterBufferHEVC`
  signature cited above
- FFmpeg `libavcodec/vaapi_encode_h265.c` (`vaapi_encode_h265_init_picture_params`,
  `vaapi_encode_h265_init_slice_params`) — fetched and read this session; real, sourced oracle for
  `coding_type`/`idr_pic_flag`/`reference_pic_flag`/`nal_unit_type`/RPS-flag/`slice_type`
  conventions cited above
- [Intel `libva` `va.h`](https://github.com/intel/libva/blob/master/va/va.h) — fetched this
  session; `VAProfileHEVCMain = 17`, `VAEntrypointEncSlice = 6`, `VAEntrypointEncSliceLP = 8`,
  `VAConfigAttribEncMaxRefFrames = 13` confirmed directly
- [`crates/mediaway-decoder/adr/linux/0003-vaapi-hevc-p-slice-dpb.md`](../../../mediaway-decoder/adr/linux/0003-vaapi-hevc-p-slice-dpb.md)
  — same-session decode-side sibling; the SAO/temporal-MVP-disabled encoder output shape this ADR
  chooses is deliberately the simplest input that sibling's decode-side slice-header parser needs
  to handle
- [`docs/ai/wiki/platform/linux-encode.md`](../../../../docs/ai/wiki/platform/linux-encode.md) ·
  [`docs/ai/wiki/encode/vulkan-h264-gop.md`](../../../../docs/ai/wiki/encode/vulkan-h264-gop.md)
- [`docs/spec/sans-io.md`](../../../../docs/spec/sans-io.md) ·
  [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md) ·
  [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) ·
  [`docs/adr/0002-system-oracle.md`](../../../../docs/adr/0002-system-oracle.md)

ADRs are **English**. Numbering is local to this `adr/` folder.
