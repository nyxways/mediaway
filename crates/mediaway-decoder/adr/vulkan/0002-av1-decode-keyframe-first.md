# ADR-0002: Vulkan Video AV1 decode — `VK_KHR_video_decode_av1`, KEY_FRAME-only first increment

- **Status**: Accepted — implemented and hardware-verified (see "Implementation addendum" near the end)
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (`src/vulkan/` module — see ADR-0021's crate-merge
  note in `adr/vulkan/0001`'s 2026-08-05 addendum; there is no separate
  `mediaway-decoder-vulkan` crate today)

## What this pass does and does not do

This ADR is **design only**. No `av1_params.rs` / `av1_refs.rs` /
`decoder_av1.rs` / `session_command_av1.rs` exists yet, and `session.rs`'s
`DecodeProfile` enum gains no `Av1` variant this pass. Everything below a
struct/function name is a plan, checked against real evidence where stated,
not compiled or run.

## Execution environment note (read before trusting any "confirmed" claim below)

This session has **read/glob/grep/write/edit + web search/fetch** tools but
**no shell/Bash tool** — a tighter constraint than `adr/vulkan/0001`'s own
authoring session, which at least had web access to check `docs.rs` pages one
at a time (the same tier this session also used). Concretely, this session
could **not**:

- Run `cargo check` / `cargo doc` / `cargo test` against this crate or
  `vulkanalia`'s vendored `vulkanalia-sys` source for real
  `StdVideoDecodeAV1*` field names (the workspace's real, ground-truth source
  of these struct definitions per `adr/vulkan/0001`'s own evidence-tier
  discipline).
- Query the test machine (RTX 4090 + Intel UHD 770) for anything not already
  recorded in a prior addendum.

What this session **did** do: read every file this design reuses/adapts
directly (cited by path and line number throughout), and fetched the
Khronos `VK_KHR_video_decode_av1` proposal document plus two `docs.vulkan.org`
struct reference pages (tier: web fetch of the **official extension proposal
and reference pages**, not `docs.rs`, not `cargo check` — a different and, for
struct-*shape* claims, arguably stronger source than `adr/vulkan/0001`'s
`docs.rs`-only tier, since the proposal doc explains *why* fields exist, not
just that they exist — but still not a compile-verified claim about this
workspace's own pinned `vulkanalia` version's exact generated field names).
Every claim below is labeled by which of these tiers it rests on, per this
workspace's own honesty convention.

## Context

`adr/vulkan/0001` (Accepted, 2026-07-29, multiple addenda through 2026-08-05)
already designed and then implemented Vulkan Video decode for H.264 and HEVC
in this same crate:

- **H.264**: general P-slice GOP, **hardware-verified** on the RTX 4090 (real
  motion-compensated `P_Skip` reference read, sliding-window DPB) — see that
  ADR's 2026-07-30 addenda.
- **HEVC**: VPS/SPS/PPS + slice-segment-header + short-term RPS parsing are
  real, sans-io, and unit-tested, but the real GPU decode path
  (`decoder_hevc.rs::decode_slice_hevc`, read directly,
  `crates/mediaway-decoder/src/vulkan/decoder_hevc.rs:223-238`) only reaches a
  real `vkCmdDecodeVideoKHR` call for **IDR pictures** — a P/B-slice HEVC NAL
  is rejected with `DecodeError::Unsupported`, an explicit, honest scope-down
  (not an oversight), hardware-verified for that IDR-only scope as of the
  2026-08-05 addendum.
- **AV1**: not started. `mod.rs`'s own module doc (`src/vulkan/mod.rs:1-5`,
  read directly) states this explicitly: *"AV1 (`adr/0001`'s wider design
  scope) remains explicit follow-up work — no `av1_params.rs` exists yet."*

This ADR is that follow-up, narrowed to a first real increment rather than
`adr/vulkan/0001`'s original (pre-implementation) "general GOP from the start"
aspiration for all three codecs — see § Scope decision below for why.

### Sibling AV1 encode experience in this same workspace (directly relevant context)

`mediaway-encoder`'s own Vulkan AV1 **encode** path (`VK_KHR_video_encode_av1`,
a related but distinct extension family) is the single most informative piece
of context for scoping this ADR, read directly:

- `crates/mediaway-encoder/src/vulkan/av1_params.rs` and `av1_gop.rs` (both
  read in full this session) construct `StdVideoAV1*`/`StdVideoEncodeAV1*`
  structs for one all-intra `KEY_FRAME` (`av1_params.rs:1-511`) and, per a
  later addendum, real single-forward-reference `INTER_FRAME` GOP
  (`av1_params.rs:513-650`, `av1_gop.rs:1-193`).
- Getting even the **`KEY_FRAME`-only** base case right took multiple real,
  hardware-verified bug fixes: an earlier draft's `reduced_still_picture_header`
  produced an invalid bitstream (`av1_params.rs:146-156`'s doc), an earlier
  draft's `disable_cdf_update`/`disable_frame_end_update_cdf = 1` also
  produced an invalid bitstream (`av1_params.rs:422-433`'s doc), and null
  `pSegmentation`/`pLoopFilter`/`pCDEF`/`pLoopRestoration`/`pGlobalMotion`/
  `pExtensionHeader` pointers were a third real bug (same doc) — all found by
  comparing field-by-field against FFmpeg's real, hardware-tested
  `vulkan_encode_av1.c`, the same technique that found H.264/HEVC **decode**'s
  own real bugs in `adr/vulkan/0001`'s addenda.
- After all of those fixes, this workspace's own AV1 Vulkan encode is **still
  not producing valid output** — confirmed via `crates/mediaway-encoder/adr/vulkan/0002-vulkan-gop-rate-control.md:308-323`
  (read directly): `push_seven_av1_frames_gop_or_skip`'s real 2026-08-05 run on
  the RTX 4090 reports *"packet 0's own frame data is not a valid OBU"* even
  though the Vulkan session itself opens successfully and the GOP-cadence
  state machine (this crate's own code, no driver involvement) behaves
  correctly. This is characterized in that ADR as a **known driver-maturity
  limitation**, cross-checked independently: this session's own persistent
  memory (and the same ADR's context) records that this was re-confirmed
  byte-for-byte against `ffmpeg`'s own `av1_vulkan` encoder failing `dav1d`
  decode on the same GPU/driver — i.e. not just this crate's own
  interpretation of an ambiguous result.

**What this does and does not mean for this ADR**: `VK_KHR_video_encode_av1`
and `VK_KHR_video_decode_av1` are **different Vulkan extensions** — a
demonstrated encode-side driver bug is not evidence the decode side shares it.
This ADR does **not** assume AV1 decode is equally broken. It **does** treat
"this driver generation's AV1 Vulkan Video support (across both directions)
has a real, confirmed track record of needing several real bug-fix rounds even
for the simplest possible frame type before hitting a genuine wall" as
directly relevant risk context for the scope decision below, per this task's
own explicit instruction not to silently ignore it.

### AV1 decode queue family: already confirmed present on this workspace's reference hardware

`probe.rs`'s `probe_video_decode_queue_families` (read directly,
`src/vulkan/probe.rs:38-41`) already queries
`VK_VIDEO_CODEC_OPERATION_DECODE_AV1_BIT_KHR` alongside H.264/H.265, and
`adr/vulkan/0001`'s 2026-07-30 addendum recorded the **real** result of
running it on this workspace's reference machines:

```text
vulkan device: NVIDIA GeForce RTX 4090 (DISCRETE_GPU)
  h264_decode_queue_family=Some(3) h265_decode_queue_family=Some(3) av1_decode_queue_family=Some(3)
vulkan device: Intel(R) UHD Graphics 770 (INTEGRATED_GPU)
  h264_decode_queue_family=Some(1) h265_decode_queue_family=Some(1) av1_decode_queue_family=Some(1)
```

**Both** reference GPUs already advertise an AV1 decode queue family — this
ADR's Stage 0 risk (the single biggest unknown `adr/vulkan/0001` flagged for
H.264/HEVC decode) is **already closed for AV1 too**, for free, by that
earlier run. This is real, hardware-confirmed evidence (tier: real
`cargo test ... -- --nocapture` run, the strongest tier this workspace uses),
not an assumption.

## Scope decision: KEY_FRAME-only decode, no film grain, is this ADR's first increment

**Decision: this ADR designs AV1 decode scoped to `frame_type == KEY_FRAME`
only** (rejecting `INTER_FRAME` / `INTRA_ONLY_FRAME` / `SWITCH_FRAME` /
`show_existing_frame == 1` with `DecodeError::Unsupported`), **film grain
disabled** (`filmGrainSupport = VK_FALSE` on the decode profile) — mirroring
HEVC's own honest IDR-only precedent (`adr/vulkan/0001`'s 2026-07-30 addendum)
rather than H.264 decode's completed general-GOP scope. This is an explicit,
justified call, not a default:

1. **AV1's reference model is strictly more complex than HEVC's, which itself
   is not yet hardware-verified for P/B slices in this crate.** Per this
   task's own framing and the AV1 spec (aomediacodec.github.io/av1-spec
   § 7.20, cross-checked against `av1_gop.rs`'s own doc,
   `crates/mediaway-encoder/src/vulkan/av1_gop.rs:21-32`, read directly): up
   to `STD_VIDEO_AV1_REFS_PER_FRAME = 7` simultaneous named references
   (vs. H.264's single-list sliding window or HEVC's before/after RPS split),
   plus warped motion, OBMC, compound prediction, and CDF forward-adaptation
   (`primary_ref_frame`) as tools a *general*-GOP AV1 stream may legally use.
   Building this crate's first AV1 reference-management design directly at
   general-GOP complexity — on top of a codec whose sibling HEVC decode
   effort *itself* chose to stage IDR-only first — would stack two full
   tiers of unverified new complexity (parser + reference model) rather than
   isolating one at a time, the same reasoning that produced HEVC's own
   scope-down.
