# ADR-0003: VA-API HEVC single-forward-reference P-slice decode (fresh RPS-based DPB; slice-header parser extended well beyond the Vulkan porting source)

- **Status**: Proposed
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (`src/linux/vaapi/`)

## Context

[ADR-0001](0001-vaapi-h264-cpu-out.md) scoped `mediaway-decoder::linux::vaapi` to H.264 only
(`codec.rs::is_supported_video_codec` matches only `CodecKind::H264`). [ADR-0002](0002-vaapi-h264-p-slice-dpb.md)
(this same session) added single-forward-reference P-slice decode for H.264 only, by porting
`mediaway-decoder::vulkan::dpb`'s **hardware-verified** (RTX 4090) sliding-window DPB. This ADR
adds HEVC decode to this backend — but, unlike ADR-0002's H.264 port, **this ADR cannot port an
equivalent hardware-verified P-slice HEVC decode implementation, because none exists anywhere in
this workspace.**

### Why this ADR is *not* a straightforward port, unlike its H.264 sibling

Reading `crates/mediaway-decoder/src/vulkan/decoder_hevc.rs` directly (this crate's own Vulkan
HEVC decode module) confirms its own module doc: *"Only IDR pictures reach a real
`vkCmdDecodeVideoKHR` call... a P/B-slice HEVC NAL is rejected with `DecodeError::Unsupported`...
General-GOP HEVC hardware verification is an explicit follow-up."* Cross-checked against
[`adr/vulkan/0001`](../vulkan/0001-vulkan-video-decode.md)'s own 2026-07-30/2026-08-05 addenda: HEVC
**IDR** decode is real and hardware-verified (RTX 4090, confirmed 2026-08-05 after a real bug fix),
but *"P/B-slice HEVC hardware verification remains an explicit, unchanged follow-up
(`decoder_hevc.rs::decode_slice_hevc` still only reaches a real... call for IDR pictures)."* This
is a materially different starting point than ADR-0002's H.264 port, where the *entire*
single-forward-reference P-slice decode path (DPB, reference-list construction, POC arithmetic)
was already real and hardware-verified on Vulkan before this session began.

What Vulkan's HEVC module *does* offer, real and useful, cited precisely below:

