# ADR-0002: VA-API H.264 single-forward-reference P-frame GOP (port from `vulkan/h264_gop.rs`)

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`src/linux/vaapi/`)

## Context

[ADR-0001](0001-vaapi-cros-libva-h264-cpu-upload.md) scoped `mediaway-encoder::linux::vaapi` to
**every pushed frame is an independent IDR** — no `frame_num` sequencing beyond `0`, no
reference-picture-list construction, no GOP structure — and named the obvious next step in its
own Scope table:

> **Out (deferred, tracked in `docs/roadmap.md`)**: ... P/B-frame GOP structure, VBR/CBR rate
> control, VUI/cropping SPS fields.

Re-read against the current file: that quote is still accurate word-for-word. Confirmed directly
in `crates/mediaway-encoder/src/linux/vaapi/video.rs` (current state, post the earlier
`manual_is_multiple_of` clippy fix — that fix only touched `validate`'s alignment check, unrelated
to this ADR):

- `next_surface: usize` round-robin cursor (`video.rs:52`, `(next_surface + 1) %
  surfaces.len()` at `video.rs:248`) is the *only* slot-selection strategy — every frame writes
  into the next surface in rotation with zero notion of "this surface still holds content a
  later frame needs to read as a reference."
- `build_pic_params` (`video.rs:387-424`) hardcodes `frame_num: 0` (comment: "always 0 (every
  frame independent IDR)"), `idr_pic_flag: 1`, `reference_pic_flag: 0`, and fills
  `reference_frames: [PictureH264; 16]` entirely with `invalid_picture_h264()`
  (`VA_PICTURE_H264_INVALID`).
- `build_slice_params` (`video.rs:427-467`) hardcodes `slice_type: 2` (I slice, VA-API/H.264's
  own numeric convention: P=0, B=1, I=2), `idr_pic_id: 0`, and fills both `ref_pic_list_0`/
  `ref_pic_list_1: [PictureH264; 32]` entirely with `invalid_picture_h264()`.
