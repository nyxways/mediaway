# ADR-0002: VA-API H.264 single-forward-reference P-slice decode (DPB port from `vulkan/dpb.rs`)

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (`src/linux/vaapi/`)

## Context

[ADR-0001](0001-vaapi-h264-cpu-out.md) scoped `mediaway-decoder::linux::vaapi` to **IDR
pictures only** — no DPB, no reference-picture-list construction, no `frame_num`/POC
carry-over across pictures — and named the obvious next step in its own Alternatives table:

> Support general (non-IDR, multi-reference) H.264 decode this session — Would require a
> DPB, reference picture list construction (8.2.4), MMCO/sliding-window marking, and POC
> state across pictures... Deferred to Stage 2 (roadmap).

Re-read against the current file (`crates/mediaway-decoder/adr/linux/0001-vaapi-h264-cpu-out.md`):
that quote is still accurate — nothing in ADR-0001 or the shipped code has since added any of
this. Confirmed directly in `crates/mediaway-decoder/src/linux/vaapi/h264.rs`:

- `NalUnitType::NonIdrSlice => return Err(DecodeError::Unsupported)` (line 335) — every P/B/
  non-IDR-I slice is rejected outright.
- `build_pic_param` hardcodes `0, // num_ref_frames: none kept (IDR-only, no DPB — see
  ADR-0001 § Scope)` (line 418) and fills `ReferenceFrames`/`RefPicList0`/`RefPicList1` with
  `invalid_picture()` (`VA_PICTURE_H264_INVALID`) unconditionally (lines 369, 438-439).
- `Pipeline.surfaces: Vec<Option<Surface<()>>>` is a plain **round-robin** ring
  (`next_surface = (next_surface + 1) % len`, `take_surface_slot`, lines 45-46, 183-189) — no
  slot is ever protected from being overwritten while still needed as a reference. This is
  correct for IDR-only decode (nothing is ever referenced) and would silently corrupt output
  the moment a real reference frame's surface got recycled mid-GOP.
- `Sps::parse` (`sps.rs:70`) and `Pps::parse` (`pps.rs:68`) already **read** but **discard**
  `max_num_ref_frames`, `num_ref_idx_l0_default_active_minus1`, `weighted_pred_flag`, and
  `weighted_bipred_idc` — the bits are consumed correctly (no bit-position bug for IDR-only
  streams, which never need these), but nothing is retained for a future P-slice path.

This ADR designs (and this session actions) the extension to accept **single-forward-reference
P-slices**: `frame_num` tracking, sliding-window DPB eviction, `pic_order_cnt_type == 0` POC
derivation across pictures, and reference-picture-list construction for exactly one active L0
reference — explicitly **not** full multi-reference/B-frame/reference-reordering decode.

### Why this ADR ports `mediaway-decoder-vulkan`'s DPB math instead of re-deriving it

`crates/mediaway-decoder/src/vulkan/dpb.rs` and
`crates/mediaway-decoder/src/vulkan/h264_slice.rs` are a **real, hardware-verified**
implementation of exactly the ITU-T H.264 §8.2.4 (`FrameNumWrap`/sliding-window marking) and
§8.2.5.3 (DPB eviction) arithmetic this task needs — confirmed hardware-verified against a real
NVIDIA RTX 4090 (`adr/vulkan/0001-vulkan-video-decode.md`'s 2026-07-30 addenda: real multi-frame
H.264 GOP decode, with two real spec-compliance bugs already found and fixed by cross-checking
FFmpeg's `vulkan_decode.c`/`vulkan_h264.c` field-by-field). Re-deriving this arithmetic from the
ITU-T H.264 spec text independently for VA-API would re-risk bugs that are already found, fixed,
and tested in this exact crate. Both `mediaway-decoder::linux` and `mediaway-decoder::vulkan`
are modules of the **same** `mediaway-decoder` crate today (post crate-merge — `src/lib.rs`
declares `pub mod linux;` and `pub mod vulkan;` side by side), so the source is directly
readable and citable line-for-line; this ADR does **not** import `crate::vulkan::dpb` at
runtime (see § Alternatives Considered for why), but every new type/function below is a
deliberate, cited **port** of specific functions in that file, not independent reinvention.

### Zero real-hardware verification remains the honest baseline

Re-confirmed: ADR-0001 makes **no** hardware-verification claim for
`mediaway-decoder::linux::vaapi` — every VA-API call path was "compile-verified on Linux (WSL2
Ubuntu 24.04 via `cargo check`/`cargo test`/`cargo clippy`, real `libva-dev` headers/bindgen
output)" but never run against `Display::open()`/`vaInitialize` on a real device (the WSL2
instance available has broken VA-API, `vainfo` segfaults, no real GPU exposed). `Cargo.toml`
confirms the dependency shape that made that WSL2 compile verification possible:

```toml
[target.'cfg(target_os = "linux")'.dependencies]
cros-libva = { workspace = true }
```

— a `cfg(target_os = "linux")`-gated target dependency, so WSL2 Ubuntu (a real Linux target,
even without a working VA-API device) already exercises real `bindgen` output against real
system `libva-dev` headers at compile time. This ADR's new work (this session, no shell/Bash
tool available) ships the same way: **design only**, no `.rs` files written this pass. The
concrete first implementation pass that follows this ADR must run
`cargo check -p mediaway-decoder --target x86_64-unknown-linux-gnu` (or equivalent WSL2
`cargo check`) before claiming even compile correctness for the new `VA_PICTURE_H264_*`
constant names this ADR assumes exist (see § VA-API-specific plumbing below) — flagged
explicitly as unconfirmed, not silently assumed.

## Decision