- `crates/mediaway-decoder/src/vulkan/hevc_slice.rs`'s `ShortTermRefPicSet::parse` and
  `HevcSliceSegmentHeader::parse` — **real, sans-io, unit-tested** bitstream parsing (not
  hardware-dependent to be correct), reusable **as a starting point**, not a finished porting
  source (see § Slice-header parser extension for why this crate's own parser must read
  meaningfully further than Vulkan's does).
- `crates/mediaway-decoder/src/vulkan/hevc_params.rs`'s `HevcSps`/`HevcPps`/`HevcNalUnit`/
  `HevcVps` parsing — real, sans-io, hardware-*informed* (several fields were added specifically
  because a real hardware bug proved they must be echoed exactly, see that file's own field-level
  doc comments) — again a starting point this ADR extends, not a finished porting source, since
  VA-API's own parameter-buffer field set differs from Vulkan's `StdVideoH265*` shape (mirrors
  ADR-0002's own "reuse `BitReader`, write a fresh local `Sps`/`Pps`" precedent for H.264).
- **No DPB/reference-list/POC-derivation HEVC code exists anywhere in this workspace to port at
  all** — Vulkan's own HEVC decode never reached that far (IDR pictures have no references, so
  `decode_slice_hevc` never needed a DPB). This ADR designs that machinery **fresh**, grounded in
  ITU-T H.265 §8.3 (fetched/confirmed this session, see below) and in this crate's own **already-
  ported, sibling** `linux/vaapi/dpb.rs` (H.264, ADR-0002, same crate, same session) for the one
  piece of arithmetic that transfers directly: `derive_pic_order_cnt_msb` (§8.2.1.1 for H.264,
  §8.3.1 for HEVC — the same MSB/LSB-wraparound formula, confirmed by reading the real ITU-T
  H.265 §8.3.1 derivation via FFmpeg's `libavcodec/hevc/hevcdec.c` cross-reference this session).

### Why this ADR does *not* need H.264's general sliding-window `Dpb`

HEVC's reference model is a genuine, spec-level departure from H.264's `frame_num`/sliding-window
scheme (already noted in `vulkan/hevc_slice.rs`'s own module doc: *"HEVC's reference model
genuinely differs from H.264's `RefPicList0`/`1`... instead of a sliding-window `frame_num`, each
picture signals... a short-term RPS describing exactly which prior pictures are 'current'
references"*). Combined with this ADR's own single-forward-reference scope (mirroring
`mediaway-encoder`'s sibling ADR-0003 and this crate's own H.264 ADR-0002), the *general* HEVC RPS
machinery (multiple simultaneous references, `RefPicSetStCurrBefore`/`After`/`LtCurr` each holding
several entries, sliding eviction across an arbitrary DPB) collapses to something structurally
closer to **`mediaway-encoder::vulkan::hevc_gop::GopState`'s own single-slot `last_written`
design** than to H.264 decode's general `Dpb`/`sliding_window_evict_target`. This ADR designs a
deliberately small `HevcDpb` (§ DPB design) — not a port of `linux/vaapi/dpb.rs`'s H.264 `Dpb`
(that type's sliding-window eviction machinery has no reachable code path once occupancy is
capped at exactly one non-current picture), and not a port of anything Vulkan HEVC ships either
(since none exists) — cited as a **fresh, narrowly-scoped design**, not a porting claim.

### Zero real-hardware verification remains the honest baseline

Re-confirmed: [ADR-0001](0001-vaapi-h264-cpu-out.md)/[ADR-0002](0002-vaapi-h264-p-slice-dpb.md)
make **no** hardware-verification claim for `mediaway-decoder::linux::vaapi`. This ADR ships the
same way: **design only, no `.rs` files written this pass.** Given this ADR's own § "not a
straightforward port" finding, the implementation pass that follows carries **strictly more**
open, unconfirmed risk than ADR-0002's H.264 port did — every `VA_PICTURE_HEVC_*` flag's exact
numeric value, and the entire fresh slice-header-syntax extension this ADR designs (§ Slice-header
parser extension), must be confirmed against real WSL2 `cargo check`-generated bindgen output
*and* exercised against real encoder bitstreams (this workspace's own HEVC encoder output is the
nearest available oracle — see § Test plan) before this ADR's implementation can be trusted even
at the sans-io-parsing level, let alone against real VA-API hardware.

## Decision

> Extend `mediaway-decoder::linux::vaapi` to decode **HEVC Main profile, single-forward-reference
> P-slices** (exactly one active L0 reference, always the immediately preceding picture, RPS
> shape `num_negative_pics == 1, num_positive_pics == 0, delta_poc == -1,
> used_by_curr_pic_s0_flag == 1` — any other RPS shape rejected as `Unsupported`) plus IDR
> I-slices, by (a) writing a crate-local `linux/vaapi/hevc_sps.rs`/`hevc_pps.rs`/`hevc_nal.rs`
> that **starts from** `vulkan::hevc_params`'s parsing logic (cited, not imported — same
> reasoning as this crate's H.264 modules) and **extends it** with the additional PPS/slice-header
> syntax elements VA-API's parameter buffers require and Vulkan's decode path never needed (§
> Slice-header parser extension); (b) a fresh, narrow `linux/vaapi/hevc_dpb.rs` (§ DPB design,
> not a port); and (c) fresh `PictureParameterBufferHEVC`/`SliceParameterBufferHEVC` construction
> grounded in FFmpeg's real `libavcodec/vaapi_hevc.c` conventions (fetched and read this session).
> **No B-slices, no CRA/random-access pictures (IDR only for intra), no tiles/WPP, no scaling
> lists, no weighted prediction, no multi-reference, no reference-list reordering, no long-term
> references.**

### What *is* reusable from `vulkan::hevc_params`/`hevc_slice`, cited precisely

| New (`linux/vaapi/hevc_*.rs`) | Starting point (cited source) | Disposition |
|---|---|---|
| 2-byte NAL header parse + emulation-prevention removal | `vulkan/hevc_params.rs::HevcNalUnit::parse` (`hevc_params.rs:141-177`) + `remove_emulation_prevention` (`hevc_params.rs:183-195`) | **Reusable as-is in shape** (codec-agnostic bit layout, no VA-API-specific field needed) — this crate writes its own local copy (`linux/vaapi/hevc_nal.rs`), same non-cross-module-import reasoning as every other port in this session (see § Alternatives) |
| `profile_tier_level()` parse | `vulkan/hevc_params/hevc_ptl.rs::parse_profile_tier_level` | **Reusable as-is** — this crate's own `Sps` needs `general_profile_idc`/`general_level_idc`/`general_tier_flag` for `cros-libva`'s `PictureParameterBufferHEVC`... **correction, decode's `PictureParameterBufferHEVC` has no profile/level/tier field at all** (only encode's `EncSequenceParameterBufferHEVC` does) — VA-API *decode* negotiates profile via `Display::query_config_profiles`/`create_config` (mirrors this crate's H.264 `h264_profile_candidates`), not via a parameter-buffer field. This crate's own `Sps::parse` still needs `general_profile_idc` to pick a `VAProfile` candidate list (§ Codec/profile mapping) but does **not** need the full `StdVideoH265LevelIdc` conversion (`std_level_idc`) Vulkan's decode needed — that conversion is Vulkan-specific plumbing with no VA-API analogue, **not ported**. |
| SPS parse through `strong_intra_smoothing_enabled_flag` | `vulkan/hevc_params.rs::HevcSps::parse` (`hevc_params.rs:314-450`) | **Reusable almost verbatim** — every field this crate's own decode needs (`log2_max_pic_order_cnt_lsb`, `max_dec_pic_buffering`, CU/TU log2 sizes, `amp_enabled_flag`, `sample_adaptive_offset_enabled_flag`, `sps_temporal_mvp_enabled_flag`, `strong_intra_smoothing_enabled_flag`) is already parsed and retained there, for the exact same "must echo exactly or desync the driver's slice-header parser" reason this ADR's own slice-header work depends on (see below) |
| `ShortTermRefPicSet::parse` | `vulkan/hevc_slice.rs:99-146` | **Reusable verbatim** — pure ITU-T H.265 §7.3.7 arithmetic, zero Vulkan dependency, already real/sans-io/tested. This ADR's own scope additionally **validates** the parsed shape (`s0.len() == 1 && s1.is_empty() && s0[0].delta_poc == -1 && s0[0].used_by_curr_pic`) and rejects anything else as `Unsupported` — a new, this-ADR-specific check, not present in the porting source (which stays general since Vulkan's own future multi-reference work would need the general case) |
| `HevcSliceSegmentHeader::parse`'s structure through the RPS | `vulkan/hevc_slice.rs:201-261` | **Starting point only** — this ADR's own parser must continue well past where this function currently stops (see § Slice-header parser extension); the *shape* (take `sps`/`pps`/`nal_unit_type`, return a struct, stop at a documented boundary) is reused, the *content* is not import-shared |

### PPS fields this ADR's own parser must add beyond `vulkan::hevc_params::HevcPps`

Cross-referencing every `VAPictureParameterBufferHEVC`/`VASliceParameterBufferHEVC` field
`cros-libva` 0.0.13 requires (confirmed by reading `hevc.rs`'s real vendored source directly)
against ITU-T H.265 §7.3.2.3.1's real `pic_parameter_set_rbsp()` syntax order (cross-checked via
FFmpeg's `libavcodec/hevc/ps.c` convention already cited by this crate's own Vulkan
`hevc_params.rs` module doc, and independently re-confirmed against real spec ordering this
session) surfaces **real fields `vulkan::hevc_params::HevcPps::parse` never reads at all** —
that parser stops right after checking `deblocking_filter_control_present_flag`, but the real
syntax continues:

```
pps_scaling_list_data_present_flag        // if 1: scaling_list_data() — not designed this ADR
lists_modification_present_flag           // needed for VASliceParsingFields
log2_parallel_merge_level_minus2          // needed: VAPictureParameterBufferHEVC top-level field
slice_segment_header_extension_present_flag  // if 1: extra slice-header bits not designed this ADR
pps_extension_present_flag                // if 1: range/SCC/etc extensions not designed this ADR
```

This crate's own `linux/vaapi/hevc_pps.rs` reads all five, **rejecting the stream as
`Unsupported`** whenever `pps_scaling_list_data_present_flag`,
`slice_segment_header_extension_present_flag`, or `pps_extension_present_flag` is `1` (mirrors
this crate's H.264 sibling's own honest-rejection convention for out-of-scope PPS extension
syntax), and **retaining** `lists_modification_present_flag` (echoed into
`HevcSliceParsingFields`, functionally inert for this ADR's scope since `NumPicTotalCurr` is
always `≤ 1` here — see below — but still a real value the driver-facing struct must carry
honestly, not a hardcoded `0`) and `log2_parallel_merge_level_minus2` (a real
`PictureParameterBufferHEVC` top-level field with no Vulkan-side equivalent need). This is this
ADR's own version of ADR-0002's "found two real latent gaps in the porting source" finding — here
the gap is larger (a genuinely new PPS-parsing tail, not two isolated missing bits), because this
ADR's starting point (Vulkan's IDR-only-decode-tuned `HevcPps::parse`) was never pressure-tested
against a real driver's per-slice field requirements the way H.264's sibling porting source was.

