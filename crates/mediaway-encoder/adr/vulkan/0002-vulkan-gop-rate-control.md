# ADR-0002: Multi-frame GOP (P-frames only) + CBR rate control for Vulkan Video encode

- **Status**: Accepted — H.264 and HEVC halves both implemented and
  **hardware-verified** 2026-08-05 (same day, two follow-up passes; see
  § Implementation update below for H.264 and § Implementation update
  (HEVC) further down for HEVC). CBR rate control stays H.264-only per this
  ADR's original Decision section scope — HEVC's `rate_control` requests are
  safely ignored (fixed-QP fallback), not implemented. AV1's GOP wiring is
  also now **implemented, real, and capability-gated — but genuinely
  unverifiable** on this crate's reference hardware (see § Implementation
  update (AV1) further down): this ADR's original Decision section excluded
  AV1 outright for exactly this reason, and that exclusion turned out to be
  correct — the driver bug it anticipated is still present.
- **Date**: 2026-08-05
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`src/vulkan/`)

## Implementation update (2026-08-05, same-day follow-up): H.264 GOP + CBR — hardware-verified

A follow-up task implemented this ADR's H.264 half only (HEVC/AV1 untouched,
matching this ADR's own scope). **Real DPB slot cycling, real P-frame
`RefPicList0` prediction, and real CBR rate control all worked on the first
hardware attempt** — the RTX 4090 reference machine, same driver as every
other addendum in this crate's ADRs.

### Result

```
vulkan H.264 GOP VideoEncoder ok: 3 IDR + 4 P packets, cadence matched gop_size=3
vulkan H.264 GOP+CBR VideoEncoder ok: 6 packets, 394 total bytes (target_bitrate_bps=500000, ...)
```

7 frames pushed with `gop_size = 3`: real IDR (NAL type 5) / P-slice (NAL
type 1) NALs in the exact `I P P I P P I` cadence `GopState::decide`
predicts, scanned via this crate's existing Annex-B `nal.rs` scanner.
Stable across 3 repeated `cargo test -p mediaway-encoder --lib
vulkan::encoder_tests:: -- --nocapture` runs. Stage 1's own diagnostic
(`encode_synthetic_intra_frame`) reproduced its historical exact result
byte-for-byte (`68 bitstream bytes`, NAL offsets `4`/`15`/`23`, unchanged
since ADR-0001) — confirming zero regression to the untouched default path.

### Deviations from this ADR's design sketch

- **`log2_max_frame_num_minus4 = 12`** (not left at the design sketch's
  implicit `0`) — the sketch flagged `frame_num` wraparound as an open
  question ("you may need to bump this"); `12` is H.264's spec-legal maximum
  (`log2_max_frame_num = 16`), chosen to sidestep implementing §8.2.4.1's
  `FrameNumWrap` arithmetic entirely (irrelevant to a single-forward-reference
  design) for any `gop_size` up to 65536. Only applied when GOP mode is
  active — the default (`gop_size == 1`) path keeps Stage 1's exact `0`.
- **DPB image layout transition, one-time not per-frame**: found via code
  reading (not a hardware bug) that Stage 1's per-frame
  `UNDEFINED -> VIDEO_ENCODE_DPB_KHR` barrier would silently discard
  already-written reference slots' content every subsequent frame if reused
  unmodified for GOP mode. Fixed by transitioning the whole (now multi-layer)
  DPB image once, before the first frame, and using a same-layout no-op
  barrier thereafter — see `session_command.rs::record_pre_encode_barriers`'s
  doc. The default (`gop_size == 1`) path is unaffected (transitions every
  frame, exactly as before — harmless there since that DPB slot is never
  read back).
- **`VkVideoEncodeH264DpbSlotInfoKHR` chaining**: chained onto both the
  setup slot and the read reference slot, in both `vkCmdBeginVideoCodingKHR`'s
  and `vkCmdEncodeVideoKHR`'s reference-slot arrays — mirroring the AV1
  addendum's own finding in `adr/0001` ("an earlier draft left
  `VkVideoReferenceSlotInfoKHR` bare... AV1's reference model needs this per
  FFmpeg's reference"). Unlike the AV1 case, this worked correctly on the
  first attempt here.
- **`RateControlConfig::vbv_buffer_size_bytes` → `virtualBufferSizeInMs`**:
  the ADR's config surface is byte-denominated (`vbv_buffer_size_bytes`) but
  `VkVideoEncodeRateControlInfoKHR::virtualBufferSizeInMs` is
  millisecond-denominated — converted via `bytes * 8 * 1000 /
  target_bitrate_bps`; `None` (or `target_bitrate_bps == 0`) leaves it `0`
  (spec-documented "implementation picks a default" sentinel), matching the
  config field's own doc.
