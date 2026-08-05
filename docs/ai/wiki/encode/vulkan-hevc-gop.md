# Vulkan HEVC GOP (CBR stays H.264-only)

- Module: `mediaway-encoder::vulkan` (`hevc_gop.rs`, `hevc_params.rs`,
  `session.rs`, `session_command_hevc.rs`, `encoder.rs`)
- ADR: [vulkan/0002](../../../../crates/mediaway-encoder/adr/vulkan/0002-vulkan-gop-rate-control.md)
  — § "Implementation update (HEVC)", 2026-08-05 hardware-verified
- Sibling: [vulkan-h264-gop](vulkan-h264-gop.md) — same design, this page
  only covers where HEVC's shape genuinely differs

## Why a separate `GopState`, not a shared one

`hevc_gop::GopState`/`Dpb`/`DpbSlot`/`FrameDecision` mirror `h264_gop`'s
shape but are a distinct type, not a generalized/shared one — HEVC's
`StdVideoEncodeH265ReferenceInfo` has no `FrameNum` equivalent (only
`PicOrderCntVal`) and `StdVideoEncodeH265PictureInfo` has no `idr_pic_id`
field, so `DpbSlot { poc, is_idr }`/`FrameDecision` both drop those fields
entirely rather than carrying an always-unused H.264-shaped one.
`WORKSPACE_DPB_CAP` **is** reused directly from `h264_gop` (genuinely
codec-agnostic slot-count headroom) — matching how `session_command.rs`'s
upload/barrier/readback helpers stay shared, not duplicated, in
`session_command_hevc.rs`.

`GopState::decide` produces `PicOrderCntVal` directly (no `2 *` doubling —
that was H.264 POC-type-2's own convention); resets to `0` at every IDR.
`log2_max_pic_order_cnt_lsb_minus4 = 12` when GOP is active (widest legal
value, sidesteps H.265 §8.3.1's `PicOrderCntMsb` wraparound — direct analogue
of H.264's `log2_max_frame_num_minus4 = 12`).

## Picture-embedded short-term RPS (not SPS-declared)

A P-frame's single L0 reference is signaled via a
`StdVideoH265ShortTermRefPicSet` pointed to by
`StdVideoEncodeH265PictureInfo::pShortTermRefPicSet`
(`short_term_ref_pic_set_sps_flag == 0`) — built fresh per frame
(`hevc_params::build_single_ref_short_term_ref_pic_set`: `num_negative_pics =
1`, `delta_poc_s0_minus1[0] = 0`, `used_by_curr_pic_s0_flag` bit 0 set), not
an SPS-level RPS list entry. This crate's SPS always keeps
`num_short_term_ref_pic_sets == 0`, GOP or not — H.265's std header supports
direct picture-level RPS signaling, unlike H.264 (which has no equivalent and
instead relies on POC type 2's implicit derivation), so there was nothing an
SPS-level list would have bought here.

`dec_pic_buf_mgr_single_ref()` sets `MaxDecPicBufferingMinus1[0] == 1` (DPB
holds the current picture + one reference) where Stage 1's
`dec_pic_buf_mgr_no_refs()` used `0`; `MaxNumReorderPics[0]` stays `0` in
both — no B-frames, ever.

## Command recording — same barrier fix, ported

`session_command_hevc.rs::record_and_submit_hevc` forwards a real
`dpb.layer_count`/`dpb.transition` into `record_upload_and_barriers`
(previously hardcoded `1, true`) — the same one-time-not-per-frame
`UNDEFINED -> VIDEO_ENCODE_DPB_KHR` discard-transition fix H.264's pass found
(see [vulkan-h264-gop](vulkan-h264-gop.md)). `record_video_coding_hevc`
needed the identical setup/reference `VkVideoReferenceSlotInfoKHR` +
`VkVideoEncodeH265DpbSlotInfoKHR` chaining restructuring — this file already
had its own separate command-recording function (the picture-info `pNext`
payload type differs per codec), so the GOP-aware version is a sibling
rewrite, not new shared code.

## CBR: verified as a real no-op for HEVC

ADR-0002 scopes CBR to H.264 only. `encoder.rs`'s `rate_control_params` stays
gated `is_h264 && capabilities.supports_cbr` — `record_video_coding_hevc`
never reads a rate-control param at all, always `DISABLED` fixed-QP.
`push_hevc_frames_gop_with_rate_control_requested_or_skip` hardware-verifies
requesting `rate_control` on an HEVC config is silently and safely ignored
(session opens, GOP cadence holds, packets stay reasonably sized) — not a CBR
sanity check, since there is no CBR path here to sanity-check.

## Hardware verification (2026-08-05, RTX 4090)

```
vulkan HEVC GOP VideoEncoder ok: 3 IDR + 4 P packets, cadence matched gop_size=3
vulkan HEVC GOP+rate_control-requested VideoEncoder ok: 6 packets, 568 total bytes
```

7 frames at `gop_size = 3` → real `I P P I P P I` NAL cadence (type 19
`IDR_W_RADL` vs type 1 `TRAIL_R`), worked on the first hardware attempt,
stable across 3 repeated runs. `push_three_hevc_frames_or_skip` (Stage 1's
IDR-only HEVC path) still passes unchanged. Tests:
`encoder_tests.rs::push_seven_hevc_frames_gop_or_skip`,
`::push_hevc_frames_gop_with_rate_control_requested_or_skip`.