2. **This driver generation's demonstrated AV1 Vulkan Video track record
   (both directions) argues for isolating variables, not stacking them.** See
   § Sibling AV1 encode experience above — even `KEY_FRAME`-only encode needed
   several real bug-fix rounds before hitting a confirmed driver wall. This is
   not proof decode shares the bug (a different extension, genuinely
   untested), but it is real, directly relevant evidence that this specific
   codec/extension family in this specific driver generation has a
   below-H.264/HEVC-average maturity bar — a reason to start at the
   *narrowest* legal AV1 picture (`KEY_FRAME`, no references, no film grain)
   and expand only once that is confirmed working, exactly mirroring how
   H.264 decode's own real bug-hunt (`adr/vulkan/0001`'s 2026-07-30 addendum)
   started from a hand-crafted **single IDR frame** before adding a P-frame.
3. **Film grain is a structural, not just a "do it later," exclusion this
   round.** This session's own WebFetch of the Khronos `VK_KHR_video_decode_av1`
   proposal document (tier: official extension proposal, cited in full in
   § Struct/extension survey below) found a concrete, previously-unconfirmed
   (by `adr/vulkan/0001`) fact: enabling film grain support
   (`VkVideoDecodeAV1ProfileInfoKHR::filmGrainSupport = VK_TRUE`) forces
   **`DISTINCT`** DPB/output image mode for any decode operation that applies
   grain, because the grain-synthesized output picture must differ from the
   grain-*free* picture stored in the DPB for future reference. This crate's
   **entire existing architecture requires `DPB_AND_OUTPUT_COINCIDE`**
   (`session.rs::query_capabilities`, read directly,
   `src/vulkan/session.rs:417-422`, rejects any driver/profile combination
   that does not advertise coincide via
   `VulkanDecodeError::SeparateReferenceImagesRequired`) — so film grain is
   not merely deferred work, it is **incompatible with this crate's current
   single-combined-image design** and would need a real, separate `DISTINCT`-mode
   image-management path to ever support, independent of frame-type scope.
   Requesting `filmGrainSupport = VK_FALSE` this round is therefore not just
   the simpler choice, it is the only choice this crate's existing image
   model can make.
4. **Test-bootstrap asymmetry exists, but does not by itself force this
   scope** — noted for fairness, not as the deciding factor. Unlike H.264
   (hand-crafted `I_PCM`/`P_Skip` Annex-B bytes) and HEVC (bootstrapped its
   hardware test via this workspace's own already-hardware-verified Vulkan
   HEVC **encoder**, `adr/vulkan/0001`'s 2026-07-30 addendum), this crate has
   **no hardware-verified Vulkan AV1 encoder** to lean on (see § Sibling AV1
   encode experience). It **does** have a real, independent, already-working
   alternative: `mediaway-sw::av1::Av1Encoder`, a pure-Rust, BSD-2-Clause
   `rav1e`-backed **software** AV1 encoder (`crates/mediaway-sw/src/av1.rs:1-60`,
   read directly; already a regular workspace dependency of this crate per
   `Cargo.toml:20`, no new dependency). `rav1e` is a complete, competent AV1
   encoder — unlike H.264/HEVC's hardware-test bootstrap problem, **AV1 could
   plausibly get real, valid `INTER_FRAME` test material from `rav1e` even for
   a general-GOP first increment**, sidestepping the Vulkan AV1 encode driver
   bug entirely (a genuinely different code path — CPU software encode, not
   the broken GPU extension). This is flagged explicitly as a reason the
   scope decision above rests primarily on reasons 1–3 (parser/reference-model
   engineering surface, driver-maturity risk isolation, film-grain
   architecture incompatibility), not on "AV1 general-GOP is untestable" —
   see § Test plan below for how `Av1Encoder` is still put to real use this
   round, at `KEY_FRAME`-only scope.

### `show_existing_frame` — a genuine simplification this scope gets for free

This session's WebFetch of the Khronos proposal doc found: *"Such frame OBUs
do not contain any actual payload that is relevant to implementations"* —
`show_existing_frame == 1` pictures need **no** `vkCmdDecodeVideoKHR` call at
all; they only reference an already-decoded picture already resident in a DPB
slot. This crate's `KEY_FRAME`-only scope makes this a non-issue in practice
(a real encoder has no reason to emit `show_existing_frame` in a keyframe-only
stream), but it is worth recording now: a future general-GOP increment must
route `show_existing_frame` OBUs to a "no decode, just re-output an existing
slot's stored frame" path, not through `record_and_submit_av1` at all — a
structurally different code path from every H.264/HEVC NAL type this crate
has handled so far, none of which have an "output without decoding" case.

## Reference-model design: AV1's fixed 8-slot array, not a port of `dpb.rs`

Per this task's own explicit instruction, `dpb.rs`'s `Dpb`/`DpbSlot`
(`src/vulkan/dpb.rs:33-75`, read directly: `frame_num`/`frame_num_wrap`/
`pic_order_cnt`/`used_for_reference` fields, sliding-window eviction) is
**not** reusable for AV1 — none of those fields have any AV1 meaning, and
AV1's reference-update process (spec § 7.20) is not a sliding window at all.
This session's WebFetch of the Khronos proposal doc confirms the real Vulkan
decode-side reference model (tier: official proposal doc, quoted directly):

> "for a given AV1 reference name `frame` [...] the corresponding DPB slot
> index is specified in `referenceNameSlotIndices[frame -
> STD_VIDEO_AV1_REFERENCE_NAME_LAST_FRAME]`" — a **direct** AV1
> reference-name → Vulkan DPB slot mapping, bypassing AV1's own "virtual
> buffer index" abstraction entirely (the proposal's own stated design
> choice, "option (3)" in its alternatives analysis).