- **`constant_qp` forced to `0` under CBR**: not mentioned in the design
  sketch — `VkVideoEncodeH264NaluSliceInfoKHR::constantQp` must be `0`
  whenever rate control is not `DISABLED` per the Vulkan spec's own VUID;
  `VulkanVideoEncoder::push_frame` now branches on
  `rate_control_params.is_some()`.
- **`GopState` owned unconditionally** (not `Option<GopState>`) on
  `VulkanVideoEncoder`, including for HEVC/AV1 sessions where it stays at
  its `gop_size == 1` default and is never read — simpler than an `Option`
  whose split would just mirror `codec == H264`, and avoids an
  invariant-violation error path that would otherwise need
  `unwrap`/`expect` (denied outside tests) to read back out.

### What this pass does not cover

HEVC wiring of `gop_size` (closed by the follow-up pass below, same day).
WMF/D3D12 wiring (separate work, unblocked by this ADR per its own
Consequences section). Intra-refresh, multi-reference search, long-term
references, SVC temporal layering — all explicitly out of scope per this
ADR's own "What this ADR does not cover" section, unchanged.

## Implementation update (2026-08-05, second same-day follow-up): HEVC GOP — hardware-verified

A second follow-up task closed this ADR's remaining HEVC gap, mirroring the
H.264 pass above. **Real DPB slot cycling and real P-frame reference
prediction worked on the first hardware attempt** — same RTX 4090, same
driver.

### Result

```
vulkan HEVC GOP VideoEncoder ok: 3 IDR + 4 P packets, cadence matched gop_size=3
vulkan HEVC GOP+rate_control-requested VideoEncoder ok: 6 packets, 568 total bytes
  (rate_control silently ignored per ADR-0002, fixed-QP path used)
```

7 frames pushed with `gop_size = 3`: real IDR (NAL type 19, `IDR_W_RADL`) /
P-slice (NAL type 1, `TRAIL_R`) NALs in the exact `I P P I P P I` cadence
`hevc_gop::GopState::decide` predicts — scanned via this crate's existing
`nal.rs::scan_nal_headers_hevc` (already handled HEVC's 2-byte NAL header
from Stage 1, no scanner changes needed). Stable across 3 repeated
`cargo test -p mediaway-encoder --lib vulkan::encoder_tests:: -- --nocapture`
runs. The unrelated `push_three_hevc_frames_or_skip` (Stage 1's IDR-only
HEVC path) still passes unchanged, confirming no regression to the default
(`gop_size == 1`) path.

### New pieces (mirroring `h264_gop.rs`/`h264_params.rs`, not shared with them)

- **`hevc_gop.rs`** — a separate `GopState`/`Dpb`/`DpbSlot`/`FrameDecision`
  state machine, not a generalized/shared one with `h264_gop.rs`. Simpler
  than H.264's: `StdVideoEncodeH265ReferenceInfo` has no `FrameNum`
  equivalent (only `PicOrderCntVal`), and `StdVideoEncodeH265PictureInfo` has
  no `idr_pic_id` field to sequence, so `DpbSlot`/`FrameDecision` both drop
  those fields entirely rather than carrying an unused H.264-shaped one.
  `WORKSPACE_DPB_CAP` **is** reused directly from `h264_gop` (`pub(crate)
  use`) — genuinely codec-agnostic (DPB slot headroom, not tied to any
  H.264-specific syntax), matching how `session_command.rs`'s
  upload/barrier/readback helpers stay shared, not duplicated, in
  `session_command_hevc.rs`.
- **`log2_max_pic_order_cnt_lsb_minus4 = 12`** (HEVC's spec-legal maximum,
  `hevc_gop::LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4`) — direct HEVC analogue of
  H.264's `log2_max_frame_num_minus4 = 12` deviation above, same reasoning
  (sidesteps H.265 §8.3.1's `PicOrderCntMsb` wraparound derivation for any
  `gop_size` up to 65536, irrelevant to single-forward-reference prediction).
- **Picture-embedded short-term RPS, not SPS-declared**: a P-frame's single
  L0 reference is signaled via a `StdVideoH265ShortTermRefPicSet` pointed to
  by `StdVideoEncodeH265PictureInfo::pShortTermRefPicSet`
  (`short_term_ref_pic_set_sps_flag == 0`), built fresh per frame
  (`hevc_params::build_single_ref_short_term_ref_pic_set`: `num_negative_pics
  = 1`, `delta_poc_s0_minus1[0] = 0`, `used_by_curr_pic_s0_flag` bit 0 set) —
  not an SPS-level `pShortTermRefPicSet` list entry. This crate's SPS always
  keeps `num_short_term_ref_pic_sets == 0`, GOP or not; H.265's std header
  supports picture-embedded RPS signaling directly (unlike H.264, which has
  no equivalent "skip the SPS list" option and instead relies on POC type 2's
  implicit derivation), so there was nothing an SPS-level RPS list would have
  bought here.