- `build_seq_params` (`video.rs:342-384`) hardcodes `intra_period: 1, intra_idr_period: 1,
  ip_period: 0` (comments: "every frame is an I frame" / "every frame is an IDR" / "no P frames
  in this stage") and `log2_max_frame_num_minus4: 4` (comment: "frame_num always 0 in this
  stage; ample range" — an arbitrary value, since `frame_num` never leaves `0`).
- `mediaway_encoder::VideoEncoderConfig` (`crates/mediaway-encoder/src/video.rs:59,65`) already
  carries cross-backend `gop_size: u32` / `rate_control: Option<RateControlConfig>` fields (added
  by [`adr/vulkan/0002`](../vulkan/0002-vulkan-gop-rate-control.md)'s Vulkan H.264/HEVC work,
  2026-08-05) — but `crates/mediaway-encoder/src/linux/vaapi/video.rs`'s `open_cpu`/`validate`
  never read `config.gop_size` at all today; every VA-API session silently behaves as if
  `gop_size == 1` regardless of what a caller requests. `config.gop_size`'s own rustdoc already
  states the contract this ADR must honor: `0` is rejected outright, and "a backend that cannot
  honor a value `> 1` ... falls back to IDR-only and must document that fallback on its own
  encoder type's rustdoc."

This ADR designs (and this session actions) the encode-side counterpart to
[`mediaway-decoder`'s ADR-0002](../../../mediaway-decoder/adr/linux/0002-vaapi-h264-p-slice-dpb.md)
(same session, same crate family, landed immediately before this ADR): single-forward-reference
P-frame GOP structure — real `frame_num` sequencing, IDR-vs-P decision per a configurable GOP
size, and real (not all-invalid) reference-picture-list fields — explicitly **not** B-frames,
multi-reference, or VBR/CBR rate control (rate control stays a separate, already-deferred axis;
see § Scope).

### Why this ADR ports `mediaway-encoder::vulkan::h264_gop`'s `GopState` instead of re-deriving it

`crates/mediaway-encoder/src/vulkan/h264_gop.rs` is a **real, hardware-verified** (RTX 4090,
[`adr/vulkan/0002`](../vulkan/0002-vulkan-gop-rate-control.md)'s 2026-08-05 implementation update:
"Real DPB slot cycling, real P-frame `RefPicList0` prediction... all worked on the first hardware
attempt") implementation of exactly the single-forward-reference P-frame GOP decision state
machine (IDR-vs-P cadence, `frame_num` sequencing, one-slot-lookback DPB ring) this task needs.
Unlike `mediaway-decoder::vulkan::dpb`'s porting precedent (general multi-reference sliding-window
DPB, ITU-T H.264 § 8.2.4/8.2.5.3 arithmetic), `h264_gop.rs` is **already GPU-API-agnostic** — its
own module doc states "No Vulkan FFI, no `unsafe`" and its `DpbSlot`/`Dpb`/`GopState`/
`FrameDecision` types hold zero `vk::*`/GPU-handle fields (confirmed by reading the file in full,
see § Precise porting plan). This is a stronger porting case than the decoder ADR's: the source
file requires no "drop the Zero-Copy handle bookkeeping" surgery at all — it is pure H.264
bitstream-spec decision logic today, incidentally living in the `vulkan/` module for historical
reasons (it was written for the Vulkan backend first), not because it depends on Vulkan.
Re-deriving this state machine from the ITU-T H.264 spec text independently for VA-API would
re-risk the same bug classes ADR-0001's own hardware-verification caveat already warns about, for
a state machine this workspace has already gotten right once.

### Zero real-hardware verification remains the honest baseline

Re-confirmed: ADR-0001 makes **no** hardware-verification claim for `mediaway-encoder::linux::vaapi`
— every VA-API call path was "written and compile-verified on Linux (WSL2 Ubuntu 24.04 via
`cargo check`/`cargo test`/`cargo clippy`, real `libva-dev` 1.20.0 headers/bindgen output)" but
`Display::open()`/`vaInitialize` has never succeeded against real hardware in this environment
(Windows dev box; the available WSL2 instance has broken VA-API, `vainfo` segfaults, no real GPU
exposed). This ADR's new work (this session) ships the same way: **design only, no `.rs` files
written this pass.** The implementation pass that follows this ADR must run `cargo check -p
mediaway-encoder --target x86_64-unknown-linux-gnu` / `cargo clippy --all-targets -- -D warnings`
(WSL2 Ubuntu, real `libva-dev`) before claiming even compile correctness for the new
`VAConfigAttribEncMaxRefFrames` constant name and `PictureH264::frame_idx` semantics this ADR
assumes (see § Open questions) — flagged explicitly as unconfirmed, not silently assumed, mirroring
the decode sibling's own disposition for its `VA_PICTURE_H264_SHORT_TERM_REFERENCE` open item.

## Decision

> Extend `mediaway-encoder::linux::vaapi` to encode **single-forward-reference P-frame GOP
> structures** (`RefPicList0[0]` only, `num_ref_idx_l0_active` fixed at exactly `1`) by porting
> `mediaway-encoder::vulkan::h264_gop::GopState` (+ `Dpb`/`DpbSlot`/`FrameDecision`/`FrameRequest`/
> `LOG2_MAX_FRAME_NUM_MINUS4`) verbatim into a new, crate-local, sans-io
> `linux/vaapi/gop.rs`, then wiring its `FrameDecision` output into real (not all-invalid)
> `EncPictureParameterBufferH264`/`EncSliceParameterBufferH264` fields. **No B-frames, no
> multi-reference, no reference-list reordering, no long-term references** — this mirrors
> `h264_gop.rs`'s own permanent scope cut and `mediaway-decoder`'s sibling ADR-0002's identical
> narrowing, not a fresh scope decision. **CQP-only rate control stays unchanged this
> increment** — `VideoEncoderConfig::rate_control` continues to be read by nothing in this
> backend (same disposition Vulkan's HEVC/AV1 give it); the one *coupled* change this GOP work
> does force (see § VA-API-specific plumbing) is that `EncSequenceParameter` is now sent **only
> on IDR frames**, not every frame, since sending a fresh SPS/PPS ahead of every single P-frame
> would defeat this ADR's own bandwidth-efficiency motivation.

### Precise porting plan: which `h264_gop.rs` items map to which new VA-API-side items

New file `crates/mediaway-encoder/src/linux/vaapi/gop.rs`, sans-io (no `cros_libva` types),
unit-testable without any VA-API device:

| New (`linux/vaapi/gop.rs`) | Ported from (cited source) | Change from source |
|---|---|---|
| `WORKSPACE_DPB_CAP: usize = 4` | `vulkan/h264_gop.rs:27` | Verbatim — already GPU-API-agnostic; happens to equal this crate's pre-existing `SURFACE_POOL_SIZE` (`video.rs:28`), so the physical surface pool needs **no size change**, only a new selection strategy (see § VA-API-specific plumbing) |
| `LOG2_MAX_FRAME_NUM_MINUS4: u8 = 12` | `vulkan/h264_gop.rs:40` | Verbatim (H.264's spec-legal maximum, sidesteps `FrameNumWrap` arithmetic entirely for any `gop_size` up to 65536 — irrelevant to a single-forward-reference design that never looks back more than one frame) — only applied when GOP mode is actually active; `gop_size <= 1` keeps this crate's existing SPS value (`4`) unchanged, matching the porting source's own "only applied when GOP mode is active" carve-out for its default path |
| `DpbSlot { frame_num: u32, poc: i32, is_idr: bool }` | `vulkan/h264_gop.rs:47-51` | Verbatim — zero GPU-handle fields in the source already |
| `Dpb { slots: [Option<DpbSlot>; WORKSPACE_DPB_CAP], next_slot: usize }` | `vulkan/h264_gop.rs:55-58` (+ `Default` impl, `60-67`) | Verbatim |
| `FrameRequest { Auto, ForceIdr }` | `vulkan/h264_gop.rs:78-81` | Verbatim — `ForceIdr` stays an unwired hook here too (no caller in this crate builds one this pass, same as the Vulkan source's own disposition) |
| `FrameDecision { is_idr, frame_num, poc, idr_pic_id, setup_slot, reference: Option<(usize, DpbSlot)> }` | `vulkan/h264_gop.rs:87-97` | Verbatim |
| `GopState { gop_size, frames_since_idr, frame_num, idr_counter, dpb, last_written }` | `vulkan/h264_gop.rs:103-110` | Verbatim |
| `GopState::new(gop_size)` | `vulkan/h264_gop.rs:119-128` | Verbatim |
| `GopState::decide(request) -> FrameDecision` | `vulkan/h264_gop.rs:130-180` | Verbatim — this function contains zero Vulkan-specific logic already; `poc = 2 * frame_num` (POC type 2) matches this crate's own already-shipped SPS choice (`pic_order_cnt_type = 2`, unchanged by this ADR — see § Cross-check below) |

No new fields or methods are added to the ported types — this crate's single-forward-reference
design only ever needs `decision.reference`'s **one** `(usize, DpbSlot)` pair (unlike
`mediaway-decoder`'s sibling ADR, which needed a full occupied-slot enumeration for its
"list every occupied DPB slot" `ReferenceFrames` convention — see § Cross-check for why VA-API
*encode* does not need that here).

### VA-API-specific plumbing (distinct from the ported `GopState` above)

**Confirmed by reading `cros-libva` 0.0.13's real vendored source directly**
(`C:\Users\User\.cargo\registry\src\...\cros-libva-0.0.13\src\buffer\h264.rs`, not paraphrased —
same file this crate's `video.rs` already calls into):

- `EncPictureParameterBufferH264::new(curr_pic, reference_frames: [PictureH264; 16], coded_buf,
  pic_parameter_set_id, seq_parameter_set_id, last_picture, frame_num: u16, pic_init_qp,
  num_ref_idx_l0_active_minus1, num_ref_idx_l1_active_minus1, chroma_qp_index_offset,
  second_chroma_qp_index_offset, pic_fields: &H264EncPicFields)` (`h264.rs:615-663`) — signature
  matches `video.rs::build_pic_params`'s current call site exactly, field for field. The only
  **types** this ADR needs to reconcile: `frame_num` is `u16` here, but `GopState::frame_num` is
  `u32`. Safe by construction: `LOG2_MAX_FRAME_NUM_MINUS4 = 12` gives `MaxFrameNum = 65536`, and
  `GopState::decide` already reduces `frame_num` modulo that range (`vulkan/h264_gop.rs:176`) —
  every value `decision.frame_num` can take is `< 65536`, i.e. always representable in `u16`. A
  `#[allow(clippy::cast_possible_truncation, reason = "...")]`-documented cast (mirroring the
  decode sibling's own `cast_possible_wrap` allows for its bounded-by-spec-field-width casts) is
  the right shape here, not a fallible `try_from` needing an unreachable error arm.
- `EncSliceParameterBufferH264::new(...)` (`h264.rs:665-755`) — signature matches
  `video.rs::build_slice_params`'s current call site exactly, field for field, including
  `ref_pic_list_0: [PictureH264; 32]` / `ref_pic_list_1: [PictureH264; 32]`. No signature change
  needed — only the **values** passed for `slice_type`, `idr_pic_id`, and `ref_pic_list_0[0]`
  change.
- `EncSequenceParameterBufferH264::new(...)` (`h264.rs:493-569`) — signature unchanged; only the
  `intra_period`/`intra_idr_period`/`ip_period`/`seq_fields` (`log2_max_frame_num_minus4`)
  **values** change per GOP mode (see below), and the **call site** now only invokes this
  constructor + `context.create_buffer(...)` + `picture.add_buffer(seq_buf)` when
  `decision.is_idr` — not unconditionally, once GOP mode is active.
- `PictureH264::new(picture_id, frame_idx: u32, flags: u32, top_field_order_cnt: i32,
  bottom_field_order_cnt: i32)` (`h264.rs:12-30`) — same constructor `video.rs`'s existing
  `invalid_picture_h264()` helper already calls. For a real reference entry: `picture_id` = the
  referenced surface's `VASurfaceID` (`Surface::id()`, already used elsewhere in `video.rs`),
  `frame_idx` = the referenced `DpbSlot::frame_num` (inferred from libva/FFmpeg's
  `vaapi_encode_h264.c` convention — **not independently confirmed against a real driver this
  session**, flagged in § Open questions), `flags` =
  `VA_PICTURE_H264_SHORT_TERM_REFERENCE` (already confirmed `= 8u32` this session by
  `mediaway-decoder`'s sibling ADR-0002, reading the real WSL2-generated `cros-libva` bindgen
  output directly — this constant is shared bindgen output across the whole `cros-libva` crate,
  not decode-specific, so that confirmation applies here unchanged, no re-verification needed),
  `top_field_order_cnt`/`bottom_field_order_cnt` = the referenced `DpbSlot::poc` (both fields
  equal — progressive only, no field coding, matching every other `PictureH264` this crate
  builds).
- `Display::get_config_attributes(profile, entrypoint, attributes: &mut [VAConfigAttrib]) ->
  Result<(), VaError>` (`cros-libva` `display.rs:225-242`, wraps `vaGetConfigAttributes`) — a
  real, already-existing safe wrapper this crate has never called (`video.rs` only ever
  *requests* `VAConfigAttribRateControl` via `create_config`'s attrs list; it never *queries* a
  driver capability first). This ADR uses it once, at `open_cpu` time, to probe
  `VAConfigAttribEncMaxRefFrames` before trusting `config.gop_size > 1` — the first "probe first,
  never assume" capability gate this VA-API encoder backend has needed, mirroring
  `mediaway-encoder::vulkan`'s `Capabilities::supports_p_frames` precedent
  ([`adr/vulkan/0002`](../vulkan/0002-vulkan-gop-rate-control.md) § Capability gating) and
  honoring `VideoEncoderConfig::gop_size`'s own rustdoc contract ("a backend that cannot honor a
  value `> 1` ... falls back to IDR-only ... and must document that fallback").

  **Confirmed same day (closing this ADR's own flagged open item) by reading the real WSL2
  `cargo check -p mediaway-encoder --target x86_64-unknown-linux-gnu`-generated
  `cros-libva`/`out/bindings.rs` directly**: `VAConfigAttribEncMaxRefFrames: Type = 13` is a real
  bindgen-generated `VAConfigAttribType` variant (name and value both confirmed, not inferred),
  and `VA_ATTRIB_NOT_SUPPORTED: u32 = 2147483648` (`0x8000_0000`, the sign/high bit) is the real
  sentinel `VAConfigAttrib::value` carries when a driver doesn't support the queried attribute at
  all — exactly the check this ADR's capability gate needs
  (`attrib.value != VA_ATTRIB_NOT_SUPPORTED`). The attribute's own internal packed-value bit
  layout (low bits = max P/forward references, high bits = max B references, per general
  `va_enc_h264.h` convention) was **not** independently confirmed against a doc comment this
  session — bindgen does not retain the header's prose here — but this ADR's gate only needs the
  not-supported sentinel check and a non-zero low-order reference count, not the full bitfield
  breakdown, so this residual uncertainty does not block the implementation pass.

### `VaapiVideoEncoder` struct shape (ZCA sketch — ownership, no `Box`/`dyn`)

```rust
// linux/vaapi/gop.rs — new file, sans-io, no cros_libva types. Verbatim port of
// crate::vulkan::h264_gop's types (see porting table above).
pub(super) const WORKSPACE_DPB_CAP: usize = 4;
pub(super) const LOG2_MAX_FRAME_NUM_MINUS4: u8 = 12;

#[derive(Debug, Clone, Copy)]
pub(super) struct DpbSlot { pub(super) frame_num: u32, pub(super) poc: i32, pub(super) is_idr: bool }

struct Dpb { slots: [Option<DpbSlot>; WORKSPACE_DPB_CAP], next_slot: usize }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameRequest { Auto, ForceIdr }

#[derive(Debug, Clone, Copy)]
pub(super) struct FrameDecision {
    pub(super) is_idr: bool,
    pub(super) frame_num: u32,
    pub(super) poc: i32,
    pub(super) idr_pic_id: u16,
    pub(super) setup_slot: usize,
    pub(super) reference: Option<(usize, DpbSlot)>,
}

#[derive(Debug)]
pub(super) struct GopState { /* verbatim fields, see porting table */ }
impl GopState {
    pub(super) fn new(gop_size: u32) -> Self { .. }
    pub(super) fn decide(&mut self, request: FrameRequest) -> FrameDecision { .. }
}

// linux/vaapi/video.rs — changed struct shape.
pub(crate) struct VaapiVideoEncoder {
    context: Rc<Context>,
    _config: Config,
    info: StreamInfo,
    width: u32,
    height: u32,
    mb_width: u16,
    mb_height: u16,
    nv12_bytes: usize,
    bits_per_second: u32,
    surfaces: Vec<Option<Surface<()>>>, // unchanged storage/size; now GOP-ring-indexed
    gop: GopState,                      // NEW — replaces `next_surface: usize`
    supports_p_frames: bool,            // NEW — from VAConfigAttribEncMaxRefFrames query
    pending: VecDeque<Packet>,
    flushed: bool,
}
```

- `next_surface: usize` (round-robin ring index) is **removed** — `GopState::decide`'s
  `setup_slot` entirely replaces it. Because `WORKSPACE_DPB_CAP` (`4`) already equals today's
  `SURFACE_POOL_SIZE`, `surfaces` keeps its existing length; only the index that
  `push_frame` writes into changes from "always `+1` mod len" to "whatever `decide()` says."
- `gop: GopState::new(effective_gop_size)` is constructed once in `open_cpu`, where
  `effective_gop_size = if config.gop_size > 1 && supports_p_frames { config.gop_size } else {
  1 }` — `gop_size <= 1` or an unsupporting driver both degrade to `GopState::new(1)`, which
  (per the porting source's own doc) reproduces today's exact all-IDR sequence byte-for-byte, so
  every existing caller/test stays unaffected by default.
- No `Box<dyn _>`/`dyn Trait` anywhere in this design — `GopState`/`Dpb`/`DpbSlot` are closed,
  concrete structs, matching every other encode/decode backend in this workspace.
- `[Option<DpbSlot>; WORKSPACE_DPB_CAP]` (not `Vec`, not `SmallVec`): ported verbatim from a
  source that already reasoned about this choice ("a bare array beats `SmallVec` here since
  there is no heap-spill case to avoid" — `vulkan/h264_gop.rs`'s own module doc) — this crate's
  version has the identical shape, so the identical reasoning applies unchanged.

### Reference-list construction — the correctness-critical sequencing

1. `push_frame` validates the incoming `VideoFrame` exactly as today (dimensions, storage kind,
   `nv12_bytes`), unchanged.
2. `let decision = self.gop.decide(FrameRequest::Auto);` — this call's own side effects (writing
   `decision.setup_slot`'s `DpbSlot` into the ring, advancing every counter) happen unconditionally,
   exactly mirroring `h264_gop.rs`'s existing Vulkan call site (`VulkanVideoEncoder::push_frame`)
   — the decision is never spuriously discarded or retried, since `GopState::decide` is not
   idempotent (see the next point for why this matters).
3. **New guard, with no Vulkan-side precedent** (see § Real gap found below): if
   `decision.reference` is `Some((ref_slot, _))`, check `self.surfaces[ref_slot].is_some()`
   *before* touching `decision.setup_slot`'s surface. If the referenced slot's physical surface
   is missing, return `Err(EncodeError::Backend)` immediately — the same disposition
   `push_frame`'s existing `self.surfaces[slot].take().ok_or(EncodeError::Backend)?` already gives
   a missing **setup**-slot surface, applied symmetrically to a missing **reference**-slot
   surface. No silent downgrade-to-IDR is attempted (see rationale below).
4. `let surface = self.surfaces[decision.setup_slot].take().ok_or(EncodeError::Backend)?;` —
   unchanged shape, new index source.
5. Upload NV12 into `surface` exactly as today (`upload_cpu_nv12`, unchanged) — this always writes
   **fresh, this-frame's-own** pixel content into the setup slot's surface, whether the frame is
   an IDR or a P frame; the *reference* slot's surface is a **different** index and is never
   uploaded into this call.
6. `encode_one(surface, frame, &decision)` builds `EncPictureParameterBufferH264`/
   `EncSliceParameterBufferH264` from `decision` (real `frame_num`/`idr_pic_flag`/
   `reference_pic_flag`/`slice_type`/`idr_pic_id`, and — when `decision.reference.is_some()` — one
   real `PictureH264` entry at `ReferenceFrames[0]`/`RefPicList0[0]`, built from
   `self.surfaces[ref_slot].as_ref()`'s `VASurfaceID` + the referenced `DpbSlot`'s `frame_num`/
   `poc`, per § VA-API-specific plumbing). `EncSequenceParameterBufferH264` is only built and
   attached when `decision.is_idr`.
7. On success: `self.surfaces[decision.setup_slot] = returned_surface;` — unchanged shape, new
   index source. The surface at `ref_slot` is **never taken or mutated** by this call — VA-API's
   own encode convention is that the same surface used as `CurrPic` during one frame's encode
   implicitly holds the *reconstructed* picture afterward, usable as a later frame's reference
   with no separate DPB image (unlike Vulkan Video's explicit DPB image array) — see § Cross-check
   for the full reasoning.

### Real gap found while designing this: a lost-reference-surface landmine with no Vulkan-side precedent

`encode_one`'s own existing doc comment already documents that a failed `Picture::begin`/
`render`/`end` step **unrecoverably loses the surface** (`video.rs:129-134`) — under all-IDR
encode this is harmless (every frame is independent; a lost pool slot just means fewer surfaces
in rotation, self-healing via the existing `take().ok_or(EncodeError::Backend)` guard). **GOP mode
introduces a real cross-frame dependency for the first time**: if a P-frame's encode fails after
its surface was already handed to `Picture`, that same slot may later be selected by `GopState` as
some future frame's `reference` (the ring's own bookkeeping has no way to know a physical surface
was lost — `GopState` tracks only its own logical `DpbSlot` records, never touches `Surface`s at
all). Left unguarded, a later `push_frame` would build a `ReferenceFrames`/`RefPicList0` entry
pointing at a `VASurfaceID` whose slot is actually `None` in `self.surfaces` — either a panic (if
coded carelessly) or, worse, a `VASurfaceID` value quietly stale/reused for something else by the
driver by the time it is dereferenced. The guard in § step 3 above closes this by treating a
missing reference surface as a hard `EncodeError::Backend` for that `push_frame` call, matching
this crate's existing "don't fabricate, don't panic, report honestly" philosophy rather than
attempting an in-flight GopState/physical-surface resynchronization this crate cannot cheaply
prove correct. A caller that needs resilience across such a failure should treat any
`push_frame` `Err(Backend)` as fatal to the session and reopen, exactly as `encode_one`'s
existing doc comment already implies for the setup-slot case.

### Cross-check against `mediaway-decoder::linux::vaapi`'s sibling ADR-0002 (same session)

Both sides port from an already-hardware-verified Vulkan sibling and land on structurally similar
per-slot bookkeeping (`frame_num`/POC/is-a-reference triple, ring or vec of slots), but they are
**not identical**, and two real disagreements are worth recording rather than assuming away:

1. **`FrameNumWrap` arithmetic**: the decoder implements the *general* ITU-T H.264 § 8.2.4.1
   `FrameNumWrap` derivation (needed because it must decode arbitrary incoming streams that might
   genuinely wrap `frame_num`). This encoder **sidesteps that arithmetic entirely** by choosing
   `LOG2_MAX_FRAME_NUM_MINUS4 = 12` (`MaxFrameNum = 65536`) so no realistic session ever wraps —
   this is not a disagreement in *outcome* (a compliant decoder reading this encoder's output
   would simply never observe a wrap event, so its general-case logic degrades harmlessly to the
   identity case), only in *implementation completeness*. No action needed; both are internally
   consistent given each side's own scope.
2. **`pic_order_cnt_type` mismatch — a real, pre-existing cross-crate interop gap.** This
   encoder's `build_seq_params` has always set `pic_order_cnt_type = 2` (POC implicitly derived
   as `2 * frame_num`, zero explicit bitstream signaling — ADR-0001's original design, unchanged
   by this ADR). `mediaway-decoder::linux::vaapi::sps::Sps::parse` **hard-rejects any
   `pic_order_cnt_type != 0`** (`sps.rs:69`, doc: "always `0` (this parser rejects `1`/`2`)") —
   its own sibling ADR-0002 explicitly designed only for `pic_order_cnt_type == 0`'s *general,
   explicit* POC derivation, since it must decode arbitrary real-world streams that predominantly
   use that type. **Consequence**: this workspace's own `mediaway-decoder::linux::vaapi` cannot
   decode this workspace's own `mediaway-encoder::linux::vaapi`'s output — not a spec violation on
   either side (`pic_order_cnt_type == 2` is fully legal H.264 for a P-frame-only, no-B-frame
   stream, arguably the *more* fitting choice for this exact use case), purely a scope-narrowing
   choice each crate made independently before this session cross-checked them. This ADR does
   **not** fix this (fixing would mean either this encoder adopting explicit POC signaling it does
   not otherwise need, or the decoder widening its own accepted `pic_order_cnt_type` set — both
   independent, cross-crate decisions outside "encoder-side P-frame GOP support"). Flagged
   prominently in § Open questions as a follow-up either side could pick up. This gap does **not**
   affect this ADR's own test plan, since system `ffmpeg`/`ffprobe` (the workspace's standing test
   oracle, [ADR-0002](../../../../docs/adr/0002-system-oracle.md)) decodes every `pic_order_cnt_type`
   correctly and remains a valid oracle for this encoder's output regardless.
3. **No full-DPB `ReferenceFrames` enumeration needed here, unlike the decoder.** The decoder's
   sibling ADR builds `PictureParameterBufferH264::ReferenceFrames` from *every* occupied DPB slot
   (mirroring FFmpeg's `fill_vaapi_ReferenceFrames`), because a general decoder must faithfully
   expose the stream's own declared `max_num_ref_frames` DPB state to the driver regardless of how
   many entries a given picture's slice actually reads. This encoder's single-forward-reference
   design never has more than **one** logical reference at any time (`GopState`'s own `last_written`
   field, not a general occupancy set) — so `ReferenceFrames`/`RefPicList0` here only ever need
   **one** real entry (index `0`) plus 15/31 `invalid_picture_h264()` fill entries, matching
   `h264_params.rs`'s Vulkan analogue (`build_single_reference_list`) exactly. This is a genuine,
   deliberate structural difference driven by each side's own scope, not an oversight on either
   ADR's part.
4. **The surface pool doubles as the DPB — VA-API encode has no separate DPB image, unlike
   Vulkan Video.** Vulkan Video's encoder allocates an explicit multi-layer DPB image distinct
   from its input-upload path (`adr/vulkan/0002`'s "DPB image layout transition" finding).
   VA-API's `VAEntrypointEncSlice` convention (inferred from FFmpeg's `vaapi_encode.c`, not
   independently driver-confirmed this session) has no such separate buffer: the same
   surface pool this crate already uses for CPU-NV12-upload input also implicitly holds each
   frame's *reconstructed* picture after `vaEndPicture`, usable as a later frame's reference with
   zero additional allocation. This is why `WORKSPACE_DPB_CAP` mapping directly onto the existing
   `SURFACE_POOL_SIZE`-sized `surfaces` array (no separate DPB array) is correct here, whereas the
   Vulkan port needed a dedicated DPB image array.

## Scope

**In (this ADR):**

- `linux/vaapi/gop.rs`: verbatim port of `vulkan::h264_gop::GopState` (+ `Dpb`/`DpbSlot`/
  `FrameDecision`/`FrameRequest`/`LOG2_MAX_FRAME_NUM_MINUS4`/`WORKSPACE_DPB_CAP`).
- `VideoEncoderConfig::gop_size` finally read by this backend: `1` (default) reproduces today's
  all-IDR behavior byte-for-byte; `> 1` requests real P-frame GOP, capability-gated (see below).
  `0` rejected as `EncodeError::InvalidInput` (per the field's own contract).
- Real `frame_num`, `idr_pic_flag`/`reference_pic_flag`, `slice_type`, `idr_pic_id`, and one real
  `ReferenceFrames`/`RefPicList0` entry per P-frame.
- `EncSequenceParameter` sent only on IDR frames once GOP mode is active (a coupled, deliberate
  change — see § Decision).
- Capability gate: `VAConfigAttribEncMaxRefFrames` queried via `Display::get_config_attributes`
  before honoring `gop_size > 1`; unsupporting/unqueryable drivers silently fall back to IDR-only
  (`supports_p_frames: bool`, documented on `VaapiVideoEncoder`'s own rustdoc per
  `caveats-and-clarity.md`, matching `VideoEncoderConfig::gop_size`'s own fallback contract).

**Out (deferred, unchanged from ADR-0001 unless noted):**

- VBR/CBR rate control (`VideoEncoderConfig::rate_control` stays unread by this backend — same
  disposition Vulkan gives HEVC/AV1). CQP-only, unchanged.
- VUI/cropping SPS fields (ADR-0001's original deferral, untouched).
- B-frames — permanent non-goal, not just deferred, matching `h264_gop.rs`'s own scope framing.
- Multi-reference (`num_ref_idx_l0_active > 1`), reference-list reordering, long-term references.
- `VideoEncoderConfig::intra_refresh_period` — already silently unread by this backend today
  (pre-existing gap, not newly introduced or widened by this ADR); a real follow-up would need
  its own capability check and `H264EncPicFields`/slice-level wiring this ADR does not design.
- Zero-Copy DMA-BUF surface import — unrelated axis, ADR-0001's own deferral, untouched.
- Resolving the `pic_order_cnt_type` cross-crate interop gap against `mediaway-decoder`'s sibling
  (§ Cross-check point 2) — flagged, not fixed, here.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Re-derive the GOP/`frame_num` state machine from the ITU-T H.264 spec text independently | Rejected per this task's own explicit instruction and this workspace's own precedent (the decode sibling ADR): `vulkan/h264_gop.rs` is real, hardware-verified, already-debugged — re-deriving independently re-risks the same bug classes for no benefit, especially since the source is already GPU-API-agnostic and needs no adaptation surgery. |
| Import `crate::vulkan::h264_gop::GopState` directly (both modules already live in the same crate) | Rejected for the same reason the decoder sibling ADR rejected the equivalent cross-module import: creates a compile-time coupling between two conceptually independent, separately-shippable platform backends (`docs/roadmap.md` still frames Linux/Vulkan encode as distinct backends); porting, not importing, matches this crate's own established precedent (ADR-0001's own decision to keep a local H.264 profile/codec module rather than reuse another backend's). |
| Support full multi-reference / B-frames this increment | Rejected — deliberately mirrors `h264_gop.rs`'s own permanent scope cut and the decoder sibling's identical narrowing; a self-contained single-forward-reference increment is independently useful and independently verifiable, matching this workspace's staged-growth pattern for GOP support on every other backend (Vulkan H.264/HEVC/AV1, D3D12). |
| Keep sending `EncSequenceParameter` on every frame (no coupled change) | Rejected — sending a fresh SPS/PPS ahead of every P-frame (not just at IDR boundaries) risks redundant SPS/PPS NAL emission on every single frame, directly undermining this ADR's own bandwidth-efficiency motivation for moving off all-IDR encode in the first place (ADR-0001's own "All-IDR encoding is bitrate-inefficient" Consequence). Real-world VA-API encoder usage (FFmpeg's `vaapi_encode.c`) sends sequence parameters once per IDR, not per frame — this ADR follows that convention. |
| Widen the decoder's `pic_order_cnt_type` acceptance, or switch this encoder to `pic_order_cnt_type == 0`, to close the cross-check interop gap now | Rejected — either fix is a real, independent, cross-crate design decision (decoder-side scope widening, or encoder-side explicit-POC-signaling addition) outside "encoder-side P-frame GOP support," this ADR's own stated scope. Flagged honestly in § Open questions instead of silently bundled in. |
| Silently downgrade a frame to IDR when its expected reference surface is missing, instead of a hard `EncodeError::Backend` | Rejected — `GopState::decide`'s side effects (DPB bookkeeping, counter advancement) already ran by the time the missing-surface guard fires; retroactively treating the frame as IDR would desync `GopState`'s internal state (which still believes a P-frame was just decided and recorded) from the physical VA-API session. A hard per-call error, with the existing "reopen on `Backend`" disposition, is simpler and provably correct; a resynchronizing recovery path is not designed here and would need its own careful reasoning this ADR chooses not to attempt speculatively. |

## Consequences

### Positive

- Real GOP structures (IPPP...) become bandwidth-efficient to encode on VA-API for the first
  time in this crate — previously every frame carried a full intra-coded picture.
- `frame_num`/reference-list logic is a **cited port** of already-hardware-verified code, not
  fresh spec-derivation — meaningfully lower bug risk than writing this from scratch.
- `gop_size <= 1` (or an unsupporting driver) stays byte-identical to today's shipped behavior —
  zero regression risk to the existing `vaapi_open_and_encode_or_skip_without_hw` test or any
  other default-path caller.
- Found and documented a real lost-reference-surface landmine (§ Real gap found) before it could
  be silently mishandled, and a real cross-crate `pic_order_cnt_type` interop gap (§ Cross-check
  point 2) neither sibling ADR would have surfaced alone.
- Introduces this backend's first "probe first, never assume" capability gate
  (`VAConfigAttribEncMaxRefFrames`), matching the workspace's Vulkan precedent and honoring
  `VideoEncoderConfig::gop_size`'s own documented fallback contract for the first time in this
  crate.

### Negative / Trade-offs

- **Still zero real-hardware verification** for this crate — this ADR's new `frame_num`/
  reference-list logic is exactly as unverified against a real driver as ADR-0001's all-IDR path
  was, now with meaningfully more surface area (cross-frame surface lifetime, reference-picture
  construction) that only a real driver run can confirm.
- Two real open items (`VAConfigAttribEncMaxRefFrames`'s exact name/bit layout, `PictureH264
  ::frame_idx`'s precise encode-side semantics) are inferred, not confirmed against this
  session's real bindgen output — must be closed by the implementation pass's WSL2 compile
  check before this ADR can be considered even compile-verified, let alone hardware-verified.
- The lost-reference-surface guard (§ Real gap found) means a single mid-GOP hardware hiccup now
  fails that `push_frame` call outright rather than degrading gracefully — a real, if honest,
  robustness trade-off versus a (currently undesigned, deliberately not attempted) resynchronizing
  recovery path.
- Leaves the `mediaway-decoder`/`mediaway-encoder` VA-API `pic_order_cnt_type` interop gap
  unresolved — this crate's own P-frame output cannot round-trip through this workspace's own
  VA-API decoder, only through system `ffmpeg`/a real external decoder, until a follow-up ADR
  picks either side of § Cross-check point 2's alternatives.
- `Sps`/`Pps`-equivalent test fixtures in `video_tests.rs` (`tiny_h264_cfg`) will need a
  `gop_size`-varying constructor variant — a real, if mechanical, test-file addition the
  implementation pass must budget for.

## Test plan (for the implementation pass that follows this ADR)

- **Sans-io, hardware-independent (highest-value, run first)**: `linux/vaapi/gop_tests.rs` —
  port/adapt `vulkan`'s existing `h264_gop.rs` coverage (no separate `h264_gop_tests.rs` file
  exists there today; the Vulkan side's GOP cadence is instead exercised end-to-end via
  `vulkan/encoder_tests.rs::push_seven_frames_gop_or_skip` — this ADR should add the sans-io unit
  tier the Vulkan side never got, since VA-API's version is trivially unit-testable with zero
  device dependency): `GopState::new(1)` reproduces all-IDR forever; `GopState::new(3)` produces
  `I P P I P P I` `is_idr` cadence over 7 `decide()` calls; `frame_num` increments and wraps at
  `1 << 16` under `LOG2_MAX_FRAME_NUM_MINUS4 = 12`; `idr_pic_id` increments once per IDR only;
  `decision.reference` is `None` on every IDR and `Some` on every P frame, always pointing at the
  immediately preceding `decide()` call's `setup_slot`.
- **`video.rs` integration** (hardware-gated, `_or_skip_without_hw`-style, expected to skip in
  this session/CI without real `/dev/dri/renderD*`): extend `vaapi_open_and_encode_or_skip_without_hw`
  with a `gop_size = 3`, 7-frame push sequence; scan the resulting Annex-B packets' NAL types
  (this crate presumably needs the same `nal.rs`-style scanner `mediaway-encoder::vulkan` already
  has, or a small equivalent — confirm during implementation) for the expected `I P P I P P I`
  keyframe cadence (`Packet::is_keyframe`, already present on every packet today), mirroring
  `vulkan::encoder_tests.rs::push_seven_frames_gop_or_skip`'s own assertion shape.
- **Lost-reference guard test** (hardware-gated or a constructed-failure unit test if `encode_one`
  can be exercised with a deliberately-poisoned surface slot without real hardware — confirm
  feasibility during implementation): confirms `push_frame` returns `Err(EncodeError::Backend)`,
  not a panic or silent misencode, when a P-frame's expected reference slot has no surface.
- **Oracle validation**: pipe a `gop_size > 1` encoded Annex-B stream through system `ffprobe`/
  `ffmpeg -i` (this workspace's standing test oracle, [ADR-0002](../../../../docs/adr/0002-system-oracle.md))
  to confirm the stream is at least structurally decodable by a real, independent decoder — this
  sidesteps the `pic_order_cnt_type` interop gap against this workspace's *own* VA-API decoder
  (§ Cross-check point 2), which is not expected to succeed on this output and is not this ADR's
  correctness bar.
- **WSL2 real-Linux compile verification** (available this workspace, per
  `docs/ai/wiki/platform/linux-encode.md`): `cargo check`/`cargo test --lib`/`cargo clippy
  --all-targets -- -D warnings` for `mediaway-encoder` on a real Linux target via WSL2 Ubuntu with
  real `libva-dev` — confirms the new `VAConfigAttribEncMaxRefFrames` constant name/type and
  `EncPictureParameterBufferH264`/`EncSliceParameterBufferH264`/`PictureH264` field assumptions
  against the real bindgen output, the two highest-priority open risks this ADR names. Must be run
  before this ADR's implementation pass is considered even compile-verified.
- Default `cargo test --workspace` (no system FFmpeg, no VA-API hardware) must keep passing —
  every new sans-io test above requires neither.

## Open questions / risks (explicit, for whoever picks up the implementation pass)

1. ~~`VAConfigAttribEncMaxRefFrames`'s real enum variant name~~ — **closed same day**: confirmed
   `= 13` (`VAConfigAttribType`) and `VA_ATTRIB_NOT_SUPPORTED = 0x8000_0000` by reading the real
   WSL2-generated `cros-libva` bindgen output directly (see § VA-API-specific plumbing above).
   The attribute's internal packed-value bit layout for the actual max-reference-count number
   remains unconfirmed, but is not needed for this ADR's binary supported/unsupported gate.
2. **`PictureH264::frame_idx`'s precise encode-side semantics** — this ADR assumes it should hold
   the referenced picture's raw `frame_num` (inferred from FFmpeg's `vaapi_encode_h264.c`
   convention), not independently confirmed against a real driver or against libva's own
   `va_enc_h264.h` doc comments this session.
3. **The `pic_order_cnt_type` interop gap against `mediaway-decoder::linux::vaapi`** (§ Cross-check
   point 2) — deliberately left open, either side (widen the decoder, or add explicit POC
   signaling here) is a legitimate independent follow-up.
4. **Whether a real VA-API driver's internal rate-control/reference-management heuristics read
   `intra_period`/`intra_idr_period`/`ip_period` even when every picture's parameters are already
   fully explicit** (this crate's CQP + per-frame explicit parameter buffers already tell the
   driver everything it needs per picture) — this ADR sets them to informationally-correct values
   defensively, but their real effect on any given driver is unconfirmed.
5. **Whether `curr_pic`'s own `flags` field should ever carry `VA_PICTURE_H264_SHORT_TERM_REFERENCE`**
   (this ADR currently designs it as always `0`, matching today's shipped code, with only
   `ReferenceFrames`/`RefPicList0` entries carrying the flag) — inferred as the correct convention
   from typical VA-API encode sample code, not independently confirmed.

## Implementation addendum (2026-08-19)

Implemented per this ADR's tables/algorithm, verbatim-ported `gop.rs`, and the 7-step
`push_frame` sequence. All three WSL2 verification commands (`cargo check`/`cargo clippy
--all-targets -- -D warnings`/`cargo test`, real `libva-dev`) pass; the full `cargo test -p
mediaway-encoder` suite (75 tests) passes, including the new `gop_tests.rs` unit tier and the two
new hardware-gated `video_tests.rs` tests (both soft-skip in this environment, as expected).
Windows `cargo check`/`clippy --workspace --all-targets --all-features -- -D warnings`/`cargo fmt
--check` also pass (this module is `cfg(target_os = "linux")`-gated, compiles as a no-op stub
elsewhere).

Real, if narrow, deviations from this ADR's exact design as written, each with justification:

1. **`VaapiVideoEncoder` gained one field beyond the ZCA sketch: `effective_gop_size: u32`.**
   The sketch's `gop: GopState`/`supports_p_frames: bool` alone cannot tell
   `build_seq_params`/`build_pic_params` (called once per frame, from a single `FrameDecision`)
   whether the *session* is running in real GOP mode at all — an IDR `FrameDecision` looks
   identical whether it is the one-and-only frame of an all-IDR session or just the periodic IDR
   of an active `gop_size > 1` GOP, but the SPS's `intra_period`/`intra_idr_period`/`ip_period`/
   `log2_max_frame_num_minus4` and the picture parameter buffer's `reference_pic_flag` must differ
   between those two cases (and must stay byte-identical to pre-ADR-0002 output in the disabled
   case — a requirement this ADR states explicitly). This field is the cheapest fix; it mirrors
   the struct's existing pattern of copying scalar config values out of `VideoEncoderConfig` at
   `open_cpu` time (`width`/`height`/`bits_per_second` are already copied the same way).
2. **`reference_pic_flag` (`H264EncPicFields`) is gated on `effective_gop_size > 1`, not on
   `decision.is_idr`/`decision.reference.is_some()` individually.** Not spelled out by this ADR
   (only `idr_pic_flag`/`slice_type`/`idr_pic_id`/`ref_pic_list_0[0]` are discussed explicitly).
   The Vulkan porting source's own `h264_params.rs` sets `is_reference = 1` unconditionally for
   every picture — but VA-API's pre-ADR-0002 Stage 1 explicitly set `reference_pic_flag: 0`, and
   this ADR requires `gop_size <= 1` output stay byte-identical to that. Gating on
   `effective_gop_size > 1` reproduces the old `0` in the disabled case and marks every picture
   (IDR and P alike) as a candidate future reference once GOP mode is genuinely active, matching
   `GopState::decide`'s own bookkeeping (every setup slot is recorded regardless of `is_idr`).
3. **`curr_pic`'s own `frame_idx`/`TopFieldOrderCnt`/`BottomFieldOrderCnt` now track
   `decision.frame_num`/`decision.poc`**, rather than staying hardcoded at `0` as pre-ADR-0002.
   This ADR only discusses these fields for *referenced*-picture entries; filled in here by
   analogy to FFmpeg's `vaapi_encode_h264.c` convention (`CurrPic.frame_idx = frame_num`) already
   cited elsewhere in this ADR for the same file. Zero regression risk for the default
   (`gop_size <= 1`) path, where `decision.frame_num`/`decision.poc` are always `0` anyway.
4. **Fixed a latent bug this ADR's own change exposed**: `encode_one` returned a hardcoded
   `Packet { is_keyframe: true, .. }` for every packet. Harmless under Stage 1 (every frame really
   was a keyframe), but wrong once P frames exist. Now `is_keyframe: decision.is_idr`.
5. **`SURFACE_POOL_SIZE` now aliases `super::gop::WORKSPACE_DPB_CAP`** (`const SURFACE_POOL_SIZE:
   usize = super::gop::WORKSPACE_DPB_CAP;`) instead of an independent `4` literal, turning this
   ADR's claimed equality into a compile-time invariant rather than two numbers that could drift.

Open questions status: item 1 (`VAConfigAttribEncMaxRefFrames`/`VA_ATTRIB_NOT_SUPPORTED`) was
already closed before implementation; re-confirmed byte-for-byte against this session's own WSL2
`cargo check`-generated `bindings.rs`. Item 2 (`PictureH264::frame_idx` encode-side semantics) —
the constructor **signature** cited by this ADR was re-confirmed verbatim against the real
vendored `cros-libva` 0.0.13 source (`src/buffer/h264.rs`) during implementation, but the
semantic question itself (does a real driver actually expect `frame_idx == frame_num` for a
reference entry) remains open, unchanged from this ADR's own disposition — no real VA-API driver
was available to test against. Items 3-5 remain open, untouched by this implementation pass.

## References

- [ADR-0001](0001-vaapi-cros-libva-h264-cpu-upload.md) — this crate's all-IDR baseline, the
  "P/B-frame GOP structure" deferred-work quote this ADR actions
- `crates/mediaway-encoder/src/linux/vaapi/video.rs` — current implementation this ADR extends
  (`next_surface` round-robin: lines 52, 248; `build_seq_params`/`build_pic_params`/
  `build_slice_params`: lines 342-467)
- `crates/mediaway-encoder/src/vulkan/h264_gop.rs` — `GopState`/`Dpb`/`DpbSlot`/`FrameDecision`/
  `FrameRequest`/`LOG2_MAX_FRAME_NUM_MINUS4`/`WORKSPACE_DPB_CAP` porting source (hardware-verified
  RTX 4090, see `adr/vulkan/0002`'s 2026-08-05 implementation update)
- `crates/mediaway-encoder/src/vulkan/h264_params.rs` — `build_single_reference_list`/
  `build_reference_info`/`build_frame_structs` — Vulkan-side analogue of this ADR's VA-API
  parameter-buffer wiring, cited for the "only one real reference entry needed" reasoning
- [`crates/mediaway-encoder/adr/vulkan/0002-vulkan-gop-rate-control.md`](../vulkan/0002-vulkan-gop-rate-control.md)
  — porting source's own ADR: design + 2026-08-05 hardware-verified implementation update,
  `Capabilities::supports_p_frames` capability-gating precedent this ADR mirrors
- [`crates/mediaway-decoder/adr/linux/0002-vaapi-h264-p-slice-dpb.md`](../../../mediaway-decoder/adr/linux/0002-vaapi-h264-p-slice-dpb.md)
  — same-session decode-side sibling; source of the `VA_PICTURE_H264_SHORT_TERM_REFERENCE = 8u32`
  confirmation this ADR reuses, and the `pic_order_cnt_type == 0`-only scope this ADR's
  § Cross-check compares against
- `crates/mediaway-decoder/src/linux/vaapi/dpb.rs` — decode-side sibling's own DPB, read for
  structural comparison (§ Cross-check), not a porting source for this ADR
- `crates/mediaway-encoder/src/video.rs` — `VideoEncoderConfig::gop_size`/`rate_control`/
  `intra_refresh_period` (lines 50-79), the cross-backend config surface this ADR wires for the
  first time in this backend
- `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cros-libva-0.0.13\src\buffer\h264.rs`
  — real vendored `cros-libva` 0.0.13 source read directly for `EncPictureParameterBufferH264`/
  `EncSliceParameterBufferH264`/`EncSequenceParameterBufferH264`/`PictureH264` signatures
- `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cros-libva-0.0.13\src\display.rs`
  — `Display::get_config_attributes` (`vaGetConfigAttributes` wrapper), the capability-query
  primitive this ADR uses for `VAConfigAttribEncMaxRefFrames`
  (`src/lib.rs:23`'s `pub use bindings::*;` — why `VAConfigAttribEncMaxRefFrames`'s real name is
  not directly visible in the crates.io source tree and needs a real bindgen-output check)
- FFmpeg `libavcodec/vaapi_encode_h264.c` / `vaapi_encode.c` — inferred (not fetched/read this
  session, unlike the decode sibling's confirmed `vaapi_h264.c` reference) oracle for
  `PictureH264::frame_idx` semantics and the "SPS sent once per IDR, not per frame" convention —
  flagged as an open item to independently confirm during implementation, same disposition as
  every other unconfirmed item in § Open questions
- [`docs/ai/wiki/platform/linux-encode.md`](../../../../docs/ai/wiki/platform/linux-encode.md) ·
  [`docs/ai/wiki/encode/vulkan-h264-gop.md`](../../../../docs/ai/wiki/encode/vulkan-h264-gop.md)
- [`docs/spec/sans-io.md`](../../../../docs/spec/sans-io.md) ·
  [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md) ·
  [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) ·
  [`docs/adr/0002-system-oracle.md`](../../../../docs/adr/0002-system-oracle.md)

ADRs are **English**. Numbering is local to this `adr/` folder.
