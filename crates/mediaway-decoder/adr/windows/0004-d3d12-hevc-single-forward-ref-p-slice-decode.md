# ADR-0004: D3D12 native HEVC decode — single-forward-reference P-slice, Main profile

- **Status**: Proposed
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder-windows`

## ⚠️ Read this before touching any hardware

This ADR is a **design-only pass — no implementation code is written here**, per the
originating task's explicit instruction. Beyond that:

1. **`mediaway-decoder-windows`'s existing D3D12 H.264 decode path has never once produced
   a correct decode on real hardware and has caused 8 confirmed `DXGI_ERROR_DEVICE_HUNG`
   GPU hangs (TDRs) across sessions**, most recently 2026-08-07 (ADR-0002's own addenda,
   read in full before implementing anything here — this is not a secondary detail, it is
   the single most important fact about this backend). Root cause remains **unresolved**.
   Every low-level D3D12 mechanism this HEVC ADR reuses unchanged — `setup.rs`'s decoder/
   heap/DPB-texture-array creation, `ops.rs`'s `DecodeFrame1` submission/barrier sequence
   shape (a **new, parallel** `hevc_ops.rs` copies this exact shape, retyped — see
   § Decision) — is exactly the machinery that is currently unproven for H.264. **This ADR
   does not resolve that hang and does not depend on it being resolved to be designed**, but
   any implementer must treat "the shared D3D12 submission plumbing might itself be broken,
   independent of any HEVC-specific bug" as a live, real hypothesis for whatever HEVC
   symptom shows up first.
2. **Do not run the existing H.264 D3D12 decode hardware test** (`d3d12_video_decode_tests.rs`)
   as any part of implementing or verifying this ADR. It is a real, disruptive GPU-hang risk
   on this workspace's dev machine, not a free verification step — this session's own
   accumulated finding, re-confirmed by every addendum in ADR-0002.
3. **This ADR's own future implementation must not run its new HEVC hardware-gated
   integration test either** (see § Test plan) — the same TDR risk applies, compounded by
   the fact that HEVC's GPU submission path is genuinely new, untested code. Compile,
   `clippy`, and sans-io unit tests only. Verification here means "does this design's static
   shape and every hand-defined `repr(C)` struct layout hold up against independent,
   citable sources" — not "does it decode a real frame."

## Context

`mediaway-decoder-windows`'s `d3d12_video_decode` module (`src/windows/d3d12_video_decode.rs`
+ its sibling files) is **H.264-only today**: `D3d12VideoDecoder::open` hard-rejects any
`config.codec != CodecKind::H264` (`d3d12_video_decode.rs:213`), and no HEVC-specific file
exists anywhere under `src/windows/d3d12_video_decode/`. The module is self-contained and
**unregistered** (`mod d3d12_video_decode;`, not `pub mod`, per `src/lib.rs` — unchanged by
this ADR).

ADR-0002 (the module's founding ADR) always scoped HEVC as an explicit follow-up ("H.264
first, append an Addendum... then HEVC, then AV1 — same file, growing status") and its own
§ Scope already named the intended HEVC cuts this ADR inherits: **single-tile, no WPP**
(`tiles_enabled_flag`/`entropy_coding_sync_enabled_flag` → `Unsupported`), Main profile only,
sliding-window-equivalent DPB management (no long-term references). This ADR narrows ADR-0002's
original "general GOP, all three codecs" ambition for HEVC specifically to **single-forward-
reference P-slice** (see § Scope decision) — a deliberate, smaller cut than ADR-0002's own
stated intent, justified below.

### Correcting the task's premise: no VA-API HEVC decoder exists in this repository today

The task that produced this ADR described a "newer, more complete VA-API HEVC decode
(single-forward-reference P-slice, DPB)" at `crates/mediaway-decoder/src/linux/vaapi/hevc*.rs`
with an ADR at `crates/mediaway-decoder/adr/linux/0003-vaapi-hevc-p-slice-dpb.md`. **Checked
directly this session — neither exists on `main` (clean working tree, `git log -1` =
`96147d6`).** `src/linux/vaapi/mod.rs` only declares `codec`, `dpb`, `h264`, `nv12`, `pps`,
`slice`, `sps` — no `hevc` module; `src/linux/vaapi/{h264_tests.rs,codec_tests.rs}` only
reference `CodecKind::Hevc` in `assert!(!is_supported_video_codec(CodecKind::Hevc))`-style
negative-support assertions. `crates/mediaway-decoder/adr/linux/` has only `0001` (H.264
IDR-only VA-API) and `0002` (**H.264** single-forward-reference P-slice DPB, not HEVC —
title: "VA-API H.264 single-forward-reference P-slice decode"). This appears to be a
cross-referenced memory of work that either happened on an unmerged branch or was conflated
with the H.264 VA-API ADR by title similarity; it is **not present to read from**.

This does not block the task — `mediaway-decoder-windows` (this crate) already has a real,
**hardware-verified-for-IDR** HEVC decode reference in the same workspace:
`crates/mediaway-decoder/src/vulkan/{decoder_hevc.rs,hevc_params.rs,hevc_params/hevc_ptl.rs,hevc_slice.rs,session_command_hevc.rs}`
(see `adr/vulkan/0001-vulkan-video-decode.md`'s HEVC addenda, 2026-07-30 and 2026-08-05).
This ADR uses that Vulkan HEVC decoder as its primary porting source instead, per
§ What's portable vs. new below. One load-bearing consequence: **the Vulkan HEVC decoder's
own P-slice/RPS parsing (`vulkan::hevc_slice::ShortTermRefPicSet`) is real and unit-tested,
but has never been wired to a real `vkCmdDecodeVideoKHR` call — `decoder_hevc.rs::decode_slice_hevc`
only reaches a real GPU decode call for IDR pictures.** So unlike H.264 (where a general-GOP
GPU decode attempt exists and hangs) there is **no prior GPU-verified P-slice HEVC decode
anywhere in this workspace, on any backend, on any OS** — this ADR's P-slice scope is
genuinely unexplored territory for HEVC, not a known-working design being ported to a new
API. See § Open questions #1 for the resulting recommendation.

### `windows` crate binding survey (real compile-adjacent check, not inference)

Checked the vendored `windows-0.62.2` source directly (same crate version ADR-0002's H.264
addendum confirmed, `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\windows-0.62.2`):

- `D3D12_VIDEO_DECODE_PROFILE_HEVC_MAIN` (and `_MAIN10`/`_MAIN10_422`/`_MAIN10_444`/
  `_MAIN10_EXT`/`_MAIN12`/`_MAIN12_422`/`_MAIN12_444`/`_MAIN16`/`_MAIN_444`/`_MONOCHROME`/
  `_MONOCHROME10`) GUID constants **are present**
  (`Win32/Media/MediaFoundation/mod.rs:3780-3791`) — same pattern as H.264's profile GUID,
  confirms this ADR's `D3D12_VIDEO_DECODE_PROFILE_HEVC_MAIN` usage in `hevc.rs` will compile.
- **`DXVA_PicParams_HEVC`, `DXVA_Slice_HEVC_Short`, `DXVA_Qmatrix_HEVC`, `DXVA_PicEntry_HEVC`
  are absent from the crate's generated bindings entirely** — grepped the full vendored
  source tree for every one of these symbols, zero matches. Exactly the same situation
  ADR-0002's H.264 addendum found for `DXVA_PicParams_H264` et al.: the D3D12 decode
  *plumbing* (`ID3D12VideoDecoder`, `D3D12_VIDEO_DECODE_FRAME_ARGUMENT::pData` as
  `*mut c_void` + `Size`) is present and reused as-is; the DXVA-specification per-codec
  structs must be hand-defined by this crate, `repr(C)`, ground-truthed against real
  `dxva.h` layout — see § DXVA struct definitions below.

### Real, hardware-verified HEVC GOP encode already exists in this workspace (test-plan-relevant)

`mediaway-encoder-windows`'s native D3D12 HEVC encoder gained real GOP/P-frame support
(`src/windows/d3d12_video_encode/gop_hevc.rs` — `HevcGopState`, mirroring the sibling
`gop.rs`'s H.264 `H264GopState`), hardware-verified per this session's own project memory
(2026-08-06). This is directly useful for § Test plan: exactly the same "chain this
workspace's own hardware-verified encoder into this crate's decoder to get a real,
driver-produced bitstream instead of hand-written CABAC" technique
`tests/vulkan/hardware_hevc_decode.rs` already used for Vulkan (HEVC has no CAVLC/`I_PCM`-style
escape — even a PCM coding unit's own `pcm_flag` is CABAC-coded, ITU-T H.265 § 9.3 — so
hand-constructing a legal HEVC bitstream by hand, the way the H.264 D3D12 decode test does
with `I_PCM`/`P_Skip` macroblocks, is not a realistic option for HEVC).

## Scope decision: single-forward-reference P-slice, not IDR-only, not general GOP

> **`RefPicList0[0]` only** (`num_ref_idx_l0_active` fixed at exactly `1`, combined count of
> `used_by_curr_pic` entries across `short_term_ref_pic_set(0)`'s S0 + S1 sets must equal
> exactly `1`), **I/IDR pictures**, **no B-slices**, **no long-term references**, **no
> `ref_pic_list_modification`/`lists_modification_present_flag`**, **Main profile, 8-bit
> 4:2:0, NV12 output, single-tile, no WPP**.

This deliberately mirrors the *shape* of the (nonexistent-in-this-repo, see above)
"VA-API HEVC single-forward-reference P-slice" scope the task described — but the closest
**real** precedent for that exact scope-name in this workspace is
`crates/mediaway-decoder/adr/linux/0002-vaapi-h264-p-slice-dpb.md`, which defines it
precisely for H.264: *"Extend `mediaway-decoder::linux::vaapi` to decode
single-forward-reference P-slices (`RefPicList0[0]` only, `num_ref_idx_l0_active` fixed at
exactly `1`)... `ref_pic_list_modification()` reordering rejected — no real
single-forward-reference stream needs reordering (there is only one candidate to reorder)"*.
This ADR applies the same shape to HEVC's own reference model (short-term RPS instead of
`frame_num` sliding window).

**Why this scope, not ADR-0002's original "general GOP from the start" ambition for HEVC:**

1. **No GPU-verified P-slice HEVC decode precedent exists anywhere in this workspace** (see
   § Correcting the task's premise) — B-slice/multi-reference reference-list construction
   (`RefPicSetStCurrBefore`/`RefPicSetStCurrAfter` both populated with multiple entries,
   weighted (bi)prediction) is real, additional untested surface on top of an already-unproven
   base. Single-forward-reference is the smallest non-trivial (i.e., not IDR-only) HEVC
   decode surface, matching how `mediaway-decoder-linux` staged the *same* cut for H.264
   before attempting anything larger.
2. **The shared D3D12 submission plumbing this scope reuses is itself unproven for H.264**
   (§ safety banner). Minimizing the *number of new, simultaneously-unverified variables*
   (RPS correctness, POC correctness, DXVA-HEVC-struct-field correctness, *and*
   general-GOP reference-list construction all at once) is a real risk-reduction move, not
   just a smaller-scope-for-its-own-sake choice.
3. **HEVC's DXVA slice-control struct (`DXVA_Slice_HEVC_Short`) carries none of the
   per-slice reference-list/weighted-prediction detail H.264's `DXVA_Slice_H264_Long` does**
   (see § DXVA struct definitions) — the accelerator re-parses the *entire* slice-segment
   header itself from raw bytes. This means single-forward-reference vs. general-GOP is
   **not** primarily a "how much do we pack into the slice-control struct" question the way
   it was for H.264; it is almost entirely a "do we compute `RefPicSetStCurrBefore`/`After`/
   `LtCurr` and `RefPicList[15]` correctly, and reject anything else" question — a smaller,
   more contained increment than H.264's own P/B general-GOP work.

Explicitly **out of scope, rejected as `DecodeError::Unsupported`, not silently mishandled**:
B-slices; `num_ref_idx_l0_active != 1` (whether via `num_ref_idx_active_override_flag` or a
PPS default); long-term references (`long_term_ref_pics_present_flag`); SPS-level RPS lists
(`short_term_ref_pic_set_sps_flag == 1`, matching the Vulkan source's own cut);
`tiles_enabled_flag`/`entropy_coding_sync_enabled_flag`; `pcm_enabled_flag`;
`scaling_list_enabled_flag == 1` (non-flat scaling lists — see § DXVA struct definitions);
`deblocking_filter_control_present_flag == 1` (matches the Vulkan source's own cut, for the
same reason: this crate does not build the sub-fields its presence requires);
`separate_colour_plane_flag`/non-4:2:0 `chroma_format_idc`; multi-slice pictures
(`first_slice_segment_in_pic_flag == 0`); 10/12-bit profiles (blocked on the same
`mediaway_common::PixelFormat` 10-bit gap ADR-vulkan-0001 already flagged — `Nv12`/`I420`/
`Bgra8`/`Rgba8`/`Yuyv` only).

## Decision

> Add HEVC support to `mediaway-decoder-windows`'s D3D12 native decode module as **new files
> only**, reusing `dpb.rs`/`setup.rs`/`util.rs` **unchanged** (both are already codec-generic
> by design — confirmed by direct read this session, see § Reuse below) and touching
> `d3d12_video_decode.rs` (the existing H.264-bearing top-level file) with **exactly one
> additive change**: new `mod` declarations + a re-export for the new HEVC types. Zero edits
> to any existing H.264 type, function body, or struct layout.

### Reuse vs. new: what's actually shared

Checked `dpb.rs`/`setup.rs` directly this session (not assumed from ADR-0002's plan text):

- **`dpb.rs`'s `SlotTable<M>`/`DpbPool<M>` are already generic over the per-codec reference
  metadata type `M: Copy`** (`dpb.rs:28,55,176`) — its own module doc states this explicitly:
  *"Generic over `M`, the per-codec reference metadata (H.264's `H264RefMeta` today; HEVC/AV1
  metadata later), so this file needs no change when those codecs land."* Confirmed true by
  reading the file: zero H.264-specific types appear anywhere in `dpb.rs`. **Reused as-is**
  for a new `HevcRefMeta` (see `hevc_refs.rs`) — this is the largest real reuse win this ADR
  has, and it costs nothing (no new file, no edit).
- **`setup.rs`'s `check_decode_support`/`create_decoder`/`create_command_objects`/
  `create_dpb_texture_array`/`create_linear_buffer`/`device_from_handle` are already
  codec-generic** — every function takes a profile `GUID` / `DXGI_FORMAT` parameter rather
  than hardcoding H.264's; its own module doc says as much (`setup.rs:1-5`). **Reused as-is**;
  `hevc.rs` (new) is a thin ~40-line sliver calling these with
  `D3D12_VIDEO_DECODE_PROFILE_HEVC_MAIN`/`DXGI_FORMAT_NV12`, mirroring `h264.rs` field-for-field.
- **`util.rs`** (fence wait, resource-state transitions, `data_size`, `nv12_size`,
  `align_up_u32`) — codec-agnostic by construction, no HEVC-specific need identified. Reused
  as-is.
- **`ops.rs` is *not* reused as-is** — `Session::decode_frame`/`build_reference_frame_arrays`
  are concretely typed to `DxvaPicParamsH264`/`DxvaQmatrixH264`/`DxvaSliceH264Long`/
  `H264RefMeta` (`ops.rs:80-119`), and the top-level `Session`/`D3d12VideoDecoder` structs
  in `d3d12_video_decode.rs` are likewise concretely H.264-typed
  (`dpb: DpbPool<H264RefMeta>`, `d3d12_video_decode.rs:145`). See § Why a parallel
  `hevc_ops.rs`/`hevc_decoder.rs`, not a generified `Session<M>`, for why this ADR does
  **not** propose making these generic this pass.

### Why a parallel `hevc_ops.rs`/`hevc_decoder.rs`, not a generified `Session<M>`

A `Session<M: Copy>` generic over ref-metadata (and further generic over the three DXVA
argument types via `decode_frame<P, Q, S>(...)`) would be a real, zero-runtime-cost (pure
monomorphization) way to make `ops.rs` genuinely codec-generic, closing the gap between its
module doc's aspirational framing and its actual H.264-only typing today. **Considered, not
adopted this pass**, for one reason that follows directly from the safety banner: it requires
editing `ops.rs` and `d3d12_video_decode.rs`'s existing `Session`/`D3d12VideoDecoder`
definitions — the same files whose current, unmodified-since-2026-08-07 state is the last
known-consistent baseline for the still-unresolved H.264 hang. Touching shared code before
either codec's GPU path is verified means a future HEVC-side bug and the pre-existing H.264
bug become harder to reason about independently (a regression in the generified shared path
could plausibly manifest as "a new HEVC bug" or "a changed H.264 symptom," with no clean way
to attribute it). Per this workspace's own "each real hardware attempt has a real cost, don't
add confounding variables" judgment call (ADR-0002's addenda), this ADR chooses the
**additive-only** design instead, at the cost of real, acknowledged duplication
(`hevc_ops.rs::decode_frame_hevc`/`readback_dpb_slot_to_cpu_hevc` will be near-line-for-line
copies of `ops.rs::decode_frame`/`readback_dpb_slot_to_cpu`, retyped). **Recommended
follow-up, not this ADR**: once *both* H.264 and HEVC GPU submission paths are independently
hardware-verified (or the H.264 hang is root-caused), a small refactor ADR should generify
`Session<M>`/`ops.rs` to remove this duplication — deferred, not forgotten, see
§ Open questions #7.

### File layout plan (design only — no file below exists yet)

```text
src/windows/d3d12_video_decode.rs   # EXISTING FILE — only change: add
                                     #   mod hevc; mod hevc_vps_sps_pps; mod hevc_slice;
                                     #   mod hevc_poc; mod hevc_refs; mod hevc_pic_params;
                                     #   mod hevc_ops; mod hevc_decoder;
                                     # + pub(crate) use hevc_decoder::{
                                     #     D3d12VideoDecoderHevc, DecodedFrameHevc, DecodedOutputHevc};
                                     # Zero edits to any existing H.264 type/fn/struct.