- **`dec_pic_buf_mgr_single_ref()`**: GOP mode's `StdVideoH265DecPicBufMgr`
  needs `MaxDecPicBufferingMinus1[0] == 1` (DPB holds the current picture
  plus one reference) where Stage 1's `dec_pic_buf_mgr_no_refs()` used `0`;
  `MaxNumReorderPics[0]` stays `0` in both (no B-frames, ever).
- **Same DPB-layout-barrier fix, ported**: `session_command_hevc.rs`'s
  `record_and_submit_hevc` now forwards a real `dpb.layer_count`/
  `dpb.transition` into `record_upload_and_barriers` (previously hardcoded
  `1, true`) — the exact one-time-not-per-frame discard-transition fix the
  H.264 pass found and documented in `session_command.rs`. HEVC's
  `record_video_coding_hevc` needed the identical setup/reference
  `VkVideoReferenceSlotInfoKHR` + `VkVideoEncodeH265DpbSlotInfoKHR` chaining
  restructuring as H.264's `record_video_coding` — this file had its own
  (separate, not shared) command-recording function to begin with (see this
  ADR's Context section on why: the picture-info `pNext` payload type
  differs per codec), so the GOP-aware rewrite is a sibling function, not a
  shared one.
- **No `idr_only()` constructor on `DpbRecordParamsHevc`**: unlike H.264's
  `DpbRecordParams::idr_only()` (still called by `session_encode.rs`'s
  `encode_synthetic_intra_frame`, H.264's Stage 1 one-shot diagnostic), this
  crate has no HEVC equivalent of that diagnostic — `VulkanVideoEncoder::push_frame`
  is `DpbRecordParamsHevc`'s only caller and always builds it from
  `self`'s own GOP/DPB state, so an `idr_only()` constructor would have been
  genuinely unused (`dead_code`). Same reasoning kept
  `hevc_params::build_frame_structs`'s IDR branch calling
  `build_idr_picture_info()` wholesale (unlike H.264's version, which
  reimplements the IDR flags inline) — HEVC's whole-struct builder needed a
  second caller to stay alive too.
- **CBR stays H.264-only, verified as a real no-op for HEVC**: a
  `rate_control` request on an HEVC `VideoEncoderConfig` is silently ignored
  by design (`encoder.rs`'s `rate_control_params` computation is gated
  `is_h264 && capabilities.supports_cbr`) — `record_video_coding_hevc` never
  reads a rate-control param at all, always building `DISABLED` fixed-QP.
  `push_hevc_frames_gop_with_rate_control_requested_or_skip` hardware-verifies
  this is a safe no-op (session still opens, GOP cadence still holds, packets
  stay reasonably sized), not a CBR sanity check — there is no CBR path here
  to sanity-check.

### What this pass does not cover

WMF/D3D12 wiring (unchanged, separate work). CBR for HEVC (unchanged,
out of this ADR's H.264-only CBR scope — a future ADR would be needed to
widen it). AV1 (unchanged, blocked on the unrelated driver bug in ADR-0001's
AV1 addendum). Everything else this ADR's own "What this ADR does not cover"
section already excludes (B-frames, intra-refresh, multi-reference search,
long-term references, SVC temporal layering) — unchanged, applies to both
codecs identically.

## Implementation update (AV1): real GOP/DPB wiring — implemented but genuinely unverifiable on this hardware

A third follow-up task, run after the H.264 and HEVC passes above, was asked
to build AV1's GOP/DPB wiring anyway, explicitly scoped as "implemented but
unverifiable" — the user's own instruction, not a scope decision this session
made unilaterally. This directly contradicts this ADR's original Decision
section ("AV1 stays IDR-only/`DISABLED`... building GOP/rate-control work on
top of a broken single-frame path would be unverifiable") and the
Alternatives Considered table's "Deferred until the base path is confirmed
fixed" reasoning for the same row — both are **superseded by this section**,
not silently ignored.

**Re-confirmation immediately before this task started**: re-running
`push_three_av1_frames_or_skip` on this crate's reference RTX 4090 reproduced
the exact same failure ADR-0001's AV1 addendum already documented — packet
0's own frame data is `obu_type = 0` (`Reserved`, illegal), not a valid OBU.
The known driver-maturity limitation is still present, unchanged, on this
same driver version (32.0.15.9579).

### What was built

- **`src/vulkan/av1_gop.rs`** (new) — a separate `GopState`/`Dpb`/`DpbSlot`/
  `FrameDecision` state machine, mirroring `h264_gop.rs`/`hevc_gop.rs`'s
  shape but keyed on AV1's `order_hint` (no `frame_num`/`PicOrderCnt`
  equivalent exists in `StdVideoEncodeAV1PictureInfo`). Same
  single-forward-reference scope as H.264/HEVC: only AV1's `LAST_FRAME`
  reference name is ever used, never `LAST2`/`LAST3`/`GOLDEN`/`BWDREF`/
  `ALTREF2`/`ALTREF`, even though AV1's own reference model supports all 8.
  One physical `WORKSPACE_DPB_CAP`-ring DPB slot is tied 1:1 to one AV1
  virtual reference-frame-slot number (`refresh_frame_flags`/`ref_frame_idx`
  both address it directly) — the simplest mapping that stays consistent with
  H.264/HEVC's narrow single-reference design, not a generalized N-reference
  scheduler.
- **`order_hint_bits_minus_1` widened to `7`** (`av1_gop::ORDER_HINT_BITS_MINUS_1_GOP`,
  AV1's spec-legal maximum) when GOP is active — same "sidestep wraparound"
  reasoning as H.264's `log2_max_frame_num_minus4 = 12`/HEVC's
  `log2_max_pic_order_cnt_lsb_minus4 = 12`, but AV1's own spec caps this
  field at 8 bits (`order_hint` wraps mod 256) — a real, inherent-to-the-format
  ceiling far below H.264/HEVC's 65536-frame headroom, not a choice this
  crate could widen further.
- **`av1_params.rs`**: `Av1SeqGopParams` (GOP-selected `order_hint_bits_minus_1`,
  mirroring `SpsGopParams`/`HevcSpsGopParams`); `build_reference_info` gained
  `order_hint`/`is_key` parameters (was hardcoded `OrderHint: 0`/`KEY_FRAME`);
  new `InterFramePrediction` + `build_inter_frame_picture_info` build one
  `StdVideoEncodeAV1PictureInfo` for an `INTER_FRAME` predicted from the sole
  `LAST_FRAME` reference — same flag values as the existing `KEY_FRAME`
  builder throughout (no optional coding tool this crate's narrow scope
  enables), differing only in `frame_type`/`order_hint`/`refresh_frame_flags`/
  `ref_frame_idx`/`ref_order_hint`.
- **`primary_ref_frame` stays `PRIMARY_REF_NONE` for `INTER_FRAME` too** (not
  the referenced slot's `ref_frame_idx` position) — a deliberate scope cut,
  not an oversight: motion-compensated prediction still reads pixels from
  `LAST_FRAME` via `ref_frame_idx`/`reference_name_slot_indices` regardless;
  `primary_ref_frame` only controls whether this frame's CDF context carries
  forward from a previous frame's adapted state or starts from AV1's
  spec-default CDFs. Carrying CDF state across this crate's DPB ring adds
  real bookkeeping this crate cannot itself verify against real hardware (the
  base encode is already broken) for no provable benefit here, so this pass
  keeps the already-established `KEY_FRAME` builder's simpler choice rather
  than speculatively adding untestable complexity.
- **`session.rs::Capabilities::supports_p_frames`** — AV1's per-codec floor
  check changed from an unconditional `true` (AV1 was previously skipped
  entirely) to a real driver query:
  `VkVideoEncodeAV1CapabilitiesKHR::maxSingleReferenceCount >= 1` (this crate
  only ever requests AV1's `SINGLE_REFERENCE` prediction mode, matching
  H.264/HEVC's `maxPPictureL0ReferenceCount >= 1` floor check exactly). This
  gate is real and driver-queried even though the resulting P-frame path
  can't be verified end-to-end — it answers "would this driver honor the
  request", not "does the resulting bitstream decode". No CBR-equivalent
  capability was added for AV1 — see below.
- **`session_command_av1.rs`**: `DpbRecordParamsAv1` (mirrors
  `DpbRecordParamsHevc`) plus a `record_video_coding_av1` rewrite carrying
  real setup/reference `VkVideoReferenceSlotInfoKHR` +
  `VkVideoEncodeAV1DpbSlotInfoKHR` chaining, forwarding a real
  `dpb.layer_count`/`dpb.transition` into `record_upload_and_barriers`
  (previously hardcoded `1, true`) — the same one-time-not-per-frame
  `UNDEFINED -> VIDEO_ENCODE_DPB_KHR` discard-transition fix the H.264 pass
  found, ported here too (this file had no per-frame DPB reuse before this
  pass, so the bug itself never manifested here, but the fix is applied
  proactively rather than waiting to rediscover it).
- **`encoder.rs`**: `av1_gop_state: Av1GopState` field (owned unconditionally,
  idle unless `codec == Av1`, same pattern as `gop_state`/`hevc_gop_state`);
  `gop_enabled`/`supports_gop_for_codec` widened to include AV1 (was
  `is_h264 || is_hevc` only); the AV1 `push_frame` branch now matches on
  `decision.reference` to choose `INTRA_ONLY`/`RATE_CONTROL_GROUP_INTRA` vs.
  `SINGLE_REFERENCE`/`RATE_CONTROL_GROUP_PREDICTIVE` and builds a real
  `DpbRecordParamsAv1` instead of always hardcoding an IDR-only,
  single-layer-DPB shape.

### CBR: not added for AV1 (deliberate, independent decision)

Following HEVC's own precedent (ADR-0002 already scopes CBR to H.264 only,
and HEVC's `rate_control` requests are a safe no-op), this pass does **not**
wire `VkVideoEncodeAV1RateControlInfoKHR`'s `CBR` mode — AV1 sessions keep
`RATE_CONTROL_MODE_DISABLED` fixed-QP unconditionally, same as HEVC. Two
independent reasons, either alone sufficient: (1) no driver/workload evidence
this crate needs CBR for AV1 specifically, the same reasoning HEVC's own ADR
section gives; (2) AV1's base per-frame encode is already broken on this
hardware — adding a second untestable rate-control surface on top of an
already-untestable GOP surface compounds unverifiable scope for no provable
benefit this session could confirm either way.

### Test disposition

`encoder_tests.rs::push_seven_av1_frames_gop_or_skip` attempts the same
`I P P I P P I` cadence check `push_seven_frames_gop_or_skip`/
`push_seven_hevc_frames_gop_or_skip` run, but is written to honestly skip
(print the reason, `return`, no `assert!`/panic) the moment it hits a packet
whose own frame data is not a valid OBU — mirroring
`push_three_av1_frames_or_skip`'s existing honest-skip convention exactly.

**Actual result on this crate's reference RTX 4090 (2026-08-05)**:

```
skip: packet 0's own frame data is not a valid OBU (found [ObuHeader { obu_type: 1, offset: 0 }, ObuHeader { obu_type: 0, offset: 13 }]) — known driver-maturity limitation on this hardware, same root cause as push_three_av1_frames_or_skip, see `adr/0001`'s AV1 addendum and `adr/vulkan/0002`'s AV1 follow-up section
```

This is the expected, honest outcome — not a pass with real multi-frame
output. Two things **are** confirmed real by this run, though: the session
opened successfully with `gop_size = 3` (meaning
`Capabilities::supports_p_frames` evaluated `true` for AV1 on this driver —
the capability gate is genuinely live, not dead code), and packet 0's
`is_keyframe` cadence matched `Av1GopState::decide`'s prediction before the
skip point was reached (this crate's own state machine, no driver
involvement) — so the GOP wiring built here is exercised on real hardware up
to exactly the same wall ADR-0001's AV1 addendum already hit, and no further.
No unexpected result was observed; the driver bug did not "not manifest" for
this shape. `cargo test -p mediaway-encoder --all-targets -- --nocapture`
also re-confirmed every existing H.264/HEVC test (Stage 1 diagnostic byte
count/NAL offsets, both codecs' GOP cadence, H.264 CBR, HEVC
rate-control-requested-and-ignored) unchanged.

### What this pass does not cover

Everything AV1 already didn't cover, unchanged: Zero-Copy GPU input,
`hasOverrides` sequence-header re-negotiation (ADR-0001's AV1 addendum,
"What remains unresolved"), multi-reference search / long-term references /
SVC temporal layering, intra-refresh, B-frames (permanent non-goal, all
codecs). WMF/D3D12 (unaffected, out of this ADR's Vulkan-only scope). No new
attempt was made to resolve or work around the underlying driver bug itself
— this pass adds the GOP/DPB shape on top of it, as explicitly scoped.

## Context

[`docs/vulkan/roadmap.md`](../../docs/vulkan/roadmap.md) Stage 2 ("GOP / rate
control") has been deferred since ADR-0001's Stage 1: every pushed frame is an
independent IDR (`h264_params.rs::build_idr_picture_info`,
`hevc_params.rs`'s equivalent), `max_num_ref_frames = 0`, and rate control is
hardcoded `VkVideoEncodeRateControlModeFlagsKHR::DISABLED` (fixed QP 26) in
`session_command.rs:290-291`, `session_command_hevc.rs:120`,
`session_command_av1.rs:134`. No DPB slot beyond one is ever written then read
back as a prediction reference.

This is being picked up now because it blocks the workspace's low-latency
streaming push: IDR-only encode is bandwidth-wasteful (every frame carries a
full intra-coded picture) and fixed-QP has no bitrate ceiling, both of which
work against a bandwidth-constrained streaming link even though neither
directly adds encoder latency by itself.

**Deliberate exclusion up front: B-frames are out of scope, permanently, not
just this stage.** B-frame prediction requires the encoder to buffer future
frames before it can encode the picture in between — that is reorder latency,
directly opposed to the low-latency goal this work exists for. This ADR
scopes in P-frame-only GOP (forward reference only, `RefPicList0`, no
`RefPicList1` use) and explicitly rules B-frames out as a non-goal, not an
oversight.

### Related backends (context only — this ADR's decision scope is Vulkan)

- **WMF** (`src/windows/wmf/`): zero `ICodecAPI`/`CODECAPI_AVEnc*` usage
  anywhere in the crate today — GOP/rate-control there is greenfield, not
  covered by this ADR.
- **D3D12** (`src/windows/d3d12_video_encode/`): GOP struct plumbing already
  exists (`D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264`/HEVC in
  `setup.rs`/`ops*.rs`) but is hardcoded `GOPLength = 1`, `IntraRefresh =
  NONE`, rate control fixed `CQP=26` — closest to a mechanical fix, but this
  backend is currently paused on an unrelated GPU-driver TDR hang
  (`adr/windows/0002`), so it is not touched here.

## Decision

> Add real multi-slot DPB + P-frame reference cycling and CBR rate control to
> the Vulkan H.264 and HEVC encode paths only. ~~AV1 stays IDR-only/`DISABLED`
> (it is already driver-blocked on invalid per-frame bitstream output per
> ADR-0001's AV1 addendum — GOP/rate-control work on top of a broken
> single-frame path would be unverifiable).~~ **Superseded**: § "Implementation
> update (AV1)" adds AV1's GOP/DPB wiring too (not CBR), per an explicit later
> instruction, honestly labeled implemented-but-unverifiable rather than
> deferred. No B-frames, ever, in this backend — unchanged.

### Config surface (facade-level, minimal)

Two new fields on `mediaway_encoder::VideoEncoderConfig`
(`src/video.rs`) — cross-backend struct, but only Vulkan reads them after this
ADR; WMF/D3D12 wiring is separate follow-up work, not blocked on this ADR:

```rust
/// Frames between forced IDR refreshes. `1` = IDR-only (today's behavior,
/// stays the `Default`/`h264()`/`hevc()` constructor value — zero behavior
/// change for existing callers). `0` is rejected (`EncodeError`), not treated
/// as "infinite GOP" — an explicit value avoids silent unbounded drift.
pub gop_size: u32,
/// Target bitrate ceiling for CBR-style rate control. `None` keeps today's
/// fixed-QP `DISABLED` mode. `Some(_)` is a request, not a guarantee — a
/// backend that cannot honor CBR (capability-gated, see below) falls back to
/// `DISABLED` and must document that fallback on its own encoder type's
/// rustdoc, per `caveats-and-clarity.md`.
pub rate_control: Option<RateControlConfig>,
```

`RateControlConfig { target_bitrate_bps: u32, vbv_buffer_size_bytes: Option<u32> }`
— `vbv_buffer_size_bytes: None` lets the backend pick a driver-suggested
default rather than this crate guessing one.

### Capability gating (query before committing, never assume)

Before enabling either feature, `query_capabilities` (`session.rs`) must read
two more values it currently discards:

- `VkVideoCapabilitiesKHR::max_dpb_slots` /
  `max_active_reference_pictures` — the real ceiling on how many DPB slots
  this driver allows. The DPB slot count this crate requests is
  `min(driver_max, WORKSPACE_DPB_CAP)` where `WORKSPACE_DPB_CAP` is a small
  fixed constant (proposed `4` — enough for one active reference plus
  in-flight pipelining headroom, not driver-dependent tuning). Vulkan Video
  drivers are not required to support more than 1; if `max_dpb_slots < 2`,
  GOP falls back to IDR-only with no error (documented degradation, not a
  silent one — the returned `Capabilities` struct gains a
  `supports_p_frames: bool` field the caller can inspect).
- `VkVideoEncodeCapabilitiesKHR::rate_control_modes` (already queried into
  `encode_caps` in `query_capabilities` but never read past `DISABLED`) — must
  contain `CBR` before this crate requests it; same graceful-fallback
  contract, surfaced as `Capabilities::supports_cbr: bool`.
- Per-codec P-reference limits — `VkVideoEncodeH264CapabilitiesKHR::max_p_picture_l0_reference_count`
  / `VkVideoEncodeH265CapabilitiesKHR::max_p_picture_l0_reference_count` — this
  crate only ever requests 1 active L0 reference (single forward reference,
  not multi-reference search), so this is a floor check (`>= 1`), not a tuning
  knob.

### DPB / reference-picture state machine (ZCA shape)

Extends ADR-0001's `VideoSession<S>` typestate sketch (never implemented —
Stage 1 shipped without it, see that ADR's "Deliberate scope cuts") with one
new state after `ParametersReady`, plus a small owned DPB-cycling struct. No
new `dyn`/`Box` — closed concrete types, matching every backend in this
workspace:

```rust
/// Fixed-capacity ring of DPB slots. `WORKSPACE_DPB_CAP = 4` is small and
/// bounded — SmallVec buys nothing over a plain array here (no heap case to
/// avoid spilling to), so this stays a bare `[Option<DpbSlot>; WORKSPACE_DPB_CAP]`
/// rather than reaching for `smallvec` out of habit.
struct Dpb {
    slots: [Option<DpbSlot>; WORKSPACE_DPB_CAP],
    next_slot: usize, // round-robin cursor, wraps mod slots.len()
}

struct DpbSlot {
    image: vk::Image,        // one of this session's pre-allocated DPB images
    image_view: vk::ImageView,
    frame_num: u32,          // H.264: raw frame_num; HEVC: reuses the same counter pre-POC-derivation
    poc: i32,
}

/// Per-session forward-only prediction state. Owned by `VideoSession<Streaming>`,
/// not `Copy` — mutated in place each `encode_frame` call rather than rebuilt,
/// avoiding a per-frame allocation on what is already a per-frame hot path.
struct GopState {
    gop_size: u32,           // from VideoEncoderConfig::gop_size, 1 = IDR-only (today's path, unchanged)
    frames_since_idr: u32,
    frame_num: u32,          // H.264 §7.4.3 frame_num, wraps per SPS's log2_max_frame_num
    poc: i32,                // both codecs' picture-order-count, monotonic non-wrapping within this crate's scope
    dpb: Dpb,
}

impl VideoSession<ParametersReady> {
    fn into_streaming(self, gop_size: u32) -> VideoSession<Streaming> { .. }
}

impl VideoSession<Streaming> {
    /// Replaces the old `encode_idr_frame` (Stage 1, still kept for the
    /// existing single-shot diagnostic path — untouched). `Auto` lets the
    /// session decide IDR vs P from `GopState::frames_since_idr` vs
    /// `gop_size`; `ForceIdr` lets a caller request one out-of-band (e.g. on
    /// a detected packet-loss event upstream — not built by this ADR, just
    /// left as a hook).
    fn encode_frame(&mut self, input: &EncodeInput, request: FrameRequest)
        -> Result<Bytes, EncodeError> { .. }
}

enum FrameRequest { Auto, ForceIdr }
```

- A P-frame's `StdVideoEncodeH264ReferenceListsInfo` (`h264_params.rs`) gains
  a real `RefPicList0[0]` pointing at the DPB slot index the encoded frame
  should predict from (today's `build_empty_reference_lists` stays as the
  IDR-frame path, unchanged); HEVC's equivalent
  (`StdVideoEncodeH265ReferenceListsInfo`) mirrors this.
- `VkVideoReferenceSlotInfoKHR`/`VkVideoPictureResourceInfoKHR` per active
  slot get built fresh each `encode_frame` call from `Dpb::slots`, matching
  the per-frame construction pattern `session_command*.rs` already uses for
  the current single-slot case — no new abstraction needed there, just more
  slots.
- Rate control: `VkVideoEncodeRateControlInfoKHR` (mode `CBR`) chained with
  one `VkVideoEncodeRateControlLayerInfoKHR` (single temporal layer — this
  crate signals `VkVideoEncodeCapabilitiesKHR`'s single-layer case only, no
  SVC/temporal-layering scope creep) replaces the current
  `RATE_CONTROL_MODE_DISABLED` builder call when `Capabilities::supports_cbr`
  and `VideoEncoderConfig::rate_control` are both present; falls back to
  today's fixed-QP path otherwise (see capability gating above).

### What this ADR does not cover

- B-frames (permanent non-goal, see above).
- AV1 (blocked on the unrelated driver bug in ADR-0001's AV1 addendum).
- WMF/D3D12 wiring of the new `VideoEncoderConfig` fields (separate work;
  D3D12 additionally blocked on its own TDR bug).
- Zero-Copy GPU input (Stage 3, unrelated, still deferred).
- Multi-reference search / long-term references / SVC temporal layering —
  single forward reference only, matching this backend's existing
  "narrowest self-consistent parameter set" pattern (`h264_params.rs`'s own
  module doc phrase).
- Intra-refresh (spreads I-block cost across frames instead of one IDR
  spike) — genuinely useful for streaming but a separate encode-time
  mechanism (`StdVideoEncodeH264PictureInfo` has no intra-refresh signaling;
  it would need per-slice partial-intra macroblock selection) large enough to
  warrant its own follow-up ADR rather than folding into this one.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Keep IDR-only, add rate control (CBR) only | Cuts real scope, but CBR without any P-frames still means every "frame" is a full I-picture — CBR would have to either starve quality every frame to hit the bitrate target or blow through it constantly. GOP and rate control are coupled here, not independent knobs. |
| Add B-frames for better compression at a fixed target bitrate | Directly opposes this work's own motivation (low-latency streaming) — the reorder buffering B-frames require is latency, not just bitrate/quality tuning. Rejected outright, not deferred. |
| Full reference-picture-list flexibility (multi-ref search, long-term refs) day one | No driver/workload evidence this crate needs more than one forward reference; adds real DPB/POC bookkeeping complexity (`num_ref_idx_l0_active_minus1 > 0`, ref-list reordering ops) for a benefit unverified on this crate's own hardware. Single-reference first, revisit only if a measured quality/bitrate gap shows up. |
| Wire AV1 GOP/rate-control alongside H.264/HEVC now | AV1's per-frame encode output is already known-broken (driver bug, ADR-0001 AV1 addendum) — building GOP state on top of an unverified base produces work nobody can hardware-verify. Deferred until the base path is confirmed fixed. **Superseded**: § "Implementation update (AV1)" built it anyway, per an explicit later instruction, honestly labeled unverifiable rather than deferred further — this row's original reasoning was correct about the verification gap, wrong only about deferring being the only option. |

## Consequences

### Positive

- Closes the two items in `docs/vulkan/roadmap.md` Stage 2, unblocking real
  bandwidth-efficient streaming from this backend.
- Keeps every existing caller's behavior byte-identical by default
  (`gop_size = 1`, `rate_control = None` stay the `h264()`/`hevc()`
  constructor defaults) — this is additive, not a breaking change to
  `VideoEncoderConfig`.
- Capability-gated fallback (`supports_p_frames`/`supports_cbr`) means a
  driver that cannot do either still gets a working (if bandwidth-heavier)
  encoder instead of an open error — matches this backend's existing
  "probe first, never assume" discipline (`query_capabilities`'s whole
  design).
- No new `unsafe` surface shape beyond what `session_command*.rs` already
  has — more structs built per frame, not a new FFI pattern.

### Negative / Trade-offs

- Real new state to get right: `frame_num` wraparound
  (`log2_max_frame_num_minus4`), POC monotonicity, DPB slot lifetime (a slot
  must not be reused as a prediction source until its image is fully written
  and any prior consumer's barrier has completed) — this is exactly the class
  of bug ADR-0001's own two "found only by running on hardware" bugs came
  from; this will need real hardware verification before being called done,
  not just a compiles-and-runs-once check.
- `WORKSPACE_DPB_CAP = 4` is a guess pending real capability data from a
  driver that actually reports `max_dpb_slots >= 2` (the RTX 4090 reference
  machine has never been queried for this value — `query_capabilities` never
  read it before this ADR). May need revising once real numbers are in.
- Intra-refresh is explicitly left out despite being a natural pairing with
  GOP/rate-control — a caller wanting to avoid periodic IDR bandwidth spikes
  has no answer from this ADR alone.

## References

- [`docs/vulkan/roadmap.md`](../../docs/vulkan/roadmap.md) Stage 2
- [ADR-0001](0001-vulkan-video-encode-ash-probe.md) — `VideoSession<S>`
  typestate sketch this extends; AV1 driver-block this ADR excludes AV1 for
  · "What this does not prove" section (GOP/rate-control listed as deferred
  from Stage 1)
- `docs/spec/zero-cost-abstractions.md` (ADR-0009) — closed concrete types,
  no `Box`/`dyn`, `SmallVec` only where it earns its keep (not used here —
  see `Dpb` reasoning above)
- `docs/spec/caveats-and-clarity.md` (ADR-0006) — capability-fallback
  degradation must be documented on the encoder type's own rustdoc when
  implemented
- `VK_KHR_video_encode_queue` / `VK_KHR_video_encode_h264` /
  `VK_KHR_video_encode_h265` specs:
  <https://registry.khronos.org/vulkan/specs/latest/man/html/VK_KHR_video_encode_queue.html>

ADRs are **English**. Numbering is local to this `adr/` folder.
