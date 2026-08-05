# Vulkan AV1 GOP — implemented, capability-gated, **unverifiable**

- Module: `mediaway-encoder::vulkan` (`av1_gop.rs`, `av1_params.rs`,
  `session.rs`, `session_command_av1.rs`, `encoder.rs`)
- ADR: [vulkan/0002](../../../../crates/mediaway-encoder/adr/vulkan/0002-vulkan-gop-rate-control.md)
  — § "Implementation update (AV1)"
- Sibling: [vulkan-h264-gop](vulkan-h264-gop.md) /
  [vulkan-hevc-gop](vulkan-hevc-gop.md) — same design; this page only covers
  where AV1 genuinely differs and the honest unverified status

## Read this first: what "implemented" means here

This crate's AV1 **base** (key-frame-only) encode is already
hardware-verified to produce an **invalid** per-frame OBU on this crate's
reference RTX 4090 — a driver-maturity limitation, not this crate's bug (see
[ADR-0001](../../../../crates/mediaway-encoder/adr/0001-vulkan-video-encode-ash-probe.md)'s
AV1 addendum). This GOP wiring is built on top of that known-broken base, per
an explicit instruction to do so anyway — it exists so the shape and
capability gate are real, **not** because its output has been confirmed
correct. `push_seven_av1_frames_gop_or_skip` hits the same known-broken
bitstream and honestly skips, exactly like `push_three_av1_frames_or_skip`.

## Why a separate `GopState`, not shared with H.264/HEVC

`av1_gop::GopState`/`Dpb`/`DpbSlot`/`FrameDecision` mirror the other two
codecs' shape but key on `order_hint` (AV1's `StdVideoEncodeAV1PictureInfo`
has no `frame_num`/`PicOrderCnt` equivalent). `order_hint` resets to `0` at
every key frame; `order_hint_bits_minus_1` widens to `7` (AV1's spec-legal
maximum) when GOP is active — same "sidestep wraparound" reasoning as
H.264/HEVC's own widened fields, but AV1's spec caps this at 8 bits
(`order_hint` wraps mod 256) — real headroom far below H.264/HEVC's
65536-frame ceiling, an inherent format limit, not a choice.

## Reference model: one physical DPB slot == one AV1 ref-name slot

AV1's reference model is structurally wider than H.264/HEVC's (up to 7 named
reference slots, 8 physical DPB slots) — this crate keeps the same
single-forward-reference scope: only `LAST_FRAME` is ever used. One physical
`WORKSPACE_DPB_CAP`-ring slot is tied 1:1 to one AV1 virtual
reference-frame-slot number — `refresh_frame_flags = 1 << setup_slot` and
`ref_frame_idx[LAST_FRAME] == reference_name_slot_indices[0] == ref_slot`
all address the same physical/virtual slot number directly, avoiding two
independent numbering spaces.

## `primary_ref_frame` stays `PRIMARY_REF_NONE`, even for inter frames

Motion-compensated prediction still reads pixels from `LAST_FRAME` via
`ref_frame_idx` regardless of `primary_ref_frame` — that field only controls
whether this frame's CDF context carries forward from a previous frame's
adapted state. Carrying CDF state across the DPB ring would add real
bookkeeping this crate cannot verify against real hardware (the base encode
is already broken), for no provable benefit — so inter frames keep the same
`PRIMARY_REF_NONE` choice the key-frame builder already used, rather than
speculative untestable complexity.

## Capability gating (`session.rs::Capabilities`)

`supports_p_frames`'s AV1 floor check changed from an unconditional `true`
(previously skipped entirely) to a real query:
`VkVideoEncodeAV1CapabilitiesKHR::maxSingleReferenceCount >= 1` (this crate
only ever requests AV1's `SINGLE_REFERENCE` prediction mode). This gate is
real and driver-queried even though the resulting P-frame path can't be
verified end-to-end — it answers "would this driver honor the request", not
"does the resulting bitstream decode".

## CBR: not added for AV1

Same reasoning HEVC's own page gives for skipping CBR, plus one more: AV1's
base encode is already broken, so a second untestable rate-control surface
on top of an already-untestable GOP surface buys nothing provable. AV1
sessions keep `RATE_CONTROL_MODE_DISABLED` fixed-QP unconditionally.

## Result on this crate's reference hardware (2026-08-05, RTX 4090)

```
skip: packet 0's own frame data is not a valid OBU (found [ObuHeader { obu_type: 1, offset: 0 }, ObuHeader { obu_type: 0, offset: 13 }]) — known driver-maturity limitation on this hardware, same root cause as push_three_av1_frames_or_skip, see `adr/0001`'s AV1 addendum and `adr/vulkan/0002`'s AV1 follow-up section
```

Two things **are** confirmed real by this run: the session opened with
`gop_size = 3` (`Capabilities::supports_p_frames` evaluated `true` for
AV1 — the gate is genuinely live), and packet 0's `is_keyframe` cadence
matched `GopState::decide`'s prediction (pure Rust, no driver involvement)
before the skip point. The driver bug did not "not manifest" for this
shape — this is not a surprising or fixed-looking result.

## Not covered

Zero-Copy GPU input, `hasOverrides` sequence-header re-negotiation
(ADR-0001's AV1 addendum), multi-reference search / long-term references /
SVC temporal layering, intra-refresh, B-frames (permanent non-goal, all
codecs), CBR (see above). Fixing the underlying driver bug itself — out of
this crate's control, see ADR-0001's AV1 addendum for the FFmpeg
cross-check that ruled out this crate's own bitstream construction.