src/windows/d3d12_video_decode/
  hevc.rs               # NEW — open-time feature query + decoder/heap creation for HEVC.
                         # Mirrors h264.rs field-for-field: calls
                         # setup::check_decode_support/setup::create_decoder with
                         # D3D12_VIDEO_DECODE_PROFILE_HEVC_MAIN / DXGI_FORMAT_NV12.
                         # ~40 lines, no new logic.

  hevc_vps_sps_pps.rs    # NEW — VPS/SPS/PPS parsing. New local 2-byte HEVC NAL header
                         # parse (HEVC's nal_unit_type(6)/nuh_layer_id(6)/
                         # nuh_temporal_id_plus1(3) is NOT h264::NalUnit::parse's 1-byte
                         # header — same finding vulkan/hevc_params.rs already made).
                         # Reuses mediaway_sw::h264::{BitReader, split_annex_b}
                         # (codec-agnostic, already the pattern h264_sps_pps.rs and
                         # vulkan/hevc_params.rs both use). Ports the *shape* of
                         # crate::vulkan::hevc_params::{HevcSps, HevcPps} field-by-field
                         # parsing (same ITU-T H.265 syntax, sans-io, no Vulkan-specific
                         # type touched) into this module's own local Sps/Pps structs —
                         # same "port the shape, not the code" relationship h264_sps_pps.rs
                         # already has to mediaway_sw::h264::{Sps,Pps} (ADR-0002 § Scope).
                         # Real, load-bearing carry-forward: every *Flags bitfield the
                         # Vulkan HEVC decoder's own hardware-verification found itself
                         # missing across 3 rounds (amp_enabled_flag,
                         # sample_adaptive_offset_enabled_flag, sps_temporal_mvp_enabled_flag,
                         # strong_intra_smoothing_enabled_flag,
                         # transquant_bypass_enabled_flag, cu_qp_delta_enabled_flag,
                         # transform_skip_enabled_flag,
                         # pps_slice_chroma_qp_offsets_present_flag,
                         # pps_loop_filter_across_slices_enabled_flag, real
                         # ProfileTierLevel/level_idc-ordinal conversion) must all be
                         # parsed and threaded into DXVA_PicParams_HEVC's own,
                         # differently-shaped flag words from day one — not rediscovered
                         # bug-by-bug against a real driver the way Vulkan's HEVC path had
                         # to (see § Open questions #1 for why that re-discovery loop is
                         # exactly what this ADR wants to avoid repeating on a new API).

  hevc_slice.rs          # NEW — slice-segment-header + short-term RPS parsing. Ports
                         # crate::vulkan::hevc_slice::{ShortTermRefPicSet,
                         # HevcSliceSegmentHeader} near-verbatim — pure ITU-T H.265
                         # § 7.3.6.1/§ 7.4.8 bitstream logic, zero Vulkan-specific types
                         # touched, the single largest real code-reuse opportunity this
                         # ADR has. Adds this ADR's own P-slice restriction (reject unless
                         # the combined `used_by_curr_pic` count across s0+s1 is exactly
                         # `1`) — a check the Vulkan source has no reason to make, since it
                         # never wires P-slices to a real decode call at all.

  hevc_poc.rs            # NEW, genuinely no existing source anywhere in this workspace
                         # (checked: crate::vulkan::decoder_hevc.rs only has
                         # "IDR: PicOrderCntVal is always 0" — no MSB-cycle tracking
                         # exists for any HEVC backend in this workspace today). POC
                         # computation per ITU-T H.265 § 8.3.1 (PicOrderCntMsb via
                         # prevTid0Pic's stored POC/lsb + pic_order_cnt_lsb, simpler than
                         # H.264's type-0/1/2 branching — one formula). Structurally
                         # mirrors h264_poc.rs's PocState shape (persistent prev-state
                         # struct + compute() returning (Poc, Self)) but the algorithm
                         # itself is new, from-scratch work.

  hevc_refs.rs           # NEW — no port available (Vulkan's decoder_hevc.rs never reaches
                         # this problem, being IDR-only end-to-end). Maps
                         # ShortTermRefPicSet::curr_before_after_poc's resulting POC
                         # values to actual occupied DPB slots (via each slot's stored
                         # HevcRefMeta.poc), builds RefPicList[15]/RefPicSetStCurrBefore[8]/
                         # RefPicSetStCurrAfter[8]/RefPicSetLtCurr[8] per § DXVA struct
                         # definitions (RefPicSetLtCurr always empty, this ADR's scope has
                         # no long-term refs), and enforces + returns
                         # DecodeError::Unsupported for the single-forward-reference scope
                         # cut (combined before+after count != 1). DPB eviction here is
                         # "not present in the current picture's own parsed RPS" (ITU-T
                         # H.265's RPS-application/"bumping" model, § 8.3.2) — structurally
                         # different from h264_refs.rs's FrameNumWrap sliding window, not a
                         # port of it.

  hevc_pic_params.rs     # NEW — hand-defined repr(C) DxvaPicEntryHevc/DxvaQmatrixHevc/
                         # DxvaSliceHevcShort/DxvaPicParamsHevc (§ DXVA struct definitions
                         # below) + build_pic_params_hevc/build_slice_short/
                         # flat_qmatrix_hevc. Mirrors h264_pic_params.rs's shape and its
                         # hand-defined-struct precedent (same absent-from-`windows`-crate
                         # situation, confirmed this session) but every struct/field is
                         # HEVC's own — no field is shared with h264_pic_params.rs.

  hevc_ops.rs            # NEW, parallel to ops.rs (see § Why a parallel hevc_ops.rs
                         # above) — decode_frame_hevc (same write_bitstream /
                         # build_reference_frame_arrays / barrier / DecodeFrame1 / barrier
                         # sequence shape as ops::decode_frame, retyped for
                         # DxvaPicParamsHevc/DxvaQmatrixHevc/DxvaSliceHevcShort/HevcRefMeta)
                         # + readback_dpb_slot_to_cpu_hevc (near-byte-identical copy of
                         # ops::readback_dpb_slot_to_cpu, which already has zero
                         # H.264-specific types in its signature — flagged in § Open
                         # questions #7 as the easiest first step of the future
                         # de-duplication refactor).

  hevc_decoder.rs        # NEW — SessionHevc (mirrors Session: same D3D12 object fields,
                         # dpb: DpbPool<HevcRefMeta>) + D3d12VideoDecoderHevc (mirrors
                         # D3d12VideoDecoder: open/ensure_session_ready/push_packet/
                         # decode_slice/poll_frame/flush/release_output) +
                         # DecodedFrameHevc/DecodedOutputHevc (same shape as
                         # DecodedFrame/DecodedOutput — kept as separate small types
                         # rather than shared, not worth the coupling for two ~15-line
                         # structs).

  hevc_vps_sps_pps_tests.rs / hevc_slice_tests.rs / hevc_poc_tests.rs /
  hevc_refs_tests.rs / hevc_pic_params_tests.rs   # sibling *_tests.rs per this
                         # workspace's convention — all pure sans-io, fully writable and
                         # runnable this pass without any hardware (see § Test plan).

