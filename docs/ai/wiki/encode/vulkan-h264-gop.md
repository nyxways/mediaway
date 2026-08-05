# Vulkan H.264 GOP + rate control

- Module: `mediaway-encoder::vulkan` (`h264_gop.rs`, `h264_params.rs`,
  `session.rs`, `session_command.rs`, `encoder.rs`)
- ADR: [vulkan/0002](../../../../crates/mediaway-encoder/adr/vulkan/0002-vulkan-gop-rate-control.md)
  — design + 2026-08-05 hardware-verified implementation update
- Config: `VideoEncoderConfig::gop_size` (`1` = IDR-only, default,
  byte-identical for every existing caller) / `rate_control:
  Option<RateControlConfig>` — cross-backend fields, **only Vulkan H.264
  reads them so far** (HEVC/AV1/WMF/D3D12 untouched, stay IDR-only/fixed-QP)

## Shape

`GopState` (pure Rust, no Vulkan FFI) tracks `frame_num`/`PicOrderCnt`
(POC type 2 → `poc = 2 * frame_num`, resets to `0` at every IDR) and a fixed
`Dpb { slots: [Option<DpbSlot>; WORKSPACE_DPB_CAP], next_slot }` ring
(`WORKSPACE_DPB_CAP = 4` — one active reference + pipelining headroom, no
`SmallVec`/`Box`/`dyn`). `GopState::decide(FrameRequest::Auto)` returns
`FrameDecision { is_idr, frame_num, poc, idr_pic_id, setup_slot, reference }`
every `push_frame` call — `gop_size == 1` always returns `is_idr: true,
reference: None`, reproducing Stage 1's exact all-IDR sequence, so
`VulkanVideoEncoder` routes **every** H.264 frame through this state machine
regardless of `gop_size` rather than keeping two code paths.

`h264_params::build_frame_structs` turns a decision into the
`StdVideoEncodeH264*` structs (`PictureInfo`/`ReferenceListsInfo`/
`SliceHeader`/`ReferenceInfo`) `session_command.rs` needs. Single forward
reference only — `RefPicList0[0]` = the DPB slot index, `RefPicList1`/
B-slices never used (POC type 2 structurally can't carry B-slices, which
lines up with this ADR's own permanent B-frame exclusion).

## DPB image + one-time layout transition

GOP mode allocates the DPB image with `array_layers = min(driver
max_dpb_slots, WORKSPACE_DPB_CAP)` and a single `_2D_ARRAY` view (`baseArrayLayer`
selects the slot per use) instead of Stage 1's single-layer `_2D` image.

The `UNDEFINED -> VIDEO_ENCODE_DPB_KHR` barrier only needs to run **once**,
before the first frame — every layer is empty going in, so a bulk discard
transition is correct there, but re-running it every frame (Stage 1's
pattern, harmless for a never-read single slot) would silently blow away
already-written reference content once P-frames start reading it back.
`DpbRecordParams::transition` (`session_command.rs`) tracks this;
`record_pre_encode_barriers` swaps to a same-layout no-op barrier after
frame 1 when GOP mode is active. The default (`gop_size == 1`) path always
passes `transition: true` (matches Stage 1 exactly, byte-identical).

## `VkVideoEncodeH264DpbSlotInfoKHR` chaining

Chained onto the setup slot **and** the read reference slot, in both
`vkCmdBeginVideoCodingKHR`'s and `vkCmdEncodeVideoKHR`'s reference-slot
arrays — mirrors the AV1 addendum's own finding in [ADR-0001](../../../../crates/mediaway-encoder/adr/0001-vulkan-video-encode-ash-probe.md)
("an earlier draft left `VkVideoReferenceSlotInfoKHR` bare... AV1's
reference model needs this per FFmpeg's reference"). Worked on the first
hardware attempt here, unlike the AV1 case.

## Capability gating (`session.rs::Capabilities`)

- `supports_p_frames`: `max_dpb_slots >= 2 && max_active_reference_pictures
  >= 1 &&` (for an H.264 profile query) `max_p_picture_l0_reference_count >=
  1`. `false` → GOP silently falls back to IDR-only, no error.
- `supports_cbr`: `VkVideoEncodeCapabilitiesKHR::rateControlModes` contains
  `CBR`. `false` → falls back to fixed-QP `DISABLED`.
- `RateControlConfig::vbv_buffer_size_bytes` (bytes) is converted to
  `virtualBufferSizeInMs` (`bytes * 8 * 1000 / target_bitrate_bps`) —
  `None`/zero bitrate leaves it `0` (driver picks its own default).
- `VkVideoEncodeH264NaluSliceInfoKHR::constantQp` must be `0` whenever CBR is
  active (spec VUID) — `push_frame` branches on
  `rate_control_params.is_some()`.

## Hardware verification (2026-08-05, RTX 4090)

```
vulkan H.264 GOP VideoEncoder ok: 3 IDR + 4 P packets, cadence matched gop_size=3
vulkan H.264 GOP+CBR VideoEncoder ok: 6 packets, 394 total bytes
```

7 frames at `gop_size = 3` → real `I P P I P P I` NAL cadence (Annex-B type
5 vs type 1), stable across 3 repeated runs. Stage 1's own diagnostic
(`encode_synthetic_intra_frame`) reproduced its historical byte-exact result
unchanged (`68 bytes`, NAL offsets `4`/`15`/`23`) — zero regression to the
untouched default path. Tests: `encoder_tests.rs::push_seven_frames_gop_or_skip`,
`::push_frames_gop_with_rate_control_or_skip`.

## HEVC

Same `gop_size`/P-frame wiring landed for HEVC same-day — separate
`GopState` in `hevc_gop.rs` (simpler: no `frame_num`/`idr_pic_id`, HEVC only
needs `PicOrderCntVal`), see [vulkan-hevc-gop](vulkan-hevc-gop.md) for
detail. CBR stays H.264-only.

## Not covered

WMF/D3D12. Intra-refresh, multi-reference search, long-term references, SVC
temporal layering — permanently or provisionally out of scope per ADR-0002.
CBR rate control for HEVC (silently falls back to fixed-QP, see
[vulkan-hevc-gop](vulkan-hevc-gop.md)).