> Extend `mediaway-decoder::linux::vaapi` to decode **single-forward-reference P-slices**
> (`RefPicList0[0]` only, `num_ref_idx_l0_active` fixed at exactly `1`), non-IDR I-slices (a
> free byproduct of moving off the `NalUnitType::IdrSlice`-only dispatch gate — zero reference
> management, same as IDR I-slices minus the IDR-specific `dec_ref_pic_marking()` form and DPB
> reset), by porting `mediaway-decoder::vulkan`'s sliding-window DPB and POC arithmetic into a
> new, crate-local, sans-io `linux/vaapi/dpb.rs`. **No B-slices, no reference-list reordering,
> no long-term references, no weighted prediction, no CABAC entropy coding for P-slices, no
> field pictures** — this narrower scope deliberately mirrors
> `mediaway-encoder`'s own `vulkan/h264_gop.rs` P-frame GOP scope cut (see § Alternatives
> Considered).

### Precise porting plan: which Vulkan functions map to which new VA-API-side functions

New file `crates/mediaway-decoder/src/linux/vaapi/dpb.rs`, sans-io
(`#![forbid(unsafe_code)]`, no `cros_libva` types), unit-testable without any VA-API device —
mirrors `vulkan/dpb.rs`'s own "no Vulkan/GPU calls anywhere in this file" doc-comment claim:

| New (`linux/vaapi/dpb.rs`) | Ported from (cited source) | Change from source |
|---|---|---|
| `H264_MAX_DPB_SLOTS: usize = 16` | `vulkan/dpb.rs:31` | Verbatim (spec-defined constant, not Vulkan-specific) |
| `DpbSlot { frame_num, frame_num_wrap, pic_order_cnt, used_for_reference }` | `vulkan/dpb.rs:41-62` (`DpbSlot` struct) | Verbatim field set — **drop** nothing here (this struct already has zero GPU-handle fields) |
| `compute_frame_num_wrap(frame_num, current_frame_num, max_frame_num) -> i32` | `vulkan/dpb.rs:89-99` | Verbatim (pure ITU-T H.264 §8.2.4.1 arithmetic, no Vulkan dependency) |
| `Dpb { slots: Vec<Option<DpbSlot>> }` | `vulkan/dpb.rs:141-148` (`Dpb` struct) | **Drop** the `outstanding: Vec<bool>` parallel array entirely — see rationale below |
| `Dpb::new(capacity)` | `vulkan/dpb.rs:154-161` | Verbatim minus `outstanding` init |
| `Dpb::occupied_slots()` | `vulkan/dpb.rs:187-193` | Verbatim |
| `Dpb::sliding_window_evict_target()` | `vulkan/dpb.rs:266-272` | Verbatim (ITU-T H.264 §8.2.5.3) |
| `Dpb::refresh_frame_num_wraps(current_frame_num, max_frame_num)` | `vulkan/dpb.rs:274-289` | Verbatim (ITU-T H.264 §8.2.4.1) |
| `Dpb::clear_all()` | `vulkan/dpb.rs:291-307` | Simplified: no `SlotOutstanding` failure path possible once `outstanding` is dropped, so this becomes infallible (`fn clear_all(&mut self)`, not `Result<(), DpbError>`) |
| `Dpb::allocate_slot()` | `vulkan/dpb.rs:309-332` | Same free-slot-or-evict logic; only failure mode kept is `DpbError::NoFreeSlot` (unreachable given `+1` sizing below, kept as an explicit error rather than a `panic!`/`unwrap`, matching the source's own reasoning at `vulkan/dpb.rs:314-320`) |
| `Dpb::insert(index, slot)` / `Dpb::evict(index)` | `vulkan/dpb.rs:232-260` | Simplified: no `SlotOutstanding` check, `DpbError::InvalidSlotIndex` only |
| `derive_pic_order_cnt_msb(pic_order_cnt_lsb, prev_msb, prev_lsb, max_pic_order_cnt_lsb) -> i32` | `vulkan/h264_params.rs:455-471` | Verbatim (ITU-T H.264 §8.2.1.1, zero Vulkan dependency) |
| `default_ref_pic_list0(dpb: &Dpb) -> Vec<usize>` | `vulkan/h264_slice.rs:270-278` | Verbatim (ITU-T H.264 §8.2.4.2.1, short-term-only case) — kept as the full sorted list, not hand-trimmed to "just compute the max," so the exact same, already-reasoned-about function is reused; this crate's caller only ever reads `.first()` |
| *(none — explicit non-port)* | `vulkan/h264_slice.rs:287-335` (`apply_ref_pic_list_modifications`) | **Not ported.** Any P-slice signaling `ref_pic_list_modification_flag_l0 == 1` is rejected as `DecodeError::Unsupported` at slice-header parse time instead — this ADR's scope has no reordering to apply |

#### Why `outstanding`/`SlotOutstanding` is dropped, not ported

`vulkan/dpb.rs`'s `Dpb` couples reference-management bookkeeping with **Zero-Copy handle
backpressure**: a slot whose GPU image a caller still holds via
`GpuBufferHandle::Vulkan` must never be silently recycled (`vulkan/dpb.rs:16-19`, `106-136`).
`mediaway-decoder::linux::vaapi` has **no Zero-Copy output path at all** —
`VideoOutputPreference::ZeroCopyGpu` already returns `DecodeError::Unsupported`
unconditionally (`h264.rs:69-72`, unchanged by this ADR) — every frame's NV12 pixels are copied
into an owned `Bytes` via `copy_nv12_from_planes` **before** `decode_one` returns
(`h264.rs:259-267`). By the time a `VideoFrame` reaches a caller, the underlying VA-API
`Surface`'s slot is already free to recycle — there is no dangling-handle risk class to guard
against here, so porting `outstanding`/`mark_outstanding`/`clear_outstanding`/
`DpbError::SlotOutstanding` would add real code with no reachable failure mode. Flagged
explicitly (not silently trimmed) since it is the one deliberate structural divergence from the
porting source.

### VA-API-specific plumbing (distinct from the ported DPB math above)

This is the genuinely new, VA-API-side design work — the `cros-libva` API surface calls that
turn `Dpb` state into `VAPictureParameterBufferH264`/`VASliceParameterBufferH264` fields.

**Confirmed by reading `cros-libva` 0.0.13's real vendored source directly**
(`C:\Users\User\.cargo\registry\src\...\cros-libva-0.0.13\src\buffer\h264.rs`, not paraphrased):

- `PictureH264::new(picture_id: VASurfaceID, frame_idx: u32, flags: u32, top_field_order_cnt:
  i32, bottom_field_order_cnt: i32) -> Self` (`h264.rs:14-29`) — already used today for
  `curr_pic` and the all-invalid `ReferenceFrames`/`RefPicList0`/`RefPicList1` padding. Takes a
  **raw `u32` flags** bitmask, not a typed enum — this crate must pass real
  `VA_PICTURE_H264_*` constant values, not invent its own.
- `PictureParameterBufferH264::new(curr_pic, reference_frames: [PictureH264; 16], …,
  num_ref_frames: u8, …)` (`h264.rs:134-184`) — the `reference_frames` array is **always fixed
  size 16** regardless of how many are actually occupied; unused entries already use
  `invalid_picture()` today and continue to for slots beyond the DPB's real occupancy. No
  signature change needed — only the **values** passed change.
- `SliceParameterBufferH264::new(...)` — already takes `ref_pic_list_0: [PictureH264; 32]`,
  `ref_pic_list_1: [PictureH264; 32]`, `num_ref_idx_l0_active_minus1: u8`-equivalent positional
  field (currently hardcoded `0` with the comment `// num_ref_idx_l0_active_minus1: unused (I
  slice)`, `h264.rs:463`) — again, no signature change, only real values for P slices.

**Confirmed this session (addendum below) — was flagged unverified in the first draft of this
ADR, then closed the same day**: `VA_PICTURE_H264_SHORT_TERM_REFERENCE` is a real, present
bindgen-generated constant. `cros-libva`'s `src/lib.rs` re-exports `pub use bindings::*;`
(confirmed by reading `lib.rs:23`) — `bindings` is `bindgen`-generated from the **real system**
`va_dec_h264.h`/`va.h` headers at build time (not checked into the crates.io source tree). A
real WSL2 `cargo check -p mediaway-decoder --target x86_64-unknown-linux-gnu` build was run and
its generated `target/.../build/cros-libva-*/out/bindings.rs` read directly:

```rust
pub const VA_PICTURE_H264_INVALID: u32 = 1;
pub const VA_PICTURE_H264_TOP_FIELD: u32 = 2;
pub const VA_PICTURE_H264_BOTTOM_FIELD: u32 = 4;
pub const VA_PICTURE_H264_SHORT_TERM_REFERENCE: u32 = 8;
pub const VA_PICTURE_H264_LONG_TERM_REFERENCE: u32 = 16;
```

— matching this ADR's FFmpeg-sourced inference exactly (name, and a plausible power-of-two
bitflag value alongside the already-used `VA_PICTURE_H264_INVALID`). FFmpeg's
`libavcodec/vaapi_h264.c` (`fill_vaapi_pic`) sets exactly