src/windows/d3d12_video_decode_hevc_tests.rs   # NEW top-level hardware-gated integration
                         # test, mirroring d3d12_video_decode_tests.rs's existing
                         # h264_decode_idr_and_p_frame_or_skip pattern. Written this pass;
                         # MUST NOT be run by whoever implements this ADR — see § Test plan.
```

Every new file is planned to stay under this workspace's 1000-line-per-source rule;
`hevc_vps_sps_pps.rs` is the one most likely to need a `hevc_ptl.rs`-style submodule split
(mirroring `vulkan::hevc_params/hevc_ptl.rs`'s own split, done for exactly this reason —
ProfileTierLevel parsing is large) if implementation finds it necessary.

## DXVA struct definitions (ground truth, cited)

Fetched directly this session (not transcribed from memory), same Wine-header-mirror
ground-truthing convention ADR-0002's H.264 addendum used:

```c
/* wine-mirror/wine, include/dxva.h — fetched this session */
typedef struct _DXVA_PicEntry_HEVC {
    union {
        struct {
            UCHAR Index7Bits : 7;
            UCHAR AssociatedFlag : 1;
        };
        UCHAR bPicEntry;
    };
} DXVA_PicEntry_HEVC, *LPDXVA_PicEntry_HEVC;

typedef struct _DXVA_Slice_HEVC_Short {
    UINT    BSNALunitDataLocation;
    UINT    SliceBytesInBuffer;
    USHORT  wBadSliceChopping;
} DXVA_Slice_HEVC_Short, *LPDXVA_Slice_HEVC_Short;