> "reference picture setup is requested and the DPB slot [...] is activated
> [...] if and only if `StdVideoDecodeAV1PictureInfo::refresh_frame_flags` is
> not zero" — a key frame's `refresh_frame_flags == 0xFF` (per
> `crates/mediaway-encoder/src/vulkan/av1_params.rs:489`'s already-confirmed
> encode-side convention, the AV1 spec's own key-frame requirement) means
> **every** decoded key frame is a setup-slot picture.

**Real, load-bearing simplification for this round's `KEY_FRAME`-only scope**:
because a key frame never *reads* any reference (all `referenceNameSlotIndices`
entries are `-1`), this round's Vulkan-level reference bookkeeping needs
**no** AV1-specific metadata (no `order_hint`, no `frame_type` per slot) at
all — only "is this Vulkan DPB slot occupied, and does a caller still hold an
outstanding Zero-Copy handle into it," the exact same backpressure contract
`dpb.rs` already established for H.264/HEVC
(`dpb.rs::DpbError::SlotOutstanding`, `src/vulkan/dpb.rs:108-119`). A new,
purpose-built, minimal type — not `dpb.rs`'s `DpbSlot`, and not (yet) a real
AV1 `order_hint`-tracking type either, since this round never needs one:

```rust
// src/vulkan/av1_refs.rs (new file — plan, no code this pass)

/// AV1's fixed 8-slot reference-frame-buffer array (AV1 spec §7.20) — NOT a
/// port of `dpb.rs`'s H.264/HEVC-shaped `Dpb`/`DpbSlot` (no sliding window,
/// no frame_num/POC, no RPS — see this ADR's own "Reference-model design"
/// section for why). This round (KEY_FRAME-only) never *reads* a reference
/// slot (every referenceNameSlotIndices entry is always -1), so this type
/// tracks only Vulkan-level slot occupancy + outstanding-Zero-Copy-handle
/// bookkeeping — structurally mirroring dpb.rs's SlotOutstanding contract,
/// not sharing its code (that code is coupled to H.264-specific fields this
/// type has no use for). A future general-GOP increment adds real
/// order_hint/frame_type-per-slot tracking here (mirroring
/// mediaway-encoder::vulkan::av1_gop::DpbSlot's decode-shaped sibling) —
/// deliberately not built this round, since nothing in a KEY_FRAME-only
/// decoder ever reads it.
pub(crate) struct Av1RefSlots {
    occupied: [bool; 8],
    outstanding: [bool; 8],
}
```

The **physical** Vulkan DPB image this round needs is small regardless of the
"8 reference names" logical space: per the proposal doc, "multiple AV1
reference names may refer to the same DPB slot" — so a key-frame-only stream,
where a single picture is (logically) every one of the 8 reference names at
once, needs only **1–2 physical `dpb_slot_count` array layers** (current
picture + optionally one prior, mirroring `decoder_hevc.rs::build_session_hevc`'s
own `(sps.max_dec_pic_buffering + 1).max(1)` sizing choice for its own
IDR-only scope, `src/vulkan/decoder_hevc.rs:138-145`) — not 8. This is a real,
concrete simplification worth stating explicitly so a future implementer does
not over-allocate.

## Struct/extension survey (this session's real findings)

Building on `adr/vulkan/0001`'s own binding survey (`docs.rs` tier, already
confirmed `VideoDecodeAV1PictureInfoKHR`/`VideoDecodeAV1DpbSlotInfoKHR`/
`StdVideoDecodeAV1PictureInfo`/`StdVideoDecodeAV1ReferenceInfo` are present in
`vulkanalia` 0.35.0 — see that ADR's table, `adr/vulkan/0001:92-94`), this
session's WebFetch of the official Khronos proposal + `docs.vulkan.org`
reference pages adds real, cited field-level detail that ADR did not have:

| Struct | Fields (this session's own fetch, cited above) |
|---|---|
| `VkVideoDecodeAV1ProfileInfoKHR` | `stdProfile: StdVideoAV1Profile`, `filmGrainSupport: VkBool32` — this round: `STD_VIDEO_AV1_PROFILE_MAIN`, `VK_FALSE` |
| `VkVideoDecodeAV1CapabilitiesKHR` | `maxLevel: StdVideoAV1Level` — chained into `query_capabilities` the same way `h264_caps`/`hevc_caps` already are (`session.rs:393-401`, read directly) |
| `VkVideoDecodeAV1SessionParametersCreateInfoKHR` | **`pStdSequenceHeader: const StdVideoAV1SequenceHeader*`** — a **single pointer**, not an `AddInfoKHR` + `max_std_*_count` list shape like `create_session_parameters_h264`/`_hevc` (`session.rs:619-681`, read directly). See § Session-parameters lifecycle below — this is a real, structural difference this ADR's `create_session_parameters_av1` must account for. |
| `VkVideoDecodeAV1PictureInfoKHR` | `pStdPictureInfo: const StdVideoDecodeAV1PictureInfo*`, `referenceNameSlotIndices: int32_t[7]`, `frameHeaderOffset: uint32_t`, `tileCount: uint32_t` (must be `> 0`), `pTileOffsets: const uint32_t*`, `pTileSizes: const uint32_t*` — **no `slice_offsets`-style single array**; AV1 addresses tiles, not slices, and additionally needs a separate `frameHeaderOffset` H.264/HEVC have no equivalent of (their picture-info structs have no header/slice split within one buffer range — see § Bitstream framing below). |
| `VkVideoDecodeAV1DpbSlotInfoKHR` | `pStdReferenceInfo: const StdVideoDecodeAV1ReferenceInfo*` — structurally identical shape to `VideoDecodeH264DpbSlotInfoKHR`/`VideoDecodeH265DpbSlotInfoKHR` (`session_command_h264.rs:239-241`'s `std_reference_info` pattern is directly analogous). |

**Not independently re-confirmed this session** (relies on `adr/vulkan/0001`'s
own `docs.rs`-tier confirmation, one level removed): the exact field list
*inside* `StdVideoDecodeAV1PictureInfo`/`StdVideoDecodeAV1ReferenceInfo`
(flag-bit names, `ref_frame_idx`/`ref_order_hint` array shapes) — that ADR's
own table describes them only in general terms ("frame type, order hints, ref
sign bias, saved order hints, tile/quant/segmentation/filter pointers"). This
ADR's own `build_key_frame_picture_info` design (below) uses the
**encode-side** `StdVideoEncodeAV1PictureInfo`'s already-confirmed,
real, hardware-cross-checked field names
(`crates/mediaway-encoder/src/vulkan/av1_params.rs:482-511`, read directly:
`flags`, `frame_type`, `order_hint`, `primary_ref_frame`,
`refresh_frame_flags`, `ref_order_hint`, `ref_frame_idx`,
`pQuantization`/`pSegmentation`/`pLoopFilter`/`pCDEF`/`pLoopRestoration`/
`pGlobalMotion`/`pExtensionHeader`, all never-null) as a **naming-convention
proxy** for the decode-side struct, not a confirmed identical struct — decode
and encode versions of "the same" AV1 picture info are separate Vulkan types
(`StdVideoDecodeAV1PictureInfo` vs. `StdVideoEncodeAV1PictureInfo`), and
H.264/HEVC decode already diverged from their encode counterparts in a real,
load-bearing way (`adr/vulkan/0001`'s 2026-07-30 addendum: H.264 decode has
**no `pRefLists`-equivalent field at all**, unlike encode's
`StdVideoEncodeH264PictureInfo::pRefLists`). This is flagged as an explicit
open question below, not assumed.

## Bitstream framing: AV1 has no Annex-B start codes — a genuine H.264/HEVC divergence

Every existing decode path in this crate (`decode_slice_h264`,
`decode_slice_hevc`) prepends a literal `[0x00, 0x00, 0x01]` Annex-B start
code before the uploaded NAL bytes — a real, hardware-confirmed requirement
for those two codecs (`decoder.rs:452-462`, read directly, citing the exact
FFmpeg-comparison finding that discovered it). **AV1 has no Annex-B framing at
all** — OBUs use their own `obu_header()` + (usually) `leb128`-coded
`obu_size` framing (AV1 spec § 5.2/§ 5.3), which this workspace already has a
**test-only, private** precedent for:
`crates/mediaway-encoder/src/vulkan/nal.rs::scan_obu_headers`/`read_leb128`
(confirmed present via Grep this session, `#[cfg(test)]`-gated per
`adr/vulkan/0001`'s own earlier finding — read only the function signatures
this session, not adapted or imported, per that ADR's own note that this code
is "duplicated, not imported" if a future ADR wants a shared OBU-framing
helper). This ADR's `av1_params.rs` needs its **own**, real (non-test-only)
`obu_header()`/`leb128()` scanner — the shape (obu_type in bits 6..3 of the
header byte, optional `leb128`-coded size field) is a small, already-proven
pattern in this exact workspace, just not one this ADR can literally import.

**`frameHeaderOffset`'s real implication**: `VkVideoDecodeInfoKHR::src_buffer`
for AV1 is expected to hold the **frame OBU's raw bytes** (matching how
H.264/HEVC hand the driver the raw NAL payload), with `frameHeaderOffset`
telling the driver where the *frame header* portion starts within that range
(distinct from `pTileOffsets`/`pTileSizes`, which locate the tile-group
payload that follows). For an `OBU_FRAME` (the common case: one OBU carries
both `frame_header_obu()` and `tile_group_obu()` back-to-back, AV1 spec
§ 5.10), this crate's own sans-io frame-header parser must track **exactly**
how many bits/bytes `uncompressed_header()` consumed (through its own
`byte_alignment()` per spec § 5.9.2) to compute where the tile-group payload
begins — a real, nontrivial bit-position-tracking requirement this ADR flags
as an **open item**, not a solved design (see § Open questions). This is
structurally analogous to (but a real, new instance of) the same "parse just
enough of the bitstream ourselves, hand the driver the rest" pattern
`h264_params.rs`/`hevc_params.rs` already establish (per this task's own
framing) — this crate's own parser stops at `uncompressed_header()`'s end, it
does not parse `tile_group_obu()`'s own contents, but it **does** need that
one boundary byte offset, which H.264/HEVC's slice-header parsers never
needed (their own "boundary" is simply "the whole NAL," addressed by
`slice_offsets = [0]`).

## Session-parameters lifecycle: one sequence header per object, no add/max-count shape

`create_session_parameters_h264`/`_hevc` (`session.rs:619-681`, read directly)
both build a `*SessionParametersAddInfoKHR` (slices of SPS/PPS/VPS) plus a
`*SessionParametersCreateInfoKHR` with `max_std_*_count` fields — because
H.264/HEVC session parameters objects are **id-indexed lists** a stream can
add to over its lifetime. Per § Struct/extension survey above,
`VkVideoDecodeAV1SessionParametersCreateInfoKHR` has **no** add-info/max-count
shape at all — just one `pStdSequenceHeader` pointer, because (per this
session's WebFetch) *"AV1 lacks sequence identifiers"* the way H.264/HEVC's
`seq_parameter_set_id` provides. **This round's own scope cut — one sequence
header per session, matching H.264/HEVC's existing "no mid-stream
parameter-set change" cut — happens to make this a non-issue in practice**
(this crate never needs to add a second sequence header to an existing
parameters object either way), but `create_session_parameters_av1`'s function
*shape* is genuinely simpler than its H.264/HEVC siblings as a direct
consequence of AV1's own design, not a scope choice this crate is making —
worth recording so a future reader does not assume it was simplified by
choice.

## `DecodeProfile`/`query_capabilities`/`query_video_format`: from 2-way to 3-way dispatch

`session.rs::DecodeProfile` (`session.rs:36-41`) is already an enum with two
variants (`H264`/`Hevc`) specifically so a third could be added later without
restructuring callers (`session.rs`'s own doc says so directly,
`session.rs:29-35`). Adding `Av1(vk::VideoDecodeAV1ProfileInfoKHR)` is
therefore additive to the enum itself, but two functions this session read
directly currently branch on a **boolean** `is_hevc`, not a full match, and
will need real (small, mechanical) changes:

- `query_capabilities` (`session.rs:379-432`): `let is_hevc = matches!(profile,
  DecodeProfile::Hevc(_));` (`session.rs:391`) then a two-way `if is_hevc {...}
  else {...}` chaining either `hevc_caps`/`h264_caps` — must become a 3-way
  match chaining `av1_caps: vk::VideoDecodeAV1CapabilitiesKHR` for the new
  variant, and the `filmGrainSupport`/`DPB_AND_OUTPUT_COINCIDE` interaction
  from § Scope decision must be checked here (a driver could theoretically
  report `COINCIDE` unavailable specifically when `filmGrainSupport = TRUE`
  even though this ADR always requests `FALSE` — worth a defensive comment
  when implemented, not a functional gap since `FALSE` is always requested).
- `decoder.rs::VulkanVideoDecoder::open` (`decoder.rs:186-192`): `let is_hevc =
  config.codec == CodecKind::Hevc;` then branches `find_hevc_decode_device`
  vs. `find_h264_decode_device` — needs a real 3-way (or `match`-based)
  dispatch once `CodecKind::Av1` (already present,
  `crates/mediaway-common/src/lib.rs:54`, confirmed via Grep this session) is
  accepted.

Flagged explicitly so implementation does not silently break H.264/HEVC's
existing boolean-shaped branches while bolting AV1 on as a third `if`.

## File layout plan (design only — no file below exists yet)

Extends `adr/vulkan/0001`'s own original file-layout sketch
(`adr/vulkan/0001:351-354` already named `av1_params.rs`/`av1_refs.rs` as
placeholders) with the real shape this ADR designs:

```text
src/vulkan/av1_params.rs      — OBU scan (obu_header/leb128, new/real, not
                                 test-only, adapted from the *shape* of
                                 mediaway-encoder::vulkan::nal.rs's private
                                 test helpers per that ADR's own "duplicated,
                                 not imported" note); sequence_header_obu()
                                 parse -> Av1SequenceHeader; KEY_FRAME-only
                                 uncompressed_header() parse -> Av1FrameHeader
                                 (tracking its own end-bit-position for
                                 frameHeaderOffset/tile-group boundary — see
                                 § Bitstream framing); StdVideoAV1SequenceHeader
                                 + StdVideoDecodeAV1PictureInfo construction
                                 (`to_std`-shaped, mirroring h264_params.rs's/
                                 hevc_params.rs's identical pattern)
src/vulkan/av1_refs.rs        — Av1RefSlots: Vulkan-level slot
                                 occupancy/outstanding-handle bookkeeping only
                                 this round (see § Reference-model design —
                                 deliberately not dpb.rs-shaped, deliberately
                                 not yet order_hint-tracking either, since
                                 nothing this round reads a reference)
src/vulkan/decoder_av1.rs     — Av1Session (mirrors H264Session/HevcSession's
                                 shape exactly: resources/command_buffer/
                                 coded_extent/bitstream_alignment/sequence
                                 header, no dpb: Dpb field — uses Av1RefSlots
                                 instead); build_session_av1; decode_frame_av1
                                 (KEY_FRAME dispatch; rejects
                                 INTER_FRAME/INTRA_ONLY_FRAME/SWITCH_FRAME/
                                 show_existing_frame==1 as
                                 DecodeError::Unsupported, mirroring
                                 decoder_hevc.rs's own P/B-slice rejection
                                 shape at src/vulkan/decoder_hevc.rs:236-238);
                                 scan_parameter_sets (finds the first
                                 sequence-header OBU, mirrors
                                 decoder_hevc.rs::scan_parameter_sets);
                                 push_packet_av1 (mirrors
                                 decoder_hevc.rs::push_packet_hevc's free-
                                 function-not-trait-method shape,
                                 src/vulkan/decoder_hevc.rs:382-421)
src/vulkan/session_command_av1.rs — build_picture_info (StdVideoDecodeAV1PictureInfo,
                                 mirrors session_command_h264.rs:40-67's/
                                 session_command_hevc.rs's shape);
                                 record_and_submit_av1 (mirrors
                                 record_and_submit_h264's exact sequence:
                                 reset_session_once reused as-is [codec-
                                 generic already], slotIndex=-1 setup-slot-in-
                                 begin-scope protocol reused as-is [Vulkan-
                                 level protocol, not codec-specific — the
                                 exact bug class adr/vulkan/0001's H.264
                                 addendum found and fixed applies identically
                                 here], VIDEO_DECODE_DST_KHR layout transition
                                 reused as-is [also codec-generic];
                                 AV1-specific: no Annex-B start code prepended
                                 — see § Bitstream framing — src_buffer holds
                                 raw OBU bytes; VkVideoDecodeAV1PictureInfoKHR's
                                 referenceNameSlotIndices=[-1;7]/frameHeaderOffset/
                                 tileCount/pTileOffsets/pTileSizes instead of
                                 H.264/HEVC's slice_offsets)
```

**Reused as-is, zero new code needed** (confirmed by reading each file this
session — every one is already codec-generic over `&mut DecodeProfile`/
`SessionResources`, not H.264/HEVC-specific): `session_command.rs` (all of
`SessionResources`, `create_command_pool`, `create_host_buffer`,
`create_dpb_image`, `submit_and_wait`, `transition_dpb_image_once`,
`color_range_for_layer`, `upload_to_host_memory` — `session_command.rs:1-447`,
read in full), `cpu_readback.rs` (`read_nv12`/`nv12_byte_size`, already reused
verbatim by `decoder_hevc.rs:297-304`/`192`), `zero_copy.rs`
(`build_handle`, already reused verbatim by `decoder_hevc.rs:314-315`). This
is a real, positive finding: AV1's own new-code surface is smaller than
H.264/HEVC's each was, since the codec-generic plumbing those two already
built out is directly reusable.

`session.rs` gains (not a new file): `DecodeProfile::Av1(vk::VideoDecodeAV1ProfileInfoKHR)`
variant + `DecodeProfile::new_av1()`, `create_session_parameters_av1`
(single-pointer shape, see § Session-parameters lifecycle),
`find_av1_decode_device` (thin wrapper mirroring `find_h264_decode_device`/
`find_hevc_decode_device`, `session.rs:310-320`), the 3-way `query_capabilities`/
`query_video_format` changes from § `DecodeProfile` dispatch above.
`decoder.rs`'s `DecodedSession` enum (`decoder.rs:113-116`) gains
`Av1(Av1Session)`. `mod.rs` gains `mod decoder_av1; pub mod av1_params; pub
mod av1_refs; mod session_command_av1;` plus doc-comment updates.

Sibling `*_tests.rs` per file, per this workspace's testing convention —
`av1_params.rs`'s OBU/sequence-header/frame-header parsing is pure, sans-io,
and unit-testable without any Vulkan device, the same way `h264_params.rs`/
`hevc_params.rs` already are.

## Error handling: reuse `DecodeError`, one new crate-internal `Av1ParamError`

Mirrors `adr/vulkan/0001`'s decision exactly: `crate::DecodeError`'s five
variants (`Unsupported`/`NoBackend`/`InvalidInput`/`Backend`/`Closed`) remain
sufficient — no new public variant. A new crate-internal
`Av1ParamError` (mirroring `H264ParamError`/`HevcParamError`'s own
`#[derive(Debug, Error, Clone, PartialEq, Eq)] #[non_exhaustive]` shape,
`h264_params.rs:38-51`) wraps OBU/sequence-header/frame-header parse failures
and unsupported-syntax rejections (`INTER_FRAME`/`INTRA_ONLY_FRAME`/
`SWITCH_FRAME`/`show_existing_frame`/`frame_id_numbers_present_flag == 1`/
non-Main-profile/`filmGrainSupport` requests), `#[from]`-wrapped into
`VulkanDecodeError::Av1Bitstream` (a new variant alongside the existing
`Bitstream`/`HevcBitstream`, `session.rs:180-186`), mapped to
`DecodeError::InvalidInput` in `decoder.rs::map_err`'s existing match arm
grouping (`decoder.rs:703-706`, which already groups `Bitstream`/
`HevcBitstream`/`UnsupportedResolution`/`MissingParameterSet` together — `Av1Bitstream`
joins that same arm).

## Test plan

**Sans-io tests (no GPU, no risk)**: `av1_params.rs`'s OBU scanner + sequence-
header + `KEY_FRAME` frame-header parser are unit-testable against hand-built
byte fixtures the same way `h264_params.rs`/`hevc_params.rs` already are —
this is the highest-value, lowest-risk coverage this ADR's implementation can
get before touching any hardware, per this workspace's own established
pattern.

**Hardware-gated test — real bitstream via `mediaway-sw::av1::Av1Encoder`,
not a hand-crafted OBU and not the driver-blocked Vulkan AV1 encoder**: this
ADR's own § Scope decision reason 4 already names the mechanism — `rav1e`
via `mediaway-sw::av1::Av1Encoder` (`crates/mediaway-sw/src/av1.rs`, already
a regular dependency, no new `[dev-dependencies]` entry needed since it is
already a direct dependency per `Cargo.toml:20`) encodes one real flat-color
`I420` frame, producing a real, driver-independent (pure-CPU) AV1
`OBU_SEQUENCE_HEADER` + `OBU_FRAME` (`KEY_FRAME`) bitstream — structurally
the same "bootstrap the hardware test from a real, independently-verified
encoder elsewhere in this workspace" pattern `hardware_hevc_decode.rs`
already established (`adr/vulkan/0001`'s 2026-07-30 addendum), just using
`mediaway-sw`'s software encoder instead of a hardware-verified Vulkan one,
since (per § Sibling AV1 encode experience) no such Vulkan AV1 encoder exists
in this workspace yet. Real caveat: `rav1e`'s own exact OBU-level output
shape (whether it emits `OBU_FRAME` or separate `OBU_FRAME_HEADER`+
`OBU_TILE_GROUP`, whether it sets `reduced_still_picture_header` for a
single-frame session, exact `operating_points`/`enable_order_hint` choices)
is **not confirmed this session** — implementation must inspect `rav1e`'s
real output bytes directly (the same "instrumented byte dump" technique
`adr/vulkan/0001`'s addenda repeatedly used) before assuming any specific
shape, and this crate's own `KEY_FRAME`-only parser must be permissive enough
to accept whatever `rav1e` actually emits for a one-frame encode, or the test
bootstrap itself becomes the first thing to debug.

**Real risk this section must flag, per this task's own explicit
instruction**: real-hardware verification of `record_and_submit_av1` may hit
the **same or a sibling driver wall** the AV1 Vulkan **encode** side already
hit (§ Sibling AV1 encode experience) — this is **unconfirmed either way**.
If it does, this ADR's own hardware test must follow `hardware_hevc_decode.rs`'s
established "soft-skip loudly, never hard-fail the default suite for a real,
not-yet-root-caused bug" convention (`adr/vulkan/0001`'s 2026-07-30 HEVC
addendum, "Honest status (current)" section) rather than a hard assertion,
until/unless a root cause is found and fixed the way both H.264's and HEVC's
own all-zero-output bugs eventually were.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| General-GOP AV1 decode from the start (matching H.264 decode's own completed scope, and `adr/vulkan/0001`'s original pre-implementation aspiration for all three codecs) | Rejected for this increment: AV1's reference model (up to 7 simultaneous named references, warped motion, CDF forward-adaptation) is strictly more complex than HEVC's own RPS, which this crate's own HEVC decode has *not yet* hardware-verified past IDR. Stacking full reference-model complexity on top of an already-immature-on-this-driver codec extension family (§ Sibling AV1 encode experience) is real, avoidable risk-stacking — not blocked by test-bootstrap availability (`rav1e` *could* supply real `INTER_FRAME` material, see § Scope decision reason 4), but by parser/engineering surface and risk-isolation discipline, the same reasoning HEVC's own decode scope-down already used. |
| Reuse `dpb.rs`'s `Dpb`/`DpbSlot` for AV1, just populating fields with placeholder/zero values | Rejected per this task's own explicit instruction and this ADR's own § Reference-model design: `frame_num`/`frame_num_wrap`/`pic_order_cnt`/sliding-window eviction have no AV1 meaning; force-fitting AV1 into that shape would either misuse fields or require immediately special-casing them, providing no real code-sharing benefit over a small, purpose-built `Av1RefSlots` type. |
| Extract a shared, real (non-test-only) OBU-framing helper into `mediaway-sw` now, since both `mediaway-encoder`'s AV1 encode side and this new decode side need `obu_header()`/`leb128()` scanning | Deferred, matching `adr/vulkan/0001`'s own identical decision for HEVC parser placement: no ADR yet decides to make this a shared-crate concern, and this ADR's own scope is decode-only. Revisit once a second real, non-test consumer exists and an ADR explicitly proposes the extraction — not decided here. |
| Support `filmGrainSupport = VK_TRUE` from the start, since AV1 film grain is common in real streaming content | Rejected this round: per § Scope decision reason 3, this forces `DISTINCT` DPB/output mode, structurally incompatible with this crate's whole `COINCIDE`-only architecture (`query_capabilities` rejects non-coincide today). Adding `DISTINCT`-mode support is real, separate architectural work this ADR does not attempt to smuggle in alongside AV1's first increment. |
| Wait for HEVC P/B-slice decode to be hardware-verified before starting AV1 at all | Considered, not adopted: AV1 decode's own risk profile (parser/reference-model complexity, driver-maturity uncertainty) is largely independent of HEVC's specific all-zero-output bug — there is no dependency between fixing HEVC's bug and starting AV1's own `KEY_FRAME`-only design/implementation. Sequencing purely for "one thing at a time" discipline, without a real technical dependency, was judged unnecessary caution given this crate's own precedent of H.264 and HEVC decode being developed and merged in the same broad effort. |

## Consequences

### Positive

- Stage 0's biggest risk (does any queue family on the reference hardware
  even advertise AV1 decode) is **already closed**, for free, by
  `adr/vulkan/0001`'s own earlier probe run — both reference GPUs report
  `av1_decode_queue_family = Some(...)`.
- A real, concrete, previously-unknown architectural fact was found this
  session (film grain forces `DISTINCT` mode, incompatible with this crate's
  `COINCIDE`-only design) — informs the scope decision with real evidence,
  not speculation.
- Most of the Vulkan-level plumbing this ADR needs
  (`session_command.rs`/`cpu_readback.rs`/`zero_copy.rs`) is **already
  codec-generic and reusable as-is**, confirmed by direct reading — AV1's own
  new-code surface is smaller than either H.264's or HEVC's was.
- A real, independent, driver-bug-free bitstream source
  (`mediaway-sw::av1::Av1Encoder`) exists for this ADR's own hardware test,
  already a workspace dependency — no new dependency, no reliance on the
  known-broken Vulkan AV1 encode path.
- `show_existing_frame`'s "no decode call needed" behavior is recorded now,
  before implementation, so a future general-GOP increment does not have to
  re-derive it from the spec cold.

### Negative / Trade-offs

- **Nothing in this ADR is hardware-verified** — same constraint every prior
  addendum in this crate's `adr/vulkan/0001` discloses at each stage; whether
  `record_and_submit_av1` actually produces correct pixels, or hits a
  sibling driver wall to the AV1 encode side's confirmed one, is genuinely
  unknown until implemented and run.
- `KEY_FRAME`-only scope means this crate cannot decode any real-world AV1
  stream end-to-end yet (virtually every real AV1 stream after the first
  frame is `INTER_FRAME`) — an honest, named ceiling, not silently presented
  as "AV1 decode" without qualification, per this workspace's caveats
  convention.
- Film grain is architecturally excluded, not just deferred — any AV1
  content relying on grain synthesis for correct appearance will look
  visibly different (flatter/cleaner) than a reference decoder's output,
  same honest gap `adr/vulkan/0001` already flagged for the encode-adjacent
  general AV1 film-grain question, now confirmed structurally load-bearing
  for this crate's specific architecture.
- `query_capabilities`/`query_video_format`'s existing 2-way boolean
  dispatch must become a real 3-way match — a small, real, easy-to-get-wrong
  mechanical change across `session.rs` and `decoder.rs::open`'s codec
  dispatch, flagged explicitly so it is not silently missed.
- The AV1 frame-header parser's own end-bit-position tracking (for
  `frameHeaderOffset`/tile-group boundary computation) is real, new
  complexity with no H.264/HEVC precedent in this crate to lean on (their
  own "boundary" is trivially "the whole NAL").
- `rav1e`'s exact real OBU-level output shape for a single-frame encode is
  unconfirmed this session — the hardware test's own bootstrap step may
  itself need debugging before AV1 decode's own bug-hunt can even begin,
  unlike HEVC's bootstrap (a known-working, already-hardware-verified
  encoder).

## Addendum (2026-08-19, confirmed via real vendored `vulkanalia-sys` 0.35.0 source)

Open question #1 is now closed for the exact pinned version this workspace uses. Read directly
from `vulkanalia-sys-0.35.0/src/video.rs` and `structs.rs` (not `docs.rs` — the real generated
source):

```rust
// video.rs — StdVideo* (bitstream-mirroring) structs
pub struct StdVideoDecodeAV1PictureInfo {
    pub flags: StdVideoDecodeAV1PictureInfoFlags,
    pub frame_type: StdVideoAV1FrameType,
    pub current_frame_id: u32,
    pub OrderHint: u8,               // NOTE: PascalCase, not order_hint
    pub primary_ref_frame: u8,
    pub refresh_frame_flags: u8,
    pub reserved1: u8,
    pub interpolation_filter: StdVideoAV1InterpolationFilter,
    pub TxMode: StdVideoAV1TxMode,   // NOTE: PascalCase
    pub delta_q_res: u8,
    pub delta_lf_res: u8,
    pub SkipModeFrame: [u8; 2],      // NOTE: PascalCase
    pub coded_denom: u8,
    pub reserved2: [u8; 3],
    pub OrderHints: [u8; 8],         // NOTE: PascalCase — one entry per ref-frame slot (0-7)
    pub expectedFrameId: [u32; 8],   // NOTE: camelCase
    pub pTileInfo: *const StdVideoAV1TileInfo,
    pub pQuantization: *const StdVideoAV1Quantization,
    pub pSegmentation: *const StdVideoAV1Segmentation,
    pub pLoopFilter: *const StdVideoAV1LoopFilter,
    pub pCDEF: *const StdVideoAV1CDEF,
    pub pLoopRestoration: *const StdVideoAV1LoopRestoration,
    pub pGlobalMotion: *const StdVideoAV1GlobalMotion,
    pub pFilmGrain: *const StdVideoAV1FilmGrain,
}

pub struct StdVideoDecodeAV1ReferenceInfo {
    pub flags: StdVideoDecodeAV1ReferenceInfoFlags,
    pub frame_type: u8,
    pub RefFrameSignBias: u8,        // NOTE: PascalCase
    pub OrderHint: u8,               // NOTE: PascalCase
    pub SavedOrderHints: [u8; 8],    // NOTE: PascalCase
}

// structs.rs — VkVideoDecodeAV1*KHR structs
pub struct VideoDecodeAV1PictureInfoKHR {
    pub s_type: StructureType, pub next: *const c_void,
    pub std_picture_info: *const video::StdVideoDecodeAV1PictureInfo,
    pub reference_name_slot_indices: [i32; 7],  // MAX_VIDEO_AV1_REFERENCES_PER_FRAME_KHR
    pub frame_header_offset: u32,
    pub tile_count: u32,
    pub tile_offsets: *const u32,
    pub tile_sizes: *const u32,
}
pub struct VideoDecodeAV1DpbSlotInfoKHR {
    pub s_type: StructureType, pub next: *const c_void,
    pub std_reference_info: *const video::StdVideoDecodeAV1ReferenceInfo,
}
pub struct VideoDecodeAV1ProfileInfoKHR {
    pub s_type: StructureType, pub next: *const c_void,
    pub std_profile: video::StdVideoAV1Profile,
    pub film_grain_support: Bool32,
}
pub struct VideoDecodeAV1SessionParametersCreateInfoKHR {
    pub s_type: StructureType, pub next: *const c_void,
    pub std_sequence_header: *const video::StdVideoAV1SequenceHeader,
}
pub struct VideoDecodeAV1CapabilitiesKHR {
    pub s_type: StructureType, pub next: *mut c_void,
    pub max_level: video::StdVideoAV1Level,
}
```

Every field this ADR's § Struct/extension survey table already cited
(`reference_name_slot_indices`, `frame_header_offset`, `tile_count`, `tile_offsets`,
`tile_sizes`, `std_reference_info`) is confirmed exactly as assumed. Two real corrections:

1. **`ref_frame_idx` and `ref_order_hint`, cited in § Struct/extension survey (line ~326) as
   `StdVideoDecodeAV1PictureInfo` fields, do not exist as fields of that struct in this pinned
   version.** Per-reference-slot order hints live in `StdVideoDecodeAV1PictureInfo::OrderHints:
   [u8; 8]` (indexed 0-7, matching `VkVideoDecodeAV1PictureInfoKHR::reference_name_slot_indices`'s
   own indexing) — there is no separate `ref_frame_idx` array in the picture-info struct at all;
   `reference_name_slot_indices` (on the **outer** `VkVideoDecodeAV1PictureInfoKHR`, not the Std
   struct) is the field that actually carries AV1's `ref_frame_idx[]` semantics. Implementers must
   not go looking for a `ref_frame_idx` field inside `StdVideoDecodeAV1PictureInfo` — it isn't
   there.
2. **Real, notable implementation gotcha not previously flagged**: both `StdVideoDecodeAV1*`
   structs mix `snake_case` and `PascalCase`/`camelCase` field names in the same struct
   (`frame_type` next to `OrderHint`, `refresh_frame_flags` next to `SkipModeFrame`/`TxMode`) —
   bindgen preserved the C header's own inconsistent naming verbatim. Every read/write site will
   need `#[allow(non_snake_case)]` (module- or item-scoped, not blanket-crate) wherever these
   exact field names are used directly — a real, unavoidable clippy friction point the H.264/HEVC
   `StdVideo*` structs this crate already uses do not have (their own fields are uniformly
   `snake_case`).

Open questions #2, #3, #4, #5, #6, #7 remain open — none are resolvable from struct definitions
alone; they need either a real driver, a real `rav1e` output sample, or actual bit-level design
work during implementation.

## Open questions / risks (explicit, for whoever picks this up)

1. **Exact `StdVideoDecodeAV1PictureInfo`/`StdVideoDecodeAV1ReferenceInfo`
   field names and bitfield-flag lists in this workspace's pinned
   `vulkanalia` 0.35.0** — not independently re-confirmed this session (relies
   on `adr/vulkan/0001`'s one-level-removed `docs.rs`-tier confirmation plus
   this ADR's own naming-convention inference from the **encode**-side
   `StdVideoEncodeAV1PictureInfo`, which is a *different*, not-guaranteed-identical
   struct — see § Struct/extension survey's own caveat). Re-verify via real
   `cargo doc`/vendored-source read before Stage 1 implementation relies on it,
   the same "docs.rs is not cargo check" discipline `adr/vulkan/0001` already
   established.
2. **`frameHeaderOffset`/tile-group byte-boundary computation** — this ADR
   names the requirement (§ Bitstream framing) but does not work out the
   exact bit-position bookkeeping `uncompressed_header()`'s own
   `byte_alignment()` needs; real, unsolved design work for whoever
   implements `av1_params.rs`.
3. **Whether `VK_KHR_video_decode_av1` is enumerated as a real device
   extension on this workspace's reference driver, beyond the queue-family
   codec-operation bit** — `probe.rs`'s existing probe only checks the
   queue-family bit (§ AV1 decode queue family section above), not whether
   the extension name itself is present in
   `vkEnumerateDeviceExtensionProperties` — the same gap `adr/vulkan/0001`
   already carried for H.264/HEVC (queue-family bit alone turned out
   sufficient there in practice), noted here for AV1 too, not a new gap this
   ADR introduces.
4. **Whether this driver generation's AV1 *decode* path shares AV1
   *encode*'s confirmed driver-maturity limitation** — explicitly,
   deliberately **unconfirmed either way** this session, per this task's own
   instruction not to assume either outcome. Real hardware verification is
   the only way to know.
5. **`rav1e`'s exact real single-frame OBU output shape** — not inspected
   this session (no shell/build tool); implementation must instrument and
   read `rav1e`'s real output bytes before assuming any specific
   `reduced_still_picture_header`/`OBU_FRAME`-vs-split shape.
6. **Exact `vulkanalia` extension-command trait exposing any AV1-decode-specific
   `vkCmd*`/`vkCreate*` call, if any exists beyond the already-confirmed
   shared `KhrVideoDecodeQueueExtensionDeviceCommands`/
   `KhrVideoQueueExtension*Commands` traits** (`adr/vulkan/0001`'s 2026-07-30
   addendum confirmed H.264/HEVC decode need no codec-specific command trait,
   only codec-specific *structs* chained via `push_next` — AV1 decode likely
   follows the identical pattern since the Vulkan extension design is
   struct-driven, not command-driven, per codec, but this was not
   independently re-confirmed for AV1 specifically this session).
7. **Whether a driver could report `DPB_AND_OUTPUT_DISTINCT`-only for an AV1
   profile even with `filmGrainSupport = VK_FALSE` requested** (unrelated to
   film grain, just a general per-profile capability variance) —
   `query_capabilities`'s existing `SeparateReferenceImagesRequired` rejection
   already handles this defensively for H.264/HEVC; AV1 inherits the same
   check once the 3-way match lands, not a new design gap, just noted for
   completeness.

## Implementation addendum (2026-08-19, real code, hardware-verified)

This ADR's design was implemented in full this same day, in
`src/vulkan/{av1_params.rs, av1_params/av1_frame_header.rs,
av1_params/av1_frame_header/av1_frame_std.rs, av1_refs.rs, decoder_av1.rs,
session_command_av1.rs}` plus the planned `session.rs`/`decoder.rs`/`mod.rs`
3-way-dispatch changes — matching this ADR's own file-layout plan almost
exactly (the frame-header parser and its `StdVideoAV1*` struct construction
were split into two files, not one, to stay under this workspace's
1000-line-per-source-file rule — `av1_params.rs` alone would have been
~1700 lines combined).

**Real compile-time corrections found, not guessed around** (per this task's
own instruction): `Av1RefSlots`/`Av1RefSlotsError` needed a real `#[from]`
variant on `VulkanDecodeError` (`Av1Dpb`) alongside `Av1Bitstream`, since it
is a distinct error type from `Av1ParamError`; `VkVideoDecodeAV1PictureInfoKHR`'s
`tile_offsets`/`tile_sizes` builder methods each independently set
`tile_count` as a side effect (no separate `.tile_count()` call needed);
`Av1FrameHeader`/`Av1PictureInfoOptionals` needed **item-level** visibility
kept at `pub(crate)` (not bare `pub`) even after review-driven module
restructuring — the workspace's `unreachable_pub` lint and
`clippy::redundant_pub_crate` lint disagree on private-module-nested items
exactly the way `session_command_hevc.rs`'s own existing comment already
documents; this ADR's own file-layout plan didn't anticipate the two-level
submodule nesting `av1_frame_header` → `av1_frame_std` needed, which changed
where that tension shows up but not its resolution (workspace policy wins,
per the established convention).

**Open question #2 (`frameHeaderOffset`/tile-boundary bit tracking) — resolved**
for this crate's single-tile scope: `src_buffer` holds only the `OBU_FRAME`'s
payload bytes (post `obu_header()`/`leb128` size field), `frameHeaderOffset`
is always `0`, and the single tile's offset/size are computed directly from
[`BitReader::bits_read`]'s position at the end of `uncompressed_header()`,
rounded up to a byte boundary — see `av1_frame_header.rs`'s own module doc
for the full reasoning and the explicit caveat that this design decision has
**no cross-checked reference implementation** behind it (unlike H.264/HEVC's
FFmpeg-confirmed offset conventions).

**Open question #5 (`rav1e`'s real OBU output shape) — resolved** by real
byte inspection this pass (a temporary instrumented test, reverted before
finishing): `rav1e` emits `OBU_TEMPORAL_DELIMITER` + `OBU_SEQUENCE_HEADER` +
one combined `OBU_FRAME` (frame header and tile group together) for a
single-frame, single-tile encode — never split `OBU_FRAME_HEADER` +
`OBU_TILE_GROUP`. The real sequence header this pass decoded byte-for-byte
by hand (and cross-checked against the implementation's own parser) has
`enable_cdef = 1` and the frame header has `segmentation_enabled = 1` —
**not** the all-disabled shape this crate's own AV1 **encoder**
(`mediaway-encoder::vulkan::av1_params`) produces — so `av1_frame_header.rs`'s
`parse_segmentation`/`parse_cdef`/`parse_loop_filter`/`parse_lr` are real,
spec-faithful parsers, not stubs; they had to be, to decode this pass's own
real test bitstream at all.

**Open question #4 (whether AV1 decode shares AV1 encode's confirmed
driver-maturity wall) — resolved, real result**: **no.**
`tests/vulkan/hardware_av1_decode.rs` pushes a real `mediaway_sw::av1::Av1Encoder`
`KEY_FRAME` (flat mid-gray 256×192 I420) through `VulkanVideoDecoder` on the
RTX 4090 reference machine and passes **hard** content assertions (every
decoded luma byte nonzero, center sample exactly `128`) — on the **first**
attempt, no bug-fix round needed, unlike every other real-hardware-verified
codec/direction combination in this crate's history (H.264 decode needed
three real protocol bugs fixed; HEVC decode needed a slice-header parsing
bug fixed; this workspace's own AV1 Vulkan *encode* is still confirmed
producing invalid output). This is a genuinely surprising, real, honestly
reported result — not assumed, not hoped for.

Open questions #1, #3, #6, #7 remain open exactly as stated above — none were
resolved or newly falsified by this implementation pass (question #1's
struct-field claims were already independently re-confirmed by the earlier
2026-08-19 addendum above, which this implementation pass re-confirmed a
third time by reading the same vendored source directly before writing any
code).

## References

- `adr/vulkan/0001-vulkan-video-decode.md` (this same `adr/vulkan/` folder) —
  direct structural precedent for H.264 general-GOP (hardware-verified) and
  HEVC IDR-only (hardware-verified, honest scope-down) decode; every module
  this ADR reuses/mirrors is defined there first.
- `crates/mediaway-decoder/src/vulkan/mod.rs`, `decoder.rs`, `decoder_hevc.rs`,
  `session.rs`, `session_command.rs`, `session_command_h264.rs`, `dpb.rs`,
  `probe.rs`, `zero_copy.rs`, `h264_params.rs` — all read directly this
  session, cited by line number throughout.
- `crates/mediaway-encoder/src/vulkan/av1_params.rs`, `av1_gop.rs` — read in
  full this session; source of every encode-side AV1 struct-construction
  finding this ADR reuses/adapts as a naming-convention proxy.
- `crates/mediaway-encoder/adr/vulkan/0002-vulkan-gop-rate-control.md` — real,
  hardware-run AV1 encode driver-maturity finding (2026-08-05 run cited
  directly), `crates/mediaway-encoder/adr/vulkan/0001-vulkan-video-encode-ash-probe.md`
  for the original AV1 addendum this session did not re-read in full (already
  summarized accurately by ADR-0002's own recap, cross-checked against this
  session's persistent memory of the same finding).
- `crates/mediaway-sw/src/av1.rs` — real, working `rav1e`-backed software AV1
  encoder this ADR's own test plan uses as its hardware-test bitstream
  source.
- `crates/mediaway-common/src/lib.rs` (`CodecKind::Av1`, confirmed present),
  `crates/mediaway-common/src/formats.rs` (`PixelFormat` — confirmed no
  10-bit variant exists, same ceiling `adr/vulkan/0001` already named,
  unaffected by this ADR's own 8-bit-only AV1 Main-profile scope).
- Khronos `VK_KHR_video_decode_av1` proposal document:
  <https://github.com/KhronosGroup/Vulkan-Docs/blob/main/proposals/VK_KHR_video_decode_av1.adoc>
  (fetched this session; source of the `referenceNameSlotIndices`/
  `refresh_frame_flags`/`show_existing_frame`/film-grain-`DISTINCT`-mode
  findings above).
- `VkVideoDecodeAV1PictureInfoKHR` reference:
  <https://docs.vulkan.org/refpages/latest/refpages/source/VkVideoDecodeAV1PictureInfoKHR.html>
- `VkVideoDecodeAV1DpbSlotInfoKHR` reference:
  <https://docs.vulkan.org/refpages/latest/refpages/source/VkVideoDecodeAV1DpbSlotInfoKHR.html>
- AV1 spec (aomediacodec.github.io/av1-spec), § 5.9.2 (`byte_alignment()`),
  § 5.10 (`frame_obu`), § 7.20 (reference frame update process) — referenced
  by section number per this workspace's external-standards convention
  ([`docs/conventions/external-standards.md`](../../../../docs/conventions/external-standards.md));
  not locally cached/pinned this session (design-only pass, no implementation
  yet to require a pinned local copy).
- [`docs/spec/sans-io.md`](../../../../docs/spec/sans-io.md) ·
  [`docs/spec/api-layers.md`](../../../../docs/spec/api-layers.md) ·
  [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md)
- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
  — no new dependency this ADR (`mediaway-sw` already a direct dependency,
  `vulkanalia` already workspace-pinned).

ADRs are **English**. Numbering is local to this `adr/` folder.