```c
if (pic->reference)
    va_pic->flags |= pic->long_ref ?
        VA_PICTURE_H264_LONG_TERM_REFERENCE :
        VA_PICTURE_H264_SHORT_TERM_REFERENCE;
```

and sets `num_ref_frames` directly from `sps->ref_frame_count` (i.e. the SPS's own
`max_num_ref_frames`, a static per-stream value — **not** a per-picture "currently occupied
count"). This ADR adopts the same two conventions: `VA_PICTURE_H264_SHORT_TERM_REFERENCE` for
every occupied DPB slot (this ADR never marks a long-term reference — out of scope, matches
`vulkan/h264_slice.rs`'s own `RefPicListModification::idc == 2` rejection), and
`num_ref_frames` set from `sps.max_num_ref_frames` directly (not `dpb.occupied_slots().count()`).
This closes this ADR's single highest-priority open risk (§ Open questions #1) before the
implementation pass even starts.

### `VaapiH264Decoder` struct shape (ZCA sketch — ownership, no `Box`/`dyn`)

```rust
// linux/vaapi/dpb.rs — new file, sans-io, no cros_libva types.
pub(super) struct DpbSlot {
    frame_num: u32,
    frame_num_wrap: i32,
    pic_order_cnt: i32,
    used_for_reference: bool,
}
pub(super) struct Dpb {
    slots: Vec<Option<DpbSlot>>, // sized once in ensure_pipeline, not per-frame
}

// linux/vaapi/h264.rs — changed struct shape.
struct Pipeline {
    _config: Config,
    context: Rc<Context>,
    surfaces: Vec<Option<Surface<()>>>, // unchanged storage; now DPB-slot-indexed
    dpb: Dpb,                           // NEW — replaces `next_surface: usize`
    coded_width: u32,
    coded_height: u32,
    nv12_format: VAImageFormat,
    max_num_ref_frames: u32,            // NEW — from Sps, needed by build_pic_param's
                                         // num_ref_frames field (see FFmpeg convention above)
}

pub(crate) struct VaapiH264Decoder {
    display: Rc<Display>,
    pipeline: Option<Pipeline>,
    sps: Option<Sps>,
    pps: Option<Pps>,
    info: StreamInfo,
    declared_width: u32,
    declared_height: u32,
    pending: VecDeque<VideoFrame>,
    flushed: bool,
    prev_poc_msb: i32,   // NEW — carried across pictures, reset to 0 on every IDR
    prev_poc_lsb: u32,   // NEW — mirrors vulkan::decoder::H264Session's identical pair
                          // (decoder.rs:99-100)
}
```

- `next_surface: usize` (round-robin ring index) is **removed** — `Dpb::allocate_slot()`
  entirely replaces it as the slot-selection strategy. `take_surface_slot` becomes fallible
  (`Result<(usize, Surface<()>), DecodeError>`, mapping `DpbError::NoFreeSlot` /
  `InvalidSlotIndex` to `DecodeError::Backend`) rather than infallible modulo arithmetic.
- `SURFACE_POOL_SIZE: usize = 4` (`h264.rs:34`) is **removed as a fixed pool size** and replaced
  by a per-stream computed value at `ensure_pipeline` time:
  `(sps.max_num_ref_frames + 1).clamp(1, H264_MAX_DPB_SLOTS)` — verbatim port of
  `vulkan/decoder.rs:306-308`'s own sizing comment ("+1: room for the picture currently being
  decoded alongside every active short-term reference"). `Dpb::new(pool_size)` and
  `create_surfaces(..., vec![(); pool_size])` both use this one computed value — no drift
  between the two arrays' lengths is possible by construction (same `pool_size` local feeds
  both).
- No `Box<dyn _>`/`dyn Trait` anywhere in this design — `Dpb`/`DpbSlot` are closed, concrete
  structs, matching every other decode/encode backend in this workspace (Vulkan, D3D12,
  encoder's `h264_gop.rs`).
- `Vec<Option<DpbSlot>>` (not `SmallVec`): DPB capacity is computed once per session at
  `ensure_pipeline` time (not per-frame), same one-time-allocation shape
  `vulkan/dpb.rs:154-161`'s own `Dpb::new` already uses and already documents as deliberate
  ("a `Vec` allocated once at session-open time, not per-frame").

### Per-picture decode ordering (the correctness-critical sequencing)

Ported directly from `vulkan/decoder.rs::decode_slice_h264` (lines 393-520), same order, same
rationale — **reference lists must be built from the DPB's state before `allocate_slot()` is
called**, since `allocate_slot()` may sliding-window-evict a reference slot as a side effect;
building the list first guarantees the destination slot is never accidentally treated as its own
reference (this is only safe because the DPB is sized `max_num_ref_frames + 1`, guaranteeing a
genuinely free slot exists whenever `num_ref_idx_l0_active` references are all still occupied —
same invariant `vulkan/decoder.rs:306-308`'s sizing comment relies on):

1. Parse slice header (extended `SliceHeader`, see below).
2. If IDR: `pipeline.dpb.clear_all()`; reset `self.prev_poc_msb = 0; self.prev_poc_lsb = 0`
   (ports `vulkan/decoder.rs:409-413`).
3. `pipeline.dpb.refresh_frame_num_wraps(slice.frame_num, max_frame_num)` (ports
   `vulkan/decoder.rs:414-416`).
4. `poc_msb = derive_pic_order_cnt_msb(...)`; `pic_order_cnt = poc_msb + slice.pic_order_cnt_lsb`;
   if this picture is a reference (`nal_ref_idc != 0`), update `self.prev_poc_msb`/`prev_lsb`
   (ports `vulkan/decoder.rs:418-429`).
5. **Before allocating the destination slot**: if `slice.slice_type == P`, look up
   `default_ref_pic_list0(&pipeline.dpb).first()` (single-forward-reference: only index 0 is
   ever read) and resolve its `(surface_id, DpbSlot)` via `pipeline.surfaces[idx].as_ref()` +
   `pipeline.dpb.slot(idx)`. Build the full `ReferenceFrames` array from every occupied DPB slot
   the same way, for `PictureParameterBufferH264` (VA-API wants the full DPB, not just the
   active list — matches the current code's `[PictureH264; 16]` shape and FFmpeg's
   `fill_vaapi_ReferenceFrames` convention).
6. `dst_slot_index = pipeline.dpb.allocate_slot()?` (ports `vulkan/decoder.rs:450`); take the
   physical `Surface<()>` at that index (`pipeline.surfaces[dst_slot_index].take()`).
7. Build `pic_param`/`slice_param` from the resolved reference(s) + `dst_slot_index`'s surface
   as `CurrPic`; run the existing `vaBeginPicture → vaRenderPicture → vaEndPicture →
   vaSyncSurface` sequence unchanged (no VA-API call-order change — only the parameter buffer
   **contents** change).
8. On success: `pipeline.surfaces[dst_slot_index] = Some(returned_surface)`; if this picture is
   a reference, `pipeline.dpb.insert(dst_slot_index, DpbSlot::new(slice.frame_num,
   frame_num_wrap, pic_order_cnt))` (ports `vulkan/decoder.rs:512-519`).

### Bitstream-parser changes — real gaps found by reading the porting source carefully

Reading `vulkan/h264_slice.rs`'s P-slice parser (the porting source for slice-header field
order) against the full ITU-T H.264 §7.3.3 `slice_header()` syntax surfaced **two real, latent
gaps in that same porting source** that this ADR's new VA-API-side parser must not silently
inherit:

1. **No `pred_weight_table()` handling.** §7.3.3's syntax reads `pred_weight_table()` right
   after `ref_pic_list_modification()` whenever `weighted_pred_flag && slice_type == P` (or the
   B-slice equivalent, irrelevant here). Neither `vulkan/h264_slice.rs` nor the current VA-API
   I-slice-only parser reads or skips this — for I-slices this is harmless (the condition can
   never be true), but a real P-slice stream with `pps.weighted_pred_flag == 1` would misparse
   every bit from that point on if silently ignored. **Fix for this ADR**: `Pps::parse` must
   retain `weighted_pred_flag` (currently read and discarded at `pps.rs:70`); the new P-slice
   header parser rejects (`DecodeError::Unsupported`) whenever `pps.weighted_pred_flag` is set,
   *before* attempting to parse further — a hard, honest scope cut, not a misparse.
2. **No `cabac_init_idc` handling.** §7.3.3 reads `cabac_init_idc` (`ue(v)`) whenever
   `entropy_coding_mode_flag && slice_type != I && slice_type != SI` — again absent from
   `vulkan/h264_slice.rs`, again harmless there only because I-slices never hit the condition.
   **Fix for this ADR**: `Pps::parse` must retain `entropy_coding_mode_flag` (already retained
   today, `pps.rs:22` — unused for I-slices, since I-slice headers have zero entropy-mode-gated
   fields). The new P-slice header parser rejects (`DecodeError::Unsupported`) whenever
   `pps.entropy_coding_mode_flag` is set on a P slice — CABAC P-slice decode is not a VA-API
   driver limitation (the driver's `VAEntrypointVLD` handles CABAC entropy decode identically to
   CAVLC once handed correct parameter buffers), purely a "this session's bitstream-header
   parser doesn't read `cabac_init_idc` yet" scope cut, named honestly rather than silently
   producing a misaligned `slice_data_bit_offset`.

Both gaps are flagged in § Open Questions as a suggested (not actioned this ADR) follow-up for
`vulkan/h264_slice.rs` itself, since that file has the same latent gap for any real P-slice
stream using either feature.

`SliceHeader` (`linux/vaapi/slice.rs`) gains, mirroring `vulkan/h264_slice.rs`'s field set where
applicable, adapted to this ADR's narrower single-ref/no-reordering scope:

- `is_idr: bool` (derived from the caller's `NalUnitType`, not itself a bitstream field).
- For P slices only: `num_ref_idx_active_override_flag` read but not retained (only used to
  decide whether to read `num_ref_idx_l0_active_minus1` or use `pps.num_ref_idx_l0_default_active`);
  resulting `num_ref_idx_l0_active` **rejected as `Unsupported` unless it equals exactly `1`**
  (stricter than `vulkan/h264_slice.rs`, which allows any count but only builds `RefPicList0` —
  this ADR's VA-API slice parameter buffer has no MB-level `ref_idx_l0` tracking of its own, so
  a driver-visible `num_ref_idx_l0_active_minus1 > 0` with only one real reference populated
  would be a latent bug if a real stream ever set a per-MB `ref_idx_l0 > 0`; rejecting outright
  is the honest, safe scope cut, matching `h264_gop.rs`'s encoder-side precedent of never
  emitting more than one active reference in the first place).
- `ref_pic_list_modification_flag_l0`: read; if `true`, reject as `Unsupported` (no reordering
  support this ADR — `apply_ref_pic_list_modifications` is explicitly not ported, see table
  above).
- `dec_ref_pic_marking()` non-IDR form: `adaptive_ref_pic_marking_mode_flag` read; if `true`,
  reject as `Unsupported` (sliding-window only, ports `vulkan/h264_slice.rs:244-260`'s identical
  scope cut and rejection reason).

`Pps` (`linux/vaapi/pps.rs`) gains `num_ref_idx_l0_default_active: u32` (stored as
`minus1 + 1`, matching `vulkan/h264_params.rs`'s `H264Pps::num_ref_idx_l0_default_active`
convention) and `weighted_pred_flag: bool` — both currently parsed and discarded
(`pps.rs:68,70`).

`Sps` (`linux/vaapi/sps.rs`) gains `max_num_ref_frames: u32` — currently parsed and discarded
(`sps.rs:70`) — used both for `Dpb`/surface-pool sizing and `VAPictureParameterBufferH264::num_ref_frames`.

### Errors: extend, don't replace, the existing `map_err`-to-`DecodeError` pattern

No new *public* error type. `linux/vaapi/dpb.rs` gets its own small, crate-internal
(`pub(super)`) `thiserror` enum mirroring `vulkan/dpb::DpbError`'s shape minus
`SlotOutstanding`:

```rust
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(super) enum DpbError {
    #[error("no free DPB slot available (capacity {capacity})")]
    NoFreeSlot { capacity: usize },
    #[error("DPB slot index {index} out of range (capacity {capacity})")]
    InvalidSlotIndex { index: usize, capacity: usize },
}
```

Mapped to `DecodeError::Backend` at every `h264.rs` call site (an internal invariant violation,
not a data/input problem) — same disposition `vulkan/decoder.rs` gives `DpbError` today via its
own `map_err`, and the same "reuse `DecodeError` as-is, no decode-specific variant" decision
`adr/vulkan/0001-vulkan-video-decode.md`'s own § "Errors" already made and reasoned about for
this exact kind of DPB failure.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Import `crate::vulkan::dpb::Dpb` directly (both modules already live in the same crate) | Rejected: creates a compile-time coupling between two conceptually independent platform backends that happen to share one crate today for organizational reasons — `docs/roadmap.md` still describes `mediaway-decoder-vulkan`/`mediaway-decoder-linux` as if separately shippable, and this workspace's crate-packaging philosophy keeps platform backends separable. It would also carry the Zero-Copy `outstanding`/`SlotOutstanding` machinery this backend has no use for (see § Why `outstanding` is dropped). `linux::vaapi`'s own module doc already states its parser reuse philosophy as "own... local" (ADR-0001's own Alternatives row on not reusing `mediaway_sw::h264::{Sps,Pps}`) — porting, not cross-module importing, is consistent with that established precedent. |
| Re-derive the DPB/POC arithmetic from the ITU-T H.264 spec text independently, without reference to `vulkan/dpb.rs` | Rejected per this task's own explicit instruction: `vulkan/dpb.rs`/`h264_slice.rs` are real, hardware-verified, already-debugged (two real spec-compliance bugs already found and fixed there) — re-deriving independently would re-risk the same bug classes for no benefit. |
| Support full multi-reference (`num_ref_idx_l0_active > 1`) and/or B-slices this increment | Rejected — deliberately mirrors `mediaway-encoder`'s own `vulkan/h264_gop.rs` scope cut ("single forward reference only... no B-frames... permanent non-goal, not just deferred", `h264_gop.rs:8-11`). A first P-slice increment that only ever needs `RefPicList0[0]` is a self-contained, independently useful, and independently verifiable unit; multi-reference/B-frame support is a distinct, larger follow-up (own ADR) once this lands and is real-hardware-verified. |
| Support `ref_pic_list_modification()` reordering this increment | Rejected — no real single-forward-reference stream needs reordering (there is only one candidate to reorder); `apply_ref_pic_list_modifications` exists in the porting source only because Vulkan's own scope is a strict superset (any `num_ref_idx_l0_active`). Out of scope here; any stream signaling the flag is rejected honestly rather than silently mishandled. |
| Keep the round-robin `next_surface` ring and add a *separate* reference-tracking side table instead of replacing it with `Dpb::allocate_slot()` | Rejected — round-robin and sliding-window eviction are mutually exclusive slot-selection strategies; keeping both would mean either the ring silently overwrites a still-referenced surface (real corruption bug) or the ADR would need a third, ad hoc reconciliation layer between them. Replacing `next_surface` outright with the ported `Dpb::allocate_slot()` is strictly simpler and is exactly what `vulkan/decoder.rs`'s own design already does (one allocator, no separate ring). |
| Reuse `mediaway_sw::h264::SliceHeader`/`Sps`/`Pps` for the P-slice fields instead of extending this crate's own local parser | Rejected for the same reason ADR-0001 already rejected it for the I-slice case: that crate's types intentionally discard several raw syntax elements (`log2_max_frame_num_minus4`, `pic_order_cnt_type`, PPS `weighted_pred_flag`/`entropy_coding_mode_flag`) this crate's VA-API parameter buffers require — unchanged reasoning, now also true for the new P-slice-specific fields (`num_ref_idx_l0_default_active_minus1`, `ref_pic_list_modification_flag_l0`). |
| Support weighted prediction / CABAC P-slices this increment (parse `pred_weight_table()` / `cabac_init_idc` properly instead of rejecting) | Rejected — both are real, separate parsing surfaces with their own bug risk (weighted-prediction table dimensions depend on `chroma_format_idc` and luma/chroma weight-denominator fields this crate's SPS/PPS parsers do not currently retain; CABAC P-slice support needs no new *bit-position* logic beyond `cabac_init_idc` itself, but combining it with a first DPB increment adds an unrelated axis of risk to an already-large change). Rejecting both honestly (see § Bitstream-parser changes) keeps this increment's real surface area to "sliding-window DPB + single-reference P-slices," matching the task's own scope framing. |

## Consequences

### Positive

- Real GOP structures (IPPP...) become decodable on VA-API for the first time in this
  crate — previously only all-intra/keyframe-only streams worked at all.
- DPB/POC/sliding-window logic is a **cited port** of already-hardware-verified code, not fresh
  spec-derivation — meaningfully lower bug risk than writing this from scratch, per this ADR's
  own stated goal.
- The new `linux/vaapi/dpb.rs` stays sans-io and unit-testable without any VA-API device or
  driver, mirroring `vulkan/dpb_tests.rs`'s standalone testability — real, non-hardware-gated
  test coverage is possible immediately, before any hardware access is available.
- Found and fixed two real latent gaps (`pred_weight_table()`, `cabac_init_idc`) in the porting
  source itself before they could be silently inherited — a genuine, if narrow, correctness
  improvement over blind copy-paste porting.
- `Dpb`/surface-pool sizing (`max_num_ref_frames + 1`, capped at `H264_MAX_DPB_SLOTS`) reuses an
  already-reasoned-about, hardware-verified invariant (`vulkan/decoder.rs:306-308`) rather than
  guessing a pool size.

### Negative / Trade-offs

- **Still zero real-hardware verification** for this crate (see § Zero real-hardware
  verification remains the honest baseline) — this ADR's own new DPB/reference-list logic is
  exactly as unverified against a real driver as ADR-0001's IDR-only path was, now with
  meaningfully more surface area (reference-picture-list construction, sliding-window eviction,
  cross-picture POC state) that a real driver could reject or silently mishandle in ways no
  amount of sans-io unit testing can catch.
- Narrower than `mediaway-decoder::vulkan`'s own P-slice scope in two ways: this ADR rejects
  `num_ref_idx_l0_active > 1` outright (Vulkan's `h264_slice.rs` tolerates it, only ever
  building a `RefPicList0`), and rejects weighted prediction / CABAC P-slices entirely (Vulkan's
  parser silently mishandles rather than rejects both — a gap this ADR does *not* fix in the
  Vulkan sibling, only documents, see § Open Questions).
- Restructures the existing `IdrSlice` dispatch branch (unifies it with the new `NonIdrSlice`
  path through one shared per-picture pipeline) rather than leaving it untouched and adding a
  parallel P-slice-only path — a larger diff than a purely additive change, but avoids
  duplicating DPB bookkeeping in two call sites (see § Alternatives Considered on the
  round-robin-vs-DPB question).
- `Sps`/`Pps`/`SliceHeader` test fixture constructors in `sps_tests.rs`/`pps_tests.rs`/
  `h264_tests.rs` (e.g. `test_sps()`, `test_pps()`, `test_header()`) will all need new fields —
  a real, if mechanical, test-file churn cost the implementation pass must budget for.

## Test plan (for the implementation pass that follows this ADR)

- **Sans-io, hardware-independent (highest-value, run first)**: `linux/vaapi/dpb_tests.rs` —
  port/adapt `vulkan/dpb_tests.rs`'s and `vulkan/h264_slice_tests.rs`'s test cases for
  `compute_frame_num_wrap`, `sliding_window_evict_target`, `refresh_frame_num_wraps`,
  `allocate_slot` (free-slot and eviction paths), `clear_all`, `derive_pic_order_cnt_msb`
  (including the IDR-reset and MSB-wrap cases), `default_ref_pic_list0`. Zero VA-API device
  needed — same tier as this crate's existing `sps_tests.rs`/`pps_tests.rs`/`slice_tests.rs`.
- **Slice-header parser extension**: new cases in `slice_tests.rs` for the P-slice branch
  (`num_ref_idx_l0_active` override on/off, rejection when `!= 1`, rejection on
  `ref_pic_list_modification_flag_l0 == 1`, rejection on `adaptive_ref_pic_marking_mode_flag ==
  1`, rejection on `pps.weighted_pred_flag`/`pps.entropy_coding_mode_flag` for P slices,
  correct `bits_consumed` for a hand-computed P-slice header — mirrors `slice_tests.rs`'s
  existing hand-computed-bit-count regression style for IDR slices).
- **`h264.rs` integration** (still hardware-gated, `open_vaapi_h264_cpu_or_skip`-style, expected
  to skip in this session/CI without real `/dev/dri/renderD*`): extend with a hand-constructed
  SPS+PPS+IDR+P Annex-B stream (small resolution, `I_PCM`/`P_Skip`-style macroblocks if a real
  CAVLC residual encoder is still out of scope for this crate — same technique
  `adr/vulkan/0001-vulkan-video-decode.md`'s own `tests/hardware_h264_decode.rs` used to get
  controllable content without a real encoder) exercising: DPB slot reuse across 3+ pictures,
  sliding-window eviction actually firing (stream longer than `max_num_ref_frames + 1`
  pictures), and an IDR mid-stream correctly clearing prior references.
- **WSL2 real-Linux compile verification** (available this workspace, per
  `docs/ai/wiki/platform/linux-decode.md` and this ADR's own § Zero real-hardware verification):
  `cargo check`/`cargo test --lib`/`cargo clippy --all-targets -- -D warnings` for
  `mediaway-decoder` on a real Linux target via WSL2 Ubuntu with real `libva-dev` — confirms the
  new `VA_PICTURE_H264_SHORT_TERM_REFERENCE` constant name/type against the real bindgen output,
  the single highest-priority open risk this ADR names (see Negative/Trade-offs). Must be run
  before this ADR's implementation pass is considered even compile-verified, let alone
  hardware-verified.
- Default `cargo test --workspace` (no system FFmpeg, no VA-API hardware) must keep passing —
  every new sans-io test above requires neither.

## Open questions / risks (explicit, for whoever picks up the implementation pass)

1. ~~`VA_PICTURE_H264_SHORT_TERM_REFERENCE`'s real name/value~~ — **closed same day**: confirmed
   `= 8` (`u32`) by reading the real WSL2-generated `cros-libva` bindgen output directly (see §
   VA-API-specific plumbing above).
2. **`vulkan/h264_slice.rs`'s own latent `pred_weight_table()`/`cabac_init_idc` gaps** — found
   while reading it as this ADR's porting source (see § Bitstream-parser changes), not fixed
   there (out of this ADR's crate-local scope for `linux/vaapi`). Worth a small, separate,
   code-grounded follow-up ADR/PR against `mediaway-decoder::vulkan` if a real CABAC or
   weighted-prediction P-slice stream is ever exercised against that backend.
3. **Whether a real VA-API driver actually requires `ReferenceFrames` to list *every* occupied
   DPB slot, or only the active `RefPicList0`/`RefPicList1` entries** — this ADR follows
   FFmpeg's `fill_vaapi_ReferenceFrames` convention (every occupied slot, regardless of whether
   it is in the active list) as the safer default, but this is inferred from FFmpeg's behavior,
   not confirmed against this workspace's own driver target.
4. **Multi-reference (`num_ref_idx_l0_active > 1`) and B-slice support** — explicitly deferred,
   not designed this ADR; a real follow-up once single-forward-reference P-slice decode is
   hardware-verified, matching the encoder's own staged growth pattern.

## References

- [ADR-0001](0001-vaapi-h264-cpu-out.md) — this crate's IDR-only baseline, the "Support general
  (non-IDR, multi-reference) H.264 decode" deferred-work quote this ADR actions
- `crates/mediaway-decoder/src/linux/vaapi/h264.rs` — current implementation this ADR extends
  (`NalUnitType::NonIdrSlice` rejection: line 335; `num_ref_frames: 0`: line 418; round-robin
  `take_surface_slot`: lines 183-189)
- `crates/mediaway-decoder/src/linux/vaapi/{sps,pps,slice}.rs` — SPS/PPS/slice-header parsers
  this ADR extends
- `crates/mediaway-decoder/src/vulkan/dpb.rs` — DPB porting source (`DpbSlot`, `Dpb`,
  `compute_frame_num_wrap`, `H264_MAX_DPB_SLOTS`)
- `crates/mediaway-decoder/src/vulkan/h264_slice.rs` — reference-list-construction porting
  source (`default_ref_pic_list0`, `apply_ref_pic_list_modifications` — not ported, see
  Alternatives), `dec_ref_pic_marking()`/CABAC/weighted-prediction gap findings
- `crates/mediaway-decoder/src/vulkan/h264_params.rs` — `derive_pic_order_cnt_msb` porting
  source (lines 455-471)
- `crates/mediaway-decoder/src/vulkan/decoder.rs` — per-picture decode ordering porting source
  (`decode_slice_h264`, lines 393-520; DPB sizing comment, lines 306-308)
- [`crates/mediaway-decoder/adr/vulkan/0001-vulkan-video-decode.md`](../vulkan/0001-vulkan-video-decode.md)
  — hardware-verification history (RTX 4090), two real spec-compliance bugs already found/fixed
  in the porting source, "an inference is not a verification" precedent this ADR follows
- `crates/mediaway-encoder/src/vulkan/h264_gop.rs` — encode-side single-forward-reference,
  no-B-frames, no-reordering scope precedent this ADR deliberately mirrors
- `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cros-libva-0.0.13\src\buffer\h264.rs`
  — real vendored `cros-libva` 0.0.13 source read directly for `PictureH264::new`/
  `PictureParameterBufferH264::new` signatures
  (`src/lib.rs:23`'s `pub use bindings::*;` — why `VA_PICTURE_H264_*` constants are not visible
  in the crates.io source tree directly)
- FFmpeg `libavcodec/vaapi_h264.c` (`fill_vaapi_pic`/`fill_vaapi_ReferenceFrames`) — real,
  sourced oracle for `VA_PICTURE_H264_SHORT_TERM_REFERENCE`/`num_ref_frames` conventions,
  fetched this session
- [`docs/ai/wiki/platform/linux-decode.md`](../../../../docs/ai/wiki/platform/linux-decode.md) ·
  [`docs/ai/wiki/platform/vulkan-decode.md`](../../../../docs/ai/wiki/platform/vulkan-decode.md)
- [`docs/spec/sans-io.md`](../../../../docs/spec/sans-io.md) ·
  [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md) ·
  [`docs/conventions/error-handling.md`](../../../../docs/conventions/error-handling.md)

ADRs are **English**. Numbering is local to this `adr/` folder.

## Addendum (2026-08-19): implementation complete, WSL2 compile+test-verified

The implementation pass this ADR called for is done: `linux/vaapi/dpb.rs` (+ sibling
`dpb_tests.rs`) added; `sps.rs`/`pps.rs`/`slice.rs`/`h264.rs` extended exactly per the porting
table and per-picture ordering above. Real WSL2 Ubuntu verification (`libva-dev`, real
`cros-libva` bindgen output, target `x86_64-unknown-linux-gnu`):

- `cargo check -p mediaway-decoder --all-features`: clean.
- `cargo clippy -p mediaway-decoder --all-features --all-targets -- -D warnings`: clean after two
  fixes (see below).
- `cargo test -p mediaway-decoder --all-features` (full crate, not just `--lib`): **168 passed, 0
  failed** (67 of them under `linux::vaapi::*`, including the new `dpb`/P-slice cases; the
  hardware-gated `linux::tests::open_vaapi_h264_cpu_or_skip` integration test soft-skips as
  expected — no real VA-API device in this workspace).
- Windows: `cargo check --workspace --all-features`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, `cargo fmt --check` all clean — the module compiles as a
  `cfg(target_os = "linux")` no-op stub as expected.

No gap was found in this ADR's `cros-libva` 0.0.13 signature assumptions —
`PictureH264::new`/`PictureParameterBufferH264::new`/`SliceParameterBufferH264::new` and
`VA_PICTURE_H264_SHORT_TERM_REFERENCE = 8u32` matched exactly as documented above; no
`cros-libva`-side surprises this pass.

Two clippy fixes needed beyond what this ADR anticipated, both mechanical, no design impact:

- `Dpb::capacity()` (ported verbatim from `vulkan/dpb.rs`) is only ever called from
  `dpb_tests.rs` — flagged `dead_code` under a plain `cargo clippy --all-targets` run when the
  "lib" target's non-test compilation unit is checked separately from the "test" unit. Added an
  explicit `#[allow(dead_code, reason = "...")]`, same disposition this workspace already gives
  other test-only-reachable helpers.
- `derive_pic_order_cnt_msb`'s `u32 as i32` casts needed the same `#[allow(clippy::cast_possible_wrap,
  reason = "...")]` `compute_frame_num_wrap` already carries in this file — the porting source
  (`vulkan/h264_params.rs`) gets this for free from a file-level `#![allow(...)]` this ADR's new
  `dpb.rs` does not inherit, so the same allow had to be added locally, function-scoped.

One cosmetic, zero-behavior naming deviation from this ADR's own step-8 prose: the DPB slot
constructor is named `DpbSlot::new_reference` (matching the porting source, `vulkan/dpb.rs`'s own
name) rather than the `DpbSlot::new` name used in this ADR's abbreviated per-picture-ordering
example — the porting table's "verbatim" instruction for the `DpbSlot` struct/constructor took
precedence over the shorthand name used in the prose walkthrough.

Still true, unchanged by this pass: **zero real VA-API hardware verification** — every new
reference-list/DPB code path is exactly as unverified against a real driver as ADR-0001's
IDR-only path was (see § Zero real-hardware verification remains the honest baseline above).