typedef struct _DXVA_Qmatrix_HEVC {
    UCHAR ucScalingLists0[6][16];
    UCHAR ucScalingLists1[6][64];
    UCHAR ucScalingLists2[6][64];
    UCHAR ucScalingLists3[2][64];
    UCHAR ucScalingListDCCoefSizeID2[6];
    UCHAR ucScalingListDCCoefSizeID3[2];
} DXVA_Qmatrix_HEVC, *LPDXVA_Qmatrix_HEVC;
```

`DXVA_PicParams_HEVC` (Wine mirror; the two coding-flags fields are **separate, sibling**
unions — see caveat below):

```c
typedef struct _DXVA_PicParams_HEVC {
    USHORT PicWidthInMinCbsY;
    USHORT PicHeightInMinCbsY;
    union { struct {
        USHORT chroma_format_idc : 2;
        USHORT separate_colour_plane_flag : 1;
        USHORT bit_depth_luma_minus8 : 3;
        USHORT bit_depth_chroma_minus8 : 3;
        USHORT log2_max_pic_order_cnt_lsb_minus4 : 4;
        USHORT NoPicReorderingFlag : 1;
        USHORT NoBiPredFlag : 1;
        USHORT ReservedBits1 : 1;
    }; USHORT wFormatAndSequenceInfoFlags; };
    DXVA_PicEntry_HEVC CurrPic;
    UCHAR  sps_max_dec_pic_buffering_minus1;
    UCHAR  log2_min_luma_coding_block_size_minus3;
    UCHAR  log2_diff_max_min_luma_coding_block_size;
    UCHAR  log2_min_transform_block_size_minus2;
    UCHAR  log2_diff_max_min_transform_block_size;
    UCHAR  max_transform_hierarchy_depth_inter;
    UCHAR  max_transform_hierarchy_depth_intra;
    UCHAR  num_short_term_ref_pic_sets;
    UCHAR  num_long_term_ref_pics_sps;
    UCHAR  num_ref_idx_l0_default_active_minus1;
    UCHAR  num_ref_idx_l1_default_active_minus1;
    CHAR   init_qp_minus26;
    UCHAR  ucNumDeltaPocsOfRefRpsIdx;
    USHORT wNumBitsForShortTermRPSInSlice;
    USHORT ReservedBits2;
    union { struct {
        UINT32 scaling_list_enabled_flag : 1;
        UINT32 amp_enabled_flag : 1;
        UINT32 sample_adaptive_offset_enabled_flag : 1;
        UINT32 pcm_enabled_flag : 1;
        UINT32 pcm_sample_bit_depth_luma_minus1 : 4;
        UINT32 pcm_sample_bit_depth_chroma_minus1 : 4;
        UINT32 log2_min_pcm_luma_coding_block_size_minus3 : 2;
        UINT32 log2_diff_max_min_pcm_luma_coding_block_size : 2;
        UINT32 pcm_loop_filter_disabled_flag : 1;
        UINT32 long_term_ref_pics_present_flag : 1;
        UINT32 sps_temporal_mvp_enabled_flag : 1;
        UINT32 strong_intra_smoothing_enabled_flag : 1;
        UINT32 dependent_slice_segments_enabled_flag : 1;
        UINT32 output_flag_present_flag : 1;
        UINT32 num_extra_slice_header_bits : 3;
        UINT32 sign_data_hiding_enabled_flag : 1;
        UINT32 cabac_init_present_flag : 1;
        UINT32 ReservedBits3 : 5;
    }; UINT32 dwCodingParamToolFlags; };
    union { struct {
        UINT32 constrained_intra_pred_flag : 1;
        UINT32 transform_skip_enabled_flag : 1;
        UINT32 cu_qp_delta_enabled_flag : 1;
        UINT32 pps_slice_chroma_qp_offsets_present_flag : 1;
        UINT32 weighted_pred_flag : 1;
        UINT32 weighted_bipred_flag : 1;
        UINT32 transquant_bypass_enabled_flag : 1;
        UINT32 tiles_enabled_flag : 1;
        UINT32 entropy_coding_sync_enabled_flag : 1;
        UINT32 uniform_spacing_flag : 1;
        UINT32 loop_filter_across_tiles_enabled_flag : 1;
        UINT32 pps_loop_filter_across_slices_enabled_flag : 1;
        UINT32 deblocking_filter_override_enabled_flag : 1;
        UINT32 pps_deblocking_filter_disabled_flag : 1;
        UINT32 lists_modification_present_flag : 1;
        UINT32 slice_segment_header_extension_present_flag : 1;
        UINT32 IrapPicFlag : 1;
        UINT32 IdrPicFlag : 1;
        UINT32 IntraPicFlag : 1;
        UINT32 ReservedBits4 : 13;
    }; UINT32 dwCodingSettingPicturePropertyFlags; };
    CHAR   pps_cb_qp_offset;
    CHAR   pps_cr_qp_offset;
    UCHAR  num_tile_columns_minus1;
    UCHAR  num_tile_rows_minus1;
    USHORT column_width_minus1[19];
    USHORT row_height_minus1[21];
    UCHAR  diff_cu_qp_delta_depth;
    CHAR   pps_beta_offset_div2;
    CHAR   pps_tc_offset_div2;
    UCHAR  log2_parallel_merge_level_minus2;
    INT    CurrPicOrderCntVal;
    DXVA_PicEntry_HEVC RefPicList[15];
    UCHAR  ReservedBits5;
    INT    PicOrderCntValList[15];
    UCHAR  RefPicSetStCurrBefore[8];
    UCHAR  RefPicSetStCurrAfter[8];
    UCHAR  RefPicSetLtCurr[8];
    USHORT ReservedBits6;
    USHORT ReservedBits7;
    UINT   StatusReportFeedbackNumber;
} DXVA_PicParams_HEVC, *LPDXVA_PicParams_HEVC;
```

**Real discrepancy found and flagged, not silently picked one source over the other**:
Microsoft Learn's own rendered page for `DXVA_PicParams_HEVC` (`learn.microsoft.com/.../
dxva-picparams-hevc`, fetched this session) shows the **second** coding-flags union nested
*inside* the first one, and every subsequent field (`pps_cb_qp_offset` through
`StatusReportFeedbackNumber`) nested inside that same outer union too — which cannot be the
real, compilable layout (a real decoder needs `pps_cb_qp_offset` and
`dwCodingParamToolFlags` simultaneously, not as mutually-exclusive union alternatives). This
is almost certainly a markdown/HTML table-conversion artifact of Microsoft's docs pipeline
losing closing braces, not the real struct — the Wine mirror (a real, independently
compilable C header tracking the actual Windows SDK byte-for-byte, the same source ADR-0002
trusted for H.264) is treated as authoritative here, with the two coding-flags unions as
**separate, sequential** struct members followed by plain fields, exactly as reproduced
above. **Not independently cross-checked against a third source this session** (e.g. FFmpeg's
`libavcodec/dxva2_hevc.c` struct-fill order, which would confirm field *order* even without
seeing the header) — flagged as § Open questions #2, a real "verify before implementing"
item, not asserted as settled.

## `RefPicList`/`RefPicSetStCurrBefore`/`After`/`LtCurr` semantics — not independently confirmed this session

Believed (general DXVA-HEVC-decoder-implementation knowledge, consistent with how every
other DXVA struct in this family works, e.g. `used_for_reference_flags` in H.264's own
struct): `RefPicSetStCurrBefore[8]`/`RefPicSetStCurrAfter[8]`/`RefPicSetLtCurr[8]` are
**byte indices into `RefPicList[15]`** (i.e., values `0..15`, `0xFF` for an unused/short
entry), not raw DPB slot numbers directly — `RefPicList[15]` itself holds the
`DXVA_PicEntry_HEVC` (DPB-slot-indexed) entries. **This was not independently fetched/
confirmed from a primary source this session** (no `dxva2_hevc.c` fetch performed, unlike
ADR-0002's H.264 addendum which did fetch and diff against FFmpeg's real
`dxva2_h264.c`/GStreamer sources field-by-field). Flagged prominently as § Open questions #3
— **implementation must confirm this against FFmpeg's real `libavcodec/dxva2_hevc.c`
(`fill_picture_parameters`) before writing `hevc_refs.rs`'s array-building logic**, exactly
the ground-truthing rigor ADR-0002's own addenda demonstrate is necessary (two of ADR-0002's
five real bugs were exactly this class of "plausible-looking field semantics, wrong without
a real cross-check").

## Alternatives Considered

| Alternative | Why not |
|---|---|
| IDR-only scope (mirror the original Vulkan HEVC decode's own current GPU-wired scope) | Rejected per the task's explicit instruction to mirror the (described, though not actually present in this repo) single-forward-reference P-slice scope; also a strictly smaller, less useful increment than what's designed here, and this ADR's design cost for P-slice support (RPS→DPB-slot mapping, POC MSB-cycle tracking) is not materially larger than IDR-only once undertaken. |
| Full general-GOP (B-slices, multi-reference, matching ADR-0002's original "all three codecs, general GOP from the start" ambition) | Rejected for this pass — real, additional untested surface (bi-prediction reference-list construction, weighted bi-prediction) stacked on top of an unproven base (§ safety banner) and no B-slice GPU-decode precedent anywhere in this workspace for any codec on any backend. Deferred, not designed here. |
| Generify `Session<M>`/`ops.rs` now, share one implementation across H.264 and HEVC | Considered (§ Why a parallel hevc_ops.rs) — rejected this pass specifically because it requires editing the existing H.264-bearing files whose current state is the last known-consistent baseline for an unresolved real hardware hang; deferred to a follow-up once at least one codec's GPU path is verified. |
| Parse but silently ignore non-flat HEVC scaling lists (mirror H.264's own `flat_qmatrix` fidelity-gap precedent exactly) | Considered — H.264's `h264_sps_pps.rs` parses custom scaling matrices only to keep bit-position in sync, then always hands the driver a flat matrix, documented as a fidelity gap. This ADR instead rejects `scaling_list_enabled_flag == 1` outright as `Unsupported` (§ Scope decision), a stricter, more honest cut for a first pass — HEVC's scaling-list syntax (`scaling_list_data()`, up to 32x32 lists with DC-coefficient fields) is materially more complex to parse-for-bit-sync-only than H.264's, and this ADR's scope is already narrower than H.264's general-GOP cut in every other dimension, so matching H.264's specific "silently downgrade fidelity" choice here was not judged worth the added parsing surface for this increment. Revisit once base decode is hardware-verified. |
| Extract `hevc_slice.rs`'s ported RPS logic into a shared, graphics-API-agnostic crate instead of duplicating it from `crate::vulkan::hevc_slice` | Deferred, same reasoning ADR-0002 and `adr/vulkan/0001` both already gave for HEVC/AV1 parser placement: no second real consumer existed when Vulkan's HEVC parser was written; now there technically would be two (Vulkan + this D3D12 module) in the *same* crate, which is a stronger case than before — but restructuring `crate::vulkan::hevc_slice` to be shared mid-ADR was judged out of scope for a design-only pass. Flagged as § Open questions #6, a real candidate for a follow-up once both are implemented and the actual code is proven to be identical enough to share. |

## Consequences

### Positive

- Real, hardware-verified-for-IDR HEVC bitstream-parsing knowledge (every `*Flags`
  bitfield the Vulkan HEVC decoder's own 3-round hardware debugging found itself missing)
  is carried forward into this design from day one, rather than needing to be rediscovered
  bug-by-bug against a new API's driver — a genuine, quantifiable risk reduction versus
  starting from `dxva.h` field names alone.
- `dpb.rs`/`setup.rs`/`util.rs` need **zero edits** — the largest, most load-bearing pieces
  of shared D3D12 infrastructure are reused exactly as designed, with their own module docs
  already anticipating this.
- `DXVA_Slice_HEVC_Short`'s much smaller field list (no `BitOffsetToSliceData` at all) makes
  an entire class of H.264's real, spec-ambiguity-driven bugs (ADR-0002's Bug 3 saga, fixed
  and then found "backwards" a session later) structurally impossible for HEVC — the
  accelerator re-parses the whole slice-segment header itself.
- The additive-only file layout (§ Decision) means implementing this ADR carries **zero risk
  of silently changing H.264 decode behavior** — every existing H.264 file, struct, and
  function stays byte-for-byte unchanged.

### Negative / Trade-offs

- This ADR's scope (single-forward-reference P-slice) is genuinely unexplored — no GPU
  decode of a non-IDR HEVC picture has ever been verified in this workspace, on any backend.
  There is no "this general shape is known to work, just on a new API" precedent the way
  H.264's D3D12 decode at least had (even though that H.264 attempt itself currently hangs).
- Real, acknowledged code duplication between `ops.rs`/`hevc_ops.rs` (§ Why a parallel
  hevc_ops.rs) — a deliberate, documented trade-off, not an oversight, with a named
  follow-up.
- `RefPicList`/`RefPicSetStCurrBefore`/`After`/`LtCurr` index semantics are not
  independently confirmed from a primary source this session (§ that section) — real risk
  that `hevc_refs.rs`'s first implementation gets this wrong, discoverable only by the
  cross-check this ADR explicitly defers to implementation time.
- Every open question from ADR-0002 that HEVC inherits unresolved (DPB-eviction backpressure
  error shape, whether H.264/HEVC/AV1 high-level-syntax parsing should become a shared crate,
  COM `.clone()` discipline) still applies here and is not re-litigated by this ADR.
- This design cannot be verified against real hardware as part of this task (§ safety
  banner) — every claim about DXVA-HEVC struct layout, RPS→DPB mapping correctness, and POC
  computation correctness is, at best, cross-checked against secondary/tertiary sources
  (Wine mirror, this workspace's own Vulkan HEVC hardware-verification history), not proven
  against a real driver.

## Open questions / risks

1. **Should HEVC GPU-path implementation even proceed before the existing H.264 D3D12 decode
   hang is root-caused?** Real risk, explicitly raised: if a future HEVC hardware attempt
   also hangs or produces zero/wrong output, there is currently no way to distinguish "a
   HEVC-specific bug" from "the shared `DecodeFrame1`/barrier/DPB-texture-array plumbing was
   never actually sound to begin with." Recommend treating H.264's hang resolution as a
   strong prerequisite signal (not a hard blocker — the sans-io parsing/design work here has
   independent value regardless) before spending any real hardware attempt on HEVC's own GPU
   path.
2. **`DXVA_PicParams_HEVC`'s exact union/field layout past the first coding-flags union is
   not independently confirmed against a third source** (§ DXVA struct definitions) — the
   Wine mirror and Microsoft Learn's own rendering visibly disagree in a way that strongly
   suggests a docs-rendering bug on Microsoft's side, but this was not cross-checked against
   FFmpeg's `libavcodec/dxva2_hevc.c` struct-fill order this session. First implementation-
   time task.
3. **`RefPicList`/`RefPicSetStCurrBefore`/`After`/`LtCurr` index semantics** (byte-indices-
   into-`RefPicList` vs. raw DPB slot numbers) — believed, not confirmed (§ that section).
   Must be resolved against `dxva2_hevc.c` before `hevc_refs.rs` is written for real.
4. **Exact DPB sizing formula**: this ADR proposes reusing H.264's shape
   (`sps_max_dec_pic_buffering_minus1 + 1 + CALLER_HEADROOM`) — not validated against any
   real HEVC stream's signaled value (mirrors ADR-0002's own Open Question #4, still
   unresolved for H.264 too).
5. **`num_short_term_ref_pic_sets`/`ucNumDeltaPocsOfRefRpsIdx`/`wNumBitsForShortTermRPSInSlice`**
   (`DXVA_PicParams_HEVC` fields whose exact semantics/required-correctness this ADR has not
   individually researched, beyond noting they exist) — flagged for implementation-time
   research, same rigor ADR-0002's H.264 addenda applied field-by-field.
6. **Should `hevc_slice.rs`'s ported RPS-parsing logic be extracted into a shared,
   graphics-API-agnostic module/crate** now that two real consumers (`crate::vulkan` and this
   new D3D12 module) would exist in the same crate? Not decided here (§ Alternatives
   Considered) — a real, concrete candidate for a small follow-up once this ADR's code is
   written and the actual duplication is visible to judge.
7. **Follow-up refactor: generify `Session<M>`/`ops.rs`** to remove the `ops.rs`/`hevc_ops.rs`
   duplication (§ Why a parallel hevc_ops.rs) — deferred until at least one codec's GPU
   decode path is hardware-verified, so the refactor doesn't entangle two still-unverified
   debugging efforts.
8. **`mediaway_common::GpuBufferHandle::DirectX12`'s missing `subresource` field** — the same
   real gap ADR-0002's H.264 addendum found (this crate cannot express "one slot of a
   texture-array DPB" as a real `GpuBufferHandle` today) applies identically to
   `DecodedOutputHevc::Gpu`; not re-litigated here, same cross-crate decision ADR-0002
   already flagged.
9. **Whether `mediaway-encoder-windows`'s D3D12 HEVC GOP encoder (`gop_hevc.rs`) actually
   produces a real forward-reference single-slice P-frame bitstream compatible with this
   ADR's scope cuts** (single-tile, no B-frames, `num_ref_idx_l0_active == 1`) — plausible
   given it mirrors the H.264 `H264GopState` shape used to produce the H.264 D3D12 decode
   test's own P-frame input, but not independently re-confirmed by reading `gop_hevc.rs`'s
   full implementation this session (only its module doc/state-machine shape was checked).
   First thing to verify (via source reading, not hardware) before writing the hardware
   test's setup code.

## Test plan

**Sans-io unit tests — write and run this pass, no hardware involved, no risk:**

- `hevc_vps_sps_pps_tests.rs` — VPS/SPS/PPS parsing against hand-built bitstream fixtures
  (mirrors `h264_sps_pps_tests.rs`'s and `vulkan::hevc_params_tests.rs`'s own convention);
  explicit tests for every scope-cut rejection (`tiles_enabled_flag`, `pcm_enabled_flag`,
  `scaling_list_enabled_flag == 1`, `chroma_format_idc != 1`, `separate_colour_plane_flag`,
  10/12-bit `bit_depth_*`).
- `hevc_slice_tests.rs` — slice-segment-header + RPS parsing, direct port-and-extend of
  `crate::vulkan::hevc_slice_tests.rs`'s fixtures where directly applicable, plus new tests
  for this ADR's single-forward-reference rejection (`total used_by_curr_pic != 1` →
  `Unsupported`), B-slice rejection, multi-slice-picture rejection.
- `hevc_poc_tests.rs` — POC MSB-cycle computation (ITU-T H.265 § 8.3.1) against hand-picked
  `pic_order_cnt_lsb`/`log2_max_pic_order_cnt_lsb` sequences exercising at least one MSB-wrap
  case, mirroring `h264_poc_tests.rs`'s own convention for H.264's type-0 POC.
- `hevc_refs_tests.rs` — `RefPicList`/`RefPicSetStCurrBefore`/`After` construction against a
  synthetic DPB (`Vec<(u32, HevcRefMeta)>`, no D3D12 device involved — same
  `dpb_tests.rs`-style device-free testing `SlotTable<M>` already enables), covering: the
  single-forward-reference happy path, the "no matching DPB slot for a signaled POC"
  error case, the ">1 reference" rejection case.
- `hevc_pic_params_tests.rs` — `DxvaPicParamsHevc`/`DxvaQmatrixHevc`/`DxvaSliceHevcShort`
  field-packing against hand-built `Sps`/`Pps`/`SliceHeader` fixtures; at minimum, a
  `#[repr(C)]` size/offset sanity check per field group (mirrors how `h264_pic_params.rs`'s
  own struct layout was ground-truthed, adapted to check this ADR's own hand-transcribed
  layout self-consistently, since no `windows`-crate reference struct exists to compare
  against via `std::mem::size_of`).