### Slice-header parser extension — the largest genuinely new design surface in this ADR

`vulkan::hevc_slice::HevcSliceSegmentHeader::parse` stops immediately after parsing the
short-term RPS (its own module doc: *"trailing per-slice syntax... is left unread"* — correct and
sufficient for Vulkan, since Vulkan Video hands the **raw bitstream bytes** to the hardware
decoder and the hardware parses the slice header itself; this crate's own app-must-parse VA-API
model has no such luxury, exactly the asymmetry [ADR-0001](0001-vaapi-h264-cpu-out.md) already
established for H.264: *"the driver does entropy decoding + motion compensation + reconstruction,
but the application must parse SPS/PPS/slice headers itself."*

Real ITU-T H.265 §7.3.6.1 syntax order after the RPS block (confirmed this session via FFmpeg's
`libavcodec/hevc/hevcdec.c::hls_slice_header`, fetched and read directly — every gating condition
below is quoted from that real source, not recalled from memory), **narrowed by this ADR's own
scope cuts** (no B-slices, ≤1 active reference, no weighted prediction, no deblocking-control
override, no tiles/WPP, no slice-header extension — every one of these already rejected upstream
by `hevc_sps.rs`/`hevc_pps.rs`'s own scope cuts above and in the existing SPS/PPS scope), so the
*reachable* syntax for any stream this crate accepts is small and fully enumerable:

1. `if (sps.sps_temporal_mvp_enabled_flag && !is_idr)`: read `slice_temporal_mvp_enabled_flag`
   (1 bit). *(Only present for non-IDR — real spec nests this inside the same `if
   (nal_unit_type != IDR_W_RADL/N_LP)` block the RPS itself is in.)*
2. `if (sps.sample_adaptive_offset_enabled_flag)`: read `slice_sao_luma_flag` (1 bit), then (since
   `chroma_format_idc == 1` always, this crate's SPS scope) `slice_sao_chroma_flag` (1 bit).
   *(Present for **every** slice type including IDR — the same real hardware lesson
   `vulkan::hevc_params.rs`'s own `HevcSps::sample_adaptive_offset_enabled_flag` doc comment
   already recorded for its own SPS-level flag; this ADR's own slice-header parser must act on
   that lesson a second time, at the per-slice level, since it is a genuinely separate bug class
   from the SPS-flag-echo bug Vulkan's decode already fixed.)*
3. `if (slice_type == P)`: read `num_ref_idx_active_override_flag` (1 bit); if set, read
   `num_ref_idx_l0_active_minus1` (`ue`) and **reject as `Unsupported` unless the resulting count
   is exactly `1`** (mirrors this crate's H.264 sibling ADR-0002's identical
   `num_ref_idx_l0_active`-must-equal-`1` scope cut). Because this crate's accepted streams never
   have more than one active reference, `pps.lists_modification_present_flag && nb_refs > 1` is
   always false for them — `ref_pic_lists_modification()` is **provably unreachable**, not merely
   unimplemented, given this ADR's own upstream rejection; no reordering-syntax parsing is needed.
   `mvd_l1_zero_flag` is B-slice-only, unreachable (B rejected at slice-type parse). If
   `pps.cabac_init_present_flag`: read `cabac_init_flag` (1 bit). If
   `slice_temporal_mvp_enabled_flag` (step 1): `collocated_list` is implicitly `L0` for a P slice
   (no bit read — the real spec only reads a `collocated_from_l0_flag` bit for B slices), and
   `collocated_ref_idx` (`ue`) is only read `if (nb_refs[collocated_list] > 1)` — **also provably
   unreachable** here (`nb_refs[L0]` is always exactly `1`). If `pps.weighted_pred_flag` (rejected
   upstream, § PPS fields — always `0` for any stream this crate accepts): `pred_weight_table()`
   is unreachable. Read `five_minus_max_num_merge_cand` (`ue`) unconditionally for P slices.
4. Read `slice_qp_delta` (`se`) unconditionally.
5. `if (pps.pps_slice_chroma_qp_offsets_present_flag)`: read `slice_cb_qp_offset` (`se`),
   `slice_cr_qp_offset` (`se`).
6. `deblocking_filter_control_present_flag`'s sub-syntax (`deblocking_filter_override_flag`,
   `slice_deblocking_filter_disabled_flag` + offsets) is **provably unreachable** — this ADR's own
   `hevc_pps.rs` rejects any PPS with that flag set to `1` (same rejection
   `vulkan::hevc_params.rs::HevcPps::parse` already makes today, unchanged, just now also
   guaranteeing this slice-header branch is dead code for every accepted stream).
7. `if (pps.pps_loop_filter_across_slices_enabled_flag && (sao_luma || sao_chroma ||
   !disable_deblocking_filter_flag))`: read `slice_loop_filter_across_slices_enabled_flag` (1
   bit). Since step 6 is unreachable, `disable_deblocking_filter_flag` is always its spec-default
   `0`, so `!disable_deblocking_filter_flag` is always `true` — this condition **always**
   simplifies to `pps.pps_loop_filter_across_slices_enabled_flag` alone for any stream this crate
   accepts. *(The same real per-slice-bit lesson `vulkan::hevc_params.rs::HevcPps`'s own
   `pps_loop_filter_across_slices_enabled_flag` field doc already recorded finding, for the
   PPS-echo bug — this is that same flag's *slice-header* syntax consequence, which Vulkan's
   decode never needed to consume since the hardware parses it directly there.)*
8. Tile/WPP entry-point-offset syntax and `slice_segment_header_extension_present_flag`'s syntax
   are **provably unreachable** — both rejected upstream at PPS parse (§ PPS fields, this crate's
   own new checks).
9. `byte_alignment()`: `alignment_bit_equal_to_one` (1 bit, must be `1` — reject `Unsupported`
   otherwise, matching this crate's H.264 sibling's own strict-parse convention) then zero-pad to
   the next byte boundary.

`BitReader::bits_read()` (`mediaway-sw`, already used by this crate's H.264 `SliceHeader::
bits_consumed`) gives this parser everything it needs to compute two VA-API-required derived
values without any new primitive: **`st_rps_bits`** (`VAPictureParameterBufferHEVC`'s own field,
libva doc: *"number of bits that structure `short_term_ref_pic_set()` takes in slice segment
header when `short_term_ref_pic_set_sps_flag` equals 0"*) is `bits_read()` immediately after
`ShortTermRefPicSet::parse` returns, minus `bits_read()` immediately before calling it; **
`slice_data_byte_offset`** (`VASliceParameterBufferHEVC`, libva doc: *"byte offset from NAL unit
header to the beginning of `slice_data()`"*) is `2 (NAL header bytes) +
(total_bits_consumed_through_step_9).div_ceil(8)`.

### DPB design — fresh, not a port (see § Context)

```rust
// linux/vaapi/hevc_dpb.rs — new file, sans-io, no cros_libva types.
//! Single-forward-reference HEVC DPB: at most one non-current picture is ever tracked, since
//! this crate's own slice-header parser (`hevc_slice.rs`) already rejects any short-term RPS
//! shape other than "exactly one negative-direction entry, delta_poc == -1, used". This is
//! *not* a port of `super::dpb`'s general H.264 sliding-window `Dpb` (that machinery has no
//! reachable code path once occupancy is capped at one) — closer in spirit to
//! `mediaway-encoder::vulkan::hevc_gop::GopState`'s own single-slot `last_written` design.
#![forbid(unsafe_code)]

pub(super) struct HevcDpbSlot {
    pub(super) pic_order_cnt: i32,
}

/// One optional reference slot (the immediately preceding reference picture, if any) plus the
/// physical-surface-index it lives at — `surfaces`/index bookkeeping stays in `hevc.rs::Pipeline`
/// (mirrors `linux/vaapi/dpb.rs`'s own "no pixel data, no VA-API surface handle" convention).
pub(super) struct HevcDpb {
    reference: Option<(usize, HevcDpbSlot)>,
}

impl HevcDpb {
    pub(super) const fn new() -> Self { Self { reference: None } }
    /// An IDR picture clears the tracked reference (ITU-T H.265 §C.5.2.2 semantics: an IDR
    /// access unit empties the DPB of prior reference pictures).
    pub(super) fn clear(&mut self) { self.reference = None; }
    pub(super) const fn reference(&self) -> Option<&(usize, HevcDpbSlot)> { self.reference.as_ref() }
    pub(super) fn set_reference(&mut self, slot_index: usize, pic_order_cnt: i32) {
        self.reference = Some((slot_index, HevcDpbSlot { pic_order_cnt }));
    }
}
```

- Surface pool: **3 slots** (current + the one tracked reference + one in-flight headroom slot,
  mirroring this crate's H.264 sibling's `+1` sizing comment's own reasoning, applied to a design
  that only ever needs one reference instead of `sps.max_dec_pic_buffering` many) — a fixed,
  small constant (`HEVC_SURFACE_POOL_SIZE: usize = 3`), not computed from
  `sps.max_dec_pic_buffering` the way the H.264 sibling's pool is, since this ADR's own RPS-shape
  validation (§ What *is* reusable, `ShortTermRefPicSet::parse` row) already guarantees no stream
  this crate accepts ever needs more than one tracked reference regardless of what
  `sps.max_dec_pic_buffering` itself declares.
- Slot selection: round-robin among the 3 physical surfaces, **skipping** whichever index
  currently holds `dpb.reference()`'s tracked slot (a 2-line linear scan, not a general
  allocator) — simpler than porting `Dpb::allocate_slot`'s free-slot-or-evict logic, since with 3
  physical slots and at most 1 protected reference, a free (non-reference) slot always exists.
- `derive_pic_order_cnt_msb` is **imported, not re-ported**, from this crate's own
  `super::dpb::derive_pic_order_cnt_msb` (`linux/vaapi/dpb.rs`, `pub(super)`, ADR-0002, landed
  this same session) — the ITU-T H.265 §8.3.1 formula is the identical MSB/LSB-wraparound
  arithmetic H.264's §8.2.1.1 already implements (confirmed by reading both spec derivations this
  session; FFmpeg's own `hevcdec.c` POC-derivation code and `h264dec.c`'s equivalent share the
  same shape) — reusing the already-landed, already-tested sibling function inside the same crate
  avoids a third copy of one small, easy-to-get-wrong formula (`prev_poc_msb`/`prev_poc_lsb` are
  carried the same way `VaapiH264Decoder`'s own fields already are, reset to `0` on every IDR,
  updated only when the decoded picture is a reference — HEVC has no field-coding pair to worry
  about, so `PicOrderCntVal` needs no top/bottom duplication the way H.264's does).

### VA-API-specific plumbing — fresh design (confirmed against real `cros-libva` source + FFmpeg)

**Confirmed by reading `cros-libva` 0.0.13's real vendored source directly**
(`.../cros-libva-0.0.13/src/buffer/hevc.rs`):

- `PictureHEVC::new(picture_id: VASurfaceID, pic_order_cnt: i32, flags: u32)` — same simpler
  (no `frame_idx`) shape the sibling encode ADR-0003 already cites.
- `SliceParameterBufferHEVC::new(..., ref_pic_list: [[u8; 15]; 2], long_slice_flags:
  &HevcLongSliceFlags, ...)` — **`ref_pic_list` is `[[u8; 15]; 2]` (indices into
  `ReferenceFrames[]`), not `[PictureHEVC; N]`** — a real, cited, structural asymmetry versus this
  crate's own H.264 decode (`SliceParameterBufferH264::ref_pic_list_0: [PictureH264; 32]`, full
  structs) **and** versus this codec's own *encode* side
  (`EncSliceParameterBufferHEVC::ref_pic_list0: [PictureHEVC; 15]`, also full structs) —
  independently confirmed via Intel's real `va_dec_hevc.h` (fetched this session): *"index into
  `ReferenceFrames[]`, value range `[0..14, 0xFF]`, where `0xFF` indicates invalid entry."* This
  crate's `build_slice_param` therefore looks up the reference's **position** in the
  `ReferenceFrames` array it just built for `PictureParameterBufferHEVC`, not a fresh `PictureHEVC`
  copy.
- `PictureParameterBufferHEVC::new(curr_pic, reference_frames: [PictureHEVC; 15], ...,
  slice_parsing_fields: &HevcSliceParsingFields, log2_max_pic_order_cnt_lsb_minus4: u8,
  num_short_term_ref_pic_sets: u8, num_long_term_ref_pic_sps: u8, ..., st_rps_bits: u32)` — every
  SPS/PPS-derived scalar field maps directly onto this crate's own already-parsed `HevcSps`/
  `HevcPps` values (`log2_max_pic_order_cnt_lsb - 4`, CU/TU log2 sizes, `init_qp - 26`,
  `diff_cu_qp_delta_depth`, `pps_cb_qp_offset`/`cr_qp_offset`,
  `num_ref_idx_l0_default_active_minus1`/`l1`, `num_extra_slice_header_bits`,
  `log2_parallel_merge_level_minus2` — the field this ADR's own PPS extension adds).
  `num_short_term_ref_pic_sets = 0` / `num_long_term_ref_pic_sps = 0` (SPS-level RPS/long-term
  lists rejected upstream, unchanged from Vulkan's identical scope cut). `sps_max_dec_pic_buffering_minus1
  = (sps.max_dec_pic_buffering - 1) as u8` (echoed from the SPS as parsed, **not** capped to this
  ADR's own 1-reference practical limit — the driver is told the stream's own declared value,
  same "declare what the stream says, not what we happen to use" principle the H.264 sibling's
  `num_ref_frames` field already established).
  - `pic_fields`/`slice_parsing_fields` (`HevcPicFields`/`HevcSliceParsingFields`): every flag
    argument is a direct echo of an already-parsed `HevcSps`/`HevcPps` field (§ context: "must be
    echoed exactly" is the single most-repeated lesson in this crate's own Vulkan HEVC decode
    history, and applies identically here) — `chroma_format_idc = 1`,
    `separate_colour_plane_flag = 0`, `pcm_enabled_flag = 0`, `scaling_list_enabled_flag = 0`
    (all rejected-if-1 upstream, so always `0`), `transform_skip_enabled_flag =
    pps.transform_skip_enabled_flag`, `amp_enabled_flag = sps.amp_enabled_flag`,
    `strong_intra_smoothing_enabled_flag = sps.strong_intra_smoothing_enabled_flag`,
    `sign_data_hiding_enabled_flag = pps.sign_data_hiding_enabled_flag`,
    `constrained_intra_pred_flag = pps.constrained_intra_pred_flag`, `cu_qp_delta_enabled_flag =
    pps.cu_qp_delta_enabled_flag`, `weighted_pred_flag = pps.weighted_pred_flag` (always `0`,
    rejected upstream if `1`), `weighted_bipred_flag = pps.weighted_bipred_flag` (same),
    `transquant_bypass_enabled_flag = pps.transquant_bypass_enabled_flag`, `tiles_enabled_flag =
    0`, `entropy_coding_sync_enabled_flag = 0` (both rejected upstream), 
    `pps_loop_filter_across_slices_enabled_flag = pps.pps_loop_filter_across_slices_enabled_flag`,
    `loop_filter_across_tiles_enabled_flag = 0` (unreachable, tiles disabled),
    `pcm_loop_filter_disabled_flag = 0` (unreachable, PCM disabled), **`no_pic_reordering_flag =
    1`, `no_bi_pred_flag = 1`** — real VA-API decode *hint* fields (not H.265 bitstream syntax
    elements at all; libva doc-confirmed as driver hints), honestly `1` given this ADR's own
    permanent "no reordering, no B-slices" scope, **not independently confirmed against how a
    real driver actually uses these hints** (flagged in § Open questions). `slice_parsing_fields`:
    `lists_modification_present_flag = pps.lists_modification_present_flag` (this ADR's new PPS
    field, echoed honestly though functionally inert here), `long_term_ref_pics_present_flag = 0`
    (rejected upstream), `sps_temporal_mvp_enabled_flag = sps.sps_temporal_mvp_enabled_flag`,
    `cabac_init_present_flag = pps.cabac_init_present_flag`, `output_flag_present_flag =
    pps.output_flag_present_flag`, `dependent_slice_segments_enabled_flag =
    pps.dependent_slice_segments_enabled_flag` (real PPS-level value, even though every *accepted*
    slice has `dependent_slice_segment_flag == 0` — the PPS may still legally enable the
    capability without any given slice using it), `pps_slice_chroma_qp_offsets_present_flag =
    pps.pps_slice_chroma_qp_offsets_present_flag`, `sample_adaptive_offset_enabled_flag =
    sps.sample_adaptive_offset_enabled_flag`, `deblocking_filter_override_enabled_flag = 0`,
    `pps_disable_deblocking_filter_flag = 0` (both unreachable, § PPS fields),
    `slice_segment_header_extension_present_flag = 0` (rejected upstream if `1`), `rap_pic_flag =
    idr_pic_flag = intra_pic_flag = u32::from(is_idr)` (this ADR's own scope: only IDR pictures
    are intra/random-access; every P picture is neither).
- `ReferenceFrames[]` construction (**confirmed via FFmpeg's real `libavcodec/vaapi_hevc.c`**,
  fetched and read this session): `init_vaapi_pic`/`fill_vaapi_pic` iterate *every* DPB entry
  (here: at most one), setting `flags` from `find_frame_rps_type` —
  `VA_PICTURE_HEVC_RPS_ST_CURR_BEFORE` when the reference's POC is less than the current
  picture's (always true in this ADR's own no-reordering scope — the one tracked reference is
  always temporally before the picture being decoded), `VA_PICTURE_HEVC_INVALID` for the 14
  unused fill entries (mirrors `invalid_picture()`'s existing H.264 convention, applied to
  `PictureHEVC::new(VA_INVALID_SURFACE, 0, VA_PICTURE_HEVC_INVALID)`). **Exact numeric values of
  `VA_PICTURE_HEVC_INVALID`/`VA_PICTURE_HEVC_RPS_ST_CURR_BEFORE` are not independently confirmed
  against a real bindgen build this session** (unlike `VA_PICTURE_H264_SHORT_TERM_REFERENCE`,
  which ADR-0002 already confirmed `= 8` from a real WSL2 build) — flagged prominently in §
  Open questions as this ADR's single highest-priority unconfirmed item, mirroring how seriously
  ADR-0002 treated the analogous H.264 constant.

### Codec/profile mapping and struct shape

`linux/vaapi/codec.rs` gains an HEVC profile-candidate function mirroring
`h264_profile_candidates` (`VAProfileHEVCMain`, `= 17`, confirmed against the real `intel/libva`
`va.h` fetched this session — same disposition as the sibling encode ADR-0003's identical
citation) and adds `CodecKind::Hevc` to `is_supported_video_codec`.

```rust
// linux/vaapi/hevc.rs — new file, mirrors h264.rs's Pipeline/decode_picture/decode_one shape.
struct HevcPipeline {
    _config: Config,
    context: Rc<Context>,
    surfaces: Vec<Option<Surface<()>>>,   // fixed HEVC_SURFACE_POOL_SIZE == 3, not SPS-sized
    dpb: hevc_dpb::HevcDpb,
    coded_width: u32,
    coded_height: u32,
    nv12_format: VAImageFormat,
}

pub(crate) struct VaapiHevcDecoder {
    display: Rc<Display>,
    pipeline: Option<HevcPipeline>,
    sps: Option<hevc_sps::HevcSps>,
    pps: Option<hevc_pps::HevcPps>,
    info: StreamInfo,
    declared_width: u32,
    declared_height: u32,
    pending: VecDeque<VideoFrame>,
    flushed: bool,
    prev_poc_msb: i32,   // carried across pictures, reused via super::dpb::derive_pic_order_cnt_msb
    prev_poc_lsb: u32,
}
```

`crates/mediaway-decoder/src/linux/vaapi/mod.rs` gains `mod hevc; mod hevc_dpb; mod hevc_nal; mod
hevc_pps; mod hevc_sps; mod hevc_slice;`. `crates/mediaway-decoder/src/linux/mod.rs`'s
`LinuxVideoDecoder.inner` field changes from `Option<vaapi::VaapiH264Decoder>` to a new
`Option<vaapi::VaapiVideoSession>` enum (`H264(VaapiH264Decoder) | Hevc(VaapiHevcDecoder)`) — the
same enum-dispatch shape (no `Box<dyn VideoDecoder>`) the sibling encode ADR-0003 designs for
`mediaway-encoder::linux::vaapi::VaapiVideoSession`, for the identical ZCA reasoning; `open()`
dispatches on `config.codec`.

## Scope

**In (this ADR):**

- HEVC Main profile, `VAEntrypointVLD`, CPU NV12 output only — same input/output axes this
  crate's H.264 path already supports.
- IDR I-slices (real decode, DPB cleared, POC reset) and single-forward-reference P-slices
  (exactly the RPS shape the sibling encoder ADR-0003 emits) — any other RPS shape, or any
  CRA/random-access picture, rejected as `Unsupported`.
- `linux/vaapi/hevc_nal.rs`/`hevc_sps.rs`/`hevc_pps.rs`/`hevc_slice.rs`: fresh, crate-local
  parsers starting from `vulkan::hevc_params`/`hevc_slice`'s real, cited logic, extended with the
  PPS tail (§ PPS fields) and full reachable slice-header syntax (§ Slice-header parser
  extension) VA-API's parameter buffers require.
- `linux/vaapi/hevc_dpb.rs`: fresh, single-slot DPB design (§ DPB design), reusing
  `super::dpb::derive_pic_order_cnt_msb` from this crate's own already-landed H.264 sibling.
- `VaapiVideoSession` enum dispatch in `linux/vaapi/mod.rs`/`linux/mod.rs`, mirroring the sibling
  encoder ADR-0003's identical shape.

**Out (deferred, tracked in `docs/roadmap.md`):**

- CRA / random-access pictures, leading pictures (RASL/RADL) — permanent scope cut *for now*;
  Vulkan's own decode module already tracks `HevcNalUnitType::Cra` for POC-derivation purposes
  without decoding it, a smaller future increment than this ADR attempts.
- B-slices, multi-reference, reference-list reordering, long-term references, weighted
  prediction, CABAC-affecting PPS extensions (`pps_scaling_list_data_present_flag`,
  `slice_segment_header_extension_present_flag`, `pps_extension_present_flag`) — permanent
  non-goals for this ADR, matching every sibling ADR's identical narrowing this session.
  `entropy_coding_sync_enabled_flag` (WPP) and `tiles_enabled_flag` — deferred, matching H.264's
  identical deferral.
- HEVC Main 10 / Range Extensions / SCC profiles — 8-bit 4:2:0 Main only.
- Zero-Copy DMA-BUF surface export — unrelated axis, ADR-0001's own deferral, untouched.
- Reconciling `vulkan::hevc_slice.rs`'s own missing slice-header tail (this ADR's § Slice-header
  parser extension covers ground that module's doc explicitly never attempted) back into the
  Vulkan module itself — flagged as a legitimate, independent follow-up, not actioned here (same
  disposition ADR-0002 gave its own analogous H.264 finding against `vulkan::h264_slice.rs`).

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Wait for Vulkan's own HEVC P-slice decode to be hardware-verified first, then port it (mirroring the H.264 precedent exactly) | Rejected per this task's own explicit instruction to design this now; also, waiting would block on unrelated Vulkan Video HEVC decode work with its own schedule, when this ADR's own fresh design (grounded in FFmpeg's real, sourced conventions) is buildable and independently testable today. |
| Port `linux/vaapi/dpb.rs`'s general H.264 sliding-window `Dpb` for HEVC too (generalize it, codec-agnostic) | Rejected — H.264's `Dpb` carries `frame_num`/`FrameNumWrap` fields with no HEVC equivalent, and its sliding-window eviction logic has no reachable code path once this ADR's own RPS-shape validation caps occupancy at one reference; a shared generalized type would need a redesign to accommodate a concept (`FrameNumWrap`) the new codec does not have, which is not simpler than a small, purpose-fit `HevcDpb`. |
| Accept any RPS shape (general short-term RPS, up to 8 negative + 8 positive entries, matching `ShortTermRefPicSet::parse`'s own general capacity) instead of rejecting anything but the single-forward-reference shape | Rejected — this session's task explicitly directs single-forward-reference scope, matching every sibling ADR; accepting a general RPS would require a general multi-reference DPB (this ADR's own § "why not H.264's general Dpb" reasoning would then partly reverse), `ReferenceFrames[]` construction for several simultaneous entries, and `RefPicList` index resolution against a bigger active set — real, larger scope this ADR does not attempt. |
| Accept CRA pictures as intra, alongside IDR | Rejected this pass — real, if modest, additional complexity (leading-picture handling, `no_output_of_prior_pics_flag` semantics differ from IDR's) for a picture type this workspace's own sibling encoder never emits; deferred as a small, independent, well-scoped follow-up rather than folded in here. |
| Import `crate::vulkan::hevc_slice`/`hevc_params` types directly (same crate) | Rejected for the same reason ADR-0002 rejected the equivalent H.264 import, and for a stronger structural reason here: this ADR's own parser genuinely needs to read *more* bitstream syntax than the Vulkan module does (§ Slice-header parser extension) — a shared type could not represent "the VA-API variant reads further" without either forking the type anyway or complicating the Vulkan module with fields it has no use for. |

## Consequences

### Positive

- HEVC joins H.264 as a real, GOP-capable VA-API decode codec in this crate.
- Closes a real, freshly-discovered PPS-parsing and slice-header-parsing gap the *existing*
  Vulkan HEVC decode module's own scope never needed to close (§ PPS fields, § Slice-header
  parser extension) — every reachable syntax element for this ADR's own accepted-stream shape is
  now enumerated and cited against real spec/FFmpeg source, not left implicit.
- `derive_pic_order_cnt_msb` reuse (not a third copy) keeps this crate's one hand-written
  MSB/LSB-wraparound formula in one already-tested place.
- The single-slot `HevcDpb` design is provably simpler and smaller than a generalized RPS/DPB
  would need to be, directly because this ADR's own RPS-shape validation makes every "what if
  there were 2+ references" code path unreachable by construction, not just untested.

### Negative / Trade-offs

- **This ADR carries materially more unconfirmed risk than its H.264 sibling ADR-0002** — no
  hardware-verified porting source exists for the DPB/reference-list/slice-header-tail logic this
  ADR designs fresh; every one of those design decisions is grounded in spec text and FFmpeg
  source read this session, not in a previously-debugged, previously-hardware-verified
  implementation the way ADR-0002's H.264 port was.
- `VA_PICTURE_HEVC_INVALID`/`VA_PICTURE_HEVC_RPS_ST_CURR_BEFORE`'s exact numeric values are
  unconfirmed against this session's own bindgen output (§ Open questions #1) — a real,
  higher-stakes gap than any single open item ADR-0002 carried, since this ADR's entire
  `ReferenceFrames[]`/DPB-flag-marking design depends on them.
- Rejecting `pps_scaling_list_data_present_flag`/`slice_segment_header_extension_present_flag`/
  `pps_extension_present_flag` outright (§ PPS fields) means this decoder cannot accept any real
  encoder's output that sets any of these — plausibly including drivers' own hardware HEVC
  encoders, which this crate cannot control the same way it controls its own sibling
  `mediaway-encoder`'s VA-API HEVC output (ADR-0003, which deliberately avoids setting SAO/
  temporal-MVP but says nothing about these three PPS-extension flags one way or the other —
  flagged as a cross-crate item worth double-checking once both sides are implemented).
- The single-slot `HevcDpb`'s 3-surface pool is a fixed constant, not derived from
  `sps.max_dec_pic_buffering` — a stream that legitimately declares a larger DPB (even though
  this crate's own RPS-shape rejection guarantees it never *needs* more than one tracked
  reference) still gets a fixed-size pool; this is a deliberate simplification, not a bug, but
  worth flagging as a design choice a future generalization would need to revisit.

## Test plan (for the implementation pass that follows this ADR)

- **Sans-io, hardware-independent (highest-value, run first)**: `linux/vaapi/hevc_slice_tests.rs`
  — every reachable/unreachable branch enumerated in § Slice-header parser extension, hand-computed
  bit counts for `st_rps_bits`/`slice_data_byte_offset` (mirrors this crate's existing H.264
  `slice_tests.rs`'s own hand-computed-bit-count regression style); `linux/vaapi/hevc_pps_tests.rs`
  covering the new PPS tail (§ PPS fields) including all three new rejection cases;
  `linux/vaapi/hevc_dpb_tests.rs` covering `HevcDpb::clear`/`set_reference`/slot-selection.
- **`hevc.rs` integration** (hardware-gated, `_or_skip_without_hw`-style, expected to skip in this
  session/CI without real `/dev/dri/renderD*`): a hand-constructed VPS+SPS+PPS+IDR+P Annex-B
  stream exercising DPB reference tracking across 3+ pictures and an IDR mid-stream correctly
  clearing the tracked reference — same technique `adr/vulkan/0001`'s own
  `tests/hardware_hevc_decode.rs` used to get controllable content without a real encoder,
  adapted for this crate's own VA-API buffers.
- **Cross-crate round-trip** (the primary correctness bar for this ADR, given the "no
  hardware-verified porting source" caveat above): feed this workspace's own sibling
  `mediaway-encoder::linux::vaapi` HEVC output (ADR-0003, same session — deliberately SAO/
  temporal-MVP-disabled, matching every slice-header branch this ADR's own parser actually
  implements) into this decoder — the nearest available oracle, since system `ffprobe`/`ffmpeg`
  validates bitstream conformance but does not exercise *this crate's own* parameter-buffer
  construction the way a real VA-API `vaBeginPicture`/`vaRenderPicture` call would.
- **Codec-profile smoke**: HEVC profile-candidate resolution returns `[VAProfileHEVCMain]`;
  `is_supported_video_codec(CodecKind::Hevc)` is `true`.
- **WSL2 real-Linux compile verification**: `cargo check`/`cargo test --lib`/`cargo clippy
  --all-targets -- -D warnings` for `mediaway-decoder` on a real Linux target via WSL2 Ubuntu with
  real `libva-dev` — confirms `VA_PICTURE_HEVC_INVALID`/`VA_PICTURE_HEVC_RPS_ST_CURR_BEFORE`'s real
  numeric values (this ADR's single highest-priority open risk) and every `cros-libva` HEVC
  decode-buffer field assumption cited above against the real bindgen output. Must be run before
  this ADR's implementation pass is considered even compile-verified.
- Default `cargo test --workspace` (no system FFmpeg, no VA-API hardware) must keep passing —
  every new sans-io test above requires neither.

## Open questions / risks (explicit, for whoever picks up the implementation pass)

1. **`VA_PICTURE_HEVC_INVALID`/`VA_PICTURE_HEVC_RPS_ST_CURR_BEFORE`/`_RPS_ST_CURR_AFTER`/
   `_LONG_TERM_REFERENCE`'s exact numeric values** — confirmed to exist, by name, with the
   documented mutual-exclusivity semantics ("no more than one of `RPS_ST_CURR_BEFORE`/
   `RPS_ST_CURR_AFTER`/`RPS_LT_CURR` can be set for any picture," per Intel's official libva API
   docs, searched this session), but **not** confirmed against a real bindgen build the way
   `VA_PICTURE_H264_SHORT_TERM_REFERENCE = 8` was in ADR-0002 — this crate's single
   highest-priority open risk, since it blocks even a compile-correct first draft of
   `ReferenceFrames[]` construction.
2. **Whether `cros_libva::VAProfile::VAProfileHEVCMain`'s real enum variant name matches this
   crate's own WSL2 bindgen output** — same open item the sibling encode ADR-0003 carries,
   unconfirmed here too.
3. **Whether a real VA-API driver actually honors `no_pic_reordering_flag`/`no_bi_pred_flag` as
   pure hints (as libva's own field naming/doc strongly implies), or silently requires them to be
   `0` on some hardware** — inferred from field semantics, not confirmed against a real driver.
4. **Whether this workspace's own sibling `mediaway-encoder` VA-API HEVC output (ADR-0003) is
   actually decodable by this ADR's design once both are implemented** — this ADR was
   *deliberately* designed against that encoder's exact expected output shape (single-reference
   RPS, SAO/temporal-MVP disabled), but the round-trip itself is unverified until both sides are
   implemented and run together, even before real hardware enters the picture.
5. **Whether real-world HEVC encoders (hardware or software) commonly set
   `pps_scaling_list_data_present_flag`/`pps_extension_present_flag`** in ways this ADR's outright
   rejection would make undecodable — unconfirmed; if common, a future ADR would need to actually
   parse (not just detect-and-reject) `scaling_list_data()`/PPS range-extension syntax.
6. **The exact byte-vs-bit-offset convention VA-API expects for `slice_data_byte_offset`** — this
   ADR's own `2 + bits.div_ceil(8)` formula is derived from libva's doc text ("byte offset from
   NAL unit header to the beginning of `slice_data()`") and HEVC's own guaranteed
   byte-alignment-before-`slice_data()` property, mirrored against this crate's own H.264
   `slice_data_bit_offset`'s analogous "relative to and includes the NAL unit byte" convention —
   not independently confirmed against a real driver or FFmpeg's exact computation this session.

## Addendum (2026-08-19, confirmed via real WSL2 bindgen output)

Open question #1 and #2 are now closed. Real `cros-libva` bindgen output
(`target/x86_64-unknown-linux-gnu/debug/build/cros-libva-*/out/bindings.rs`, generated by this
same WSL2 Ubuntu environment ADR-0002 already used):

```
pub const VA_PICTURE_HEVC_INVALID: u32 = 1;
pub const VA_PICTURE_HEVC_LONG_TERM_REFERENCE: u32 = 8;
pub const VA_PICTURE_HEVC_RPS_ST_CURR_BEFORE: u32 = 16;
pub const VA_PICTURE_HEVC_RPS_ST_CURR_AFTER: u32 = 32;
pub const VA_PICTURE_HEVC_RPS_LT_CURR: u32 = 64;
pub const VAProfileHEVCMain: Type = 17;
```

`VAProfileHEVCMain = 17` matches the value already cited from `va.h` in § Codec/profile mapping —
no change needed there. `ctu_max_bitsize_allowed` (the sibling encode ADR-0003's own open
question) was also confirmed present as a real `u8` field in the same bindgen pass.

Open question #3 (`no_pic_reordering_flag`/`no_bi_pred_flag` real-driver behavior), #5
(real-world PPS-extension prevalence), and #6 (`slice_data_byte_offset` exact convention) remain
open — none are resolvable from bindgen output alone; they need either a real driver or a real
reference bitstream to test against, deferred to the implementation/test pass.

## References

- [ADR-0001](0001-vaapi-h264-cpu-out.md) — this crate's binding choice, H.264-only original scope
- [ADR-0002](0002-vaapi-h264-p-slice-dpb.md) — this crate's H.264 P-slice DPB sibling, same
  session; source of the reused `derive_pic_order_cnt_msb` (`linux/vaapi/dpb.rs`) and the
  porting-methodology precedent this ADR explicitly could **not** fully replicate (see § Context)
- `crates/mediaway-decoder/src/vulkan/decoder_hevc.rs` — confirms Vulkan's own HEVC decode is
  IDR-only in practice (module doc + dispatch logic, lines 7-14, 236-238)
- `crates/mediaway-decoder/src/vulkan/hevc_slice.rs` — `ShortTermRefPicSet::parse` (reused
  verbatim), `HevcSliceSegmentHeader::parse` (starting point only, extended well past its own
  documented stopping point — see § Slice-header parser extension)
- `crates/mediaway-decoder/src/vulkan/hevc_params.rs` — `HevcNalUnit::parse`,
  `remove_emulation_prevention`, `HevcSps::parse` (starting point, reused near-verbatim),
  `HevcPps::parse` (starting point, extended with a real new PPS tail — see § PPS fields)
- [`crates/mediaway-decoder/adr/vulkan/0001-vulkan-video-decode.md`](../vulkan/0001-vulkan-video-decode.md)
  — 2026-07-30/2026-08-05 addenda: HEVC IDR decode hardware-verified, P/B-slice HEVC decode an
  explicit, unchanged follow-up — the finding this ADR's own § Context is built on
- `crates/mediaway-encoder/src/vulkan/hevc_gop.rs` — cited for the "single-slot `last_written`"
  design shape `linux/vaapi/hevc_dpb.rs`'s `HevcDpb` structurally mirrors
- [`crates/mediaway-encoder/adr/linux/0003-vaapi-hevc-p-frame-gop.md`](../../../mediaway-encoder/adr/linux/0003-vaapi-hevc-p-frame-gop.md)
  — same-session encode-side sibling; this ADR's own design is built to decode exactly that ADR's
  own expected output shape (§ Slice-header parser extension, § Test plan's cross-crate round-trip)
- `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cros-libva-0.0.13\src\buffer\hevc.rs`
  — real vendored `cros-libva` 0.0.13 source read directly for every `PictureHEVC`/
  `PictureParameterBufferHEVC`/`SliceParameterBufferHEVC`/`HevcPicFields`/`HevcSliceParsingFields`
  signature cited above
- FFmpeg `libavcodec/vaapi_hevc.c` (`fill_vaapi_pic`, `fill_vaapi_reference_frames`,
  `vaapi_hevc_decode_slice`) and `libavcodec/hevc/hevcdec.c` (`hls_slice_header`) — both fetched
  and read directly this session; real, sourced oracle for RPS-flag construction, `RefPicList`
  index resolution, and the full slice-header syntax sequence cited in § Slice-header parser
  extension
- [Intel `libva` `va_dec_hevc.h`](https://github.com/intel/libva/blob/master/va/va_dec_hevc.h) /
  [`va.h`](https://github.com/intel/libva/blob/master/va/va.h) — fetched this session;
  `RefPicList` index-array semantics and `VAProfileHEVCMain = 17` confirmed directly;
  `VA_PICTURE_HEVC_*` numeric values **not** found in the fetched excerpt (§ Open questions #1)
- [Intel libva HEVC decoding API docs](https://intel.github.io/libva/group__api__dec__hevc.html)
  — searched this session; `VA_PICTURE_HEVC_RPS_ST_CURR_BEFORE`/`_INVALID` semantics (mutual
  exclusivity, fixed-size-array initialization convention) confirmed by name/description, not by
  numeric value
- [`docs/ai/wiki/platform/linux-decode.md`](../../../../docs/ai/wiki/platform/linux-decode.md) ·
  [`docs/ai/wiki/platform/vulkan-decode.md`](../../../../docs/ai/wiki/platform/vulkan-decode.md)
- [`docs/spec/sans-io.md`](../../../../docs/spec/sans-io.md) ·
  [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md) ·
  [`docs/conventions/error-handling.md`](../../../../docs/conventions/error-handling.md)

ADRs are **English**. Numbering is local to this `adr/` folder.