Run via `cargo check -p mediaway-decoder --all-targets --features video`, `cargo clippy -p
mediaway-decoder --all-targets --all-features -- -D warnings`, `cargo test -p mediaway-decoder
--lib --features video` (or the equivalent scoped test invocation) — all achievable and
expected to be **required to pass** before this ADR's implementation is considered done, with
**zero hardware/device involvement**.

**Hardware-gated integration test — write, do NOT run:**

> ⚠️ **`d3d12_video_decode_hevc_tests.rs` must be written this pass (or in this ADR's
> eventual implementation pass) following the exact `..._or_skip_without_hw`-style soft-skip
> convention `d3d12_video_decode_tests.rs::h264_decode_idr_and_p_frame_or_skip` already
> uses (open/push/poll failures → `eprintln!` + soft skip, never a hard `assert!` on a
> not-yet-root-caused symptom) — but it must NOT be executed by whoever implements this ADR.**
> Per the safety banner, running it risks a real `DXGI_ERROR_DEVICE_HUNG` TDR on this
> workspace's dev machine, on code that (unlike H.264's at least-once-attempted GPU path) has
> never been run at all. Leave it written, compiling, and explicitly unrun; a human/agent
> with informed, deliberate consent for a real hardware attempt (mirroring every "explicit
> project-owner go-ahead" ADR-0002 addendum required before each of its 8 real TDRs) must
> decide separately, later, whether and when to run it.

Planned shape once that consent exists (design only, not built this pass): use
`mediaway-encoder-windows`'s D3D12 HEVC encoder with `gop_hevc.rs`'s GOP mode enabled to
produce a real, driver-encoded multi-frame HEVC Annex-B bitstream (IDR + at least one real
P-frame) — the same "chain this workspace's own hardware-verified encoder into the decoder
under test" technique `tests/vulkan/hardware_hevc_decode.rs` already established for HEVC,
adapted from H.264 D3D12's own WMF-encoder-sourced test input. Feed those exact bytes into
`D3d12VideoDecoderHevc`; soft-skip (not fail) on any `open`/`push_packet`/`poll_frame` error,
consistent with this workspace's "a real, not-yet-root-caused bug must soft-skip" convention.

## References

- [`mediaway-decoder-windows` ADR-0002](0002-d3d12-native-video-decode.md) — this module's
  founding ADR and its five addenda (2026-07-29 through 2026-08-07); **read the safety
  banner's citations in full before implementing anything here** — 8 real hardware TDRs,
  root cause unresolved.
- [`mediaway-decoder` ADR-vulkan-0001](../vulkan/0001-vulkan-video-decode.md) — this ADR's
  primary porting source for HEVC bitstream/RPS parsing (its 2026-07-30 and 2026-08-05 HEVC
  addenda in particular); the correct precedent to use in place of the task's described
  (not-actually-present) VA-API HEVC ADR.
- [`mediaway-decoder-linux` ADR-0002](../linux/0002-vaapi-h264-p-slice-dpb.md) — real source
  of the "single-forward-reference P-slice" scope-cut shape this ADR mirrors for HEVC (its
  title/subject is H.264, not HEVC — see § Correcting the task's premise).
- `crates/mediaway-decoder/src/vulkan/{decoder_hevc.rs,hevc_params.rs,hevc_params/hevc_ptl.rs,hevc_slice.rs,session_command_hevc.rs}`
  — real, hardware-verified-for-IDR HEVC bitstream-parsing source read directly this session.
- `crates/mediaway-decoder/src/windows/d3d12_video_decode/{dpb.rs,setup.rs,util.rs,ops.rs,h264.rs,h264_pic_params.rs,h264_poc.rs,h264_refs.rs}`
  and `crates/mediaway-decoder/src/windows/d3d12_video_decode.rs` — the existing H.264
  implementation this ADR extends/mirrors, read directly this session.
- `crates/mediaway-encoder/src/windows/d3d12_video_encode/gop_hevc.rs` — real, hardware-
  verified (per project memory, 2026-08-06) HEVC GOP/P-frame encode, the planned real-
  bitstream source for this ADR's (unrun) hardware test.
- Wine project `dxva.h` mirror: <https://raw.githubusercontent.com/wine-mirror/wine/master/include/dxva.h>
  (fetched this session; primary source for every DXVA-HEVC struct layout above).
- Microsoft Learn: [`DXVA_PicParams_HEVC` structure](https://learn.microsoft.com/en-us/windows/win32/medfound/dxva-picparams-hevc)
  (fetched this session; secondary source, flagged discrepancy — see § DXVA struct definitions).
- [`docs/spec/sans-io.md`](../../../../docs/spec/sans-io.md),
  [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md),
  [`docs/spec/gpu-interop.md`](../../../../docs/spec/gpu-interop.md).

ADRs are **English**. Numbering is local to this `adr/` folder.
