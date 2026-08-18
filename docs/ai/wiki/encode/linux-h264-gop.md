# Linux (VA-API) H.264 P-frame GOP

- Module: `mediaway-encoder::linux::vaapi` (`gop.rs`, `video.rs`)
- ADR: [linux/0002](../../../../crates/mediaway-encoder/adr/linux/0002-vaapi-h264-p-frame-gop.md)
  — **Implemented**, see its implementation addendum for real deviations found during coding
- Config: `VideoEncoderConfig::gop_size` (`1` = IDR-only, default, byte-identical for every
  existing caller) now read by this backend — `> 1` requests real P-frame GOP, capability-gated
  on `VAConfigAttribEncMaxRefFrames` (queried once at `open_cpu` time); `0` rejected
  (`EncodeError::InvalidInput`); an unsupporting driver silently falls back to IDR-only
- Rate control (`VideoEncoderConfig::rate_control`) stays unread — CQP-only, unchanged, matching
  Vulkan's own HEVC/AV1 disposition
- Compile/clippy/test-verified on real Linux (WSL2 Ubuntu, real `libva-dev`) — **zero real
  VA-API hardware verification** (no working VA-API device in this workspace)

## Porting plan

`GopState`/`Dpb`/`DpbSlot`/`FrameDecision`/`FrameRequest`/`LOG2_MAX_FRAME_NUM_MINUS4`/
`WORKSPACE_DPB_CAP` port **verbatim** from
[`mediaway-encoder::vulkan::h264_gop`](vulkan-h264-gop.md) — that file is already
GPU-API-agnostic (no `vk::*` types), so this is a stronger porting case than
`mediaway-decoder`'s sibling DPB port (which had to drop Zero-Copy handle bookkeeping). No new
fields needed: this backend's single-forward-reference design only ever needs
`decision.reference`'s one `(usize, DpbSlot)` pair, unlike the decoder sibling's
full-occupied-slot enumeration.

`WORKSPACE_DPB_CAP` (`4`) happens to equal this crate's pre-existing `SURFACE_POOL_SIZE` — no
physical surface-pool resize needed, just a new selection strategy (`GopState::decide`'s
`setup_slot` replaces the old `next_surface` round-robin cursor).

## VA-API-specific plumbing (new, not ported)

- `EncPictureParameterBufferH264`/`EncSliceParameterBufferH264`/`EncSequenceParameterBufferH264`
  signatures confirmed unchanged against real vendored `cros-libva` 0.0.13 source — only the
  **values** passed change (`frame_num`, `idr_pic_flag`/`reference_pic_flag`, `slice_type`,
  `idr_pic_id`, one real `PictureH264` reference entry).
- `EncSequenceParameter` sent **only on IDR frames** once GOP mode is active (a deliberate,
  coupled change — sending fresh SPS/PPS ahead of every P-frame would defeat the point of moving
  off all-IDR encode).
- New capability gate: `Display::get_config_attributes` (`vaGetConfigAttributes`) queries
  `VAConfigAttribEncMaxRefFrames` before honoring `gop_size > 1` — this backend's first
  "probe first, never assume" gate, mirroring Vulkan's `Capabilities::supports_p_frames`.
  `VAConfigAttribEncMaxRefFrames = 13` / `VA_ATTRIB_NOT_SUPPORTED = 0x8000_0000` confirmed against
  real WSL2 bindgen output; the attribute's internal packed-value bit layout was not, but the
  gate only needs the not-supported sentinel + a non-zero value check.
- `effective_gop_size: u32` (a field beyond the ADR's own struct sketch, see its implementation
  addendum) tracks whether the whole *session* is in GOP mode — needed because a single
  `FrameDecision`'s `is_idr` alone can't distinguish "the only frame of an all-IDR session" from
  "the periodic IDR of an active GOP," which the SPS's `intra_period` family and
  `reference_pic_flag` both need to know.
- Surface pool doubles as the DPB: unlike Vulkan Video's explicit DPB image, VA-API's own
  surface (used as `CurrPic` for one frame's encode) implicitly holds the reconstructed picture
  afterward — no separate DPB array needed.

## Real gap found: lost-reference-surface landmine (no Vulkan-side precedent)

A failed `Picture::begin`/`render`/`end` step already unrecoverably loses a surface
(`video.rs`'s own existing doc comment) — harmless under all-IDR (no cross-frame dependency), but
GOP mode means a later frame's reference-list build could point at a slot whose surface is gone.
Guard: check `self.surfaces[ref_slot].is_some()` before trusting `decision.reference`; missing →
hard `Err(EncodeError::Backend)` for that `push_frame` call, no silent downgrade-to-IDR (would
desync `GopState`'s already-mutated internal bookkeeping from the physical session).

## Cross-check against `mediaway-decoder::linux::vaapi` (same-session sibling)

Real, pre-existing, **deliberately unresolved** interop gap: this encoder emits
`pic_order_cnt_type = 2` (implicit POC, zero signaling — ADR-0001's original choice, unchanged).
[`mediaway-decoder`'s sibling ADR-0002](../platform/linux-decode.md) only accepts
`pic_order_cnt_type == 0`. Neither side is spec-wrong; this workspace's own VA-API decoder simply
cannot decode this workspace's own VA-API encoder's P-frame output. System `ffmpeg`/`ffprobe`
(the oracle) decodes either type fine, so the test plan is unaffected. `FrameNumWrap` arithmetic
differs too (decoder implements the general case; encoder sidesteps wraparound entirely via
`log2_max_frame_num_minus4 = 12`) but produces no real disagreement in practice.

## Not covered

Rate control, B-frames (permanent), multi-reference, reference-list reordering,
`intra_refresh_period`, Zero-Copy — all unchanged from ADR-0001 or explicitly out of this ADR's
scope. Real-hardware verification is still zero for this crate.

## Test coverage

`gop.rs`'s `GopState` has a full sans-io unit tier (`gop_tests.rs`, no VA-API device needed):
`gop_size=1` all-IDR-forever, `gop_size=3` I-P-P-I-P-P-I cadence over 7 calls, `frame_num`
increment/wrap at `1 << 16`, `idr_pic_id` increments once per IDR, `reference` is `None`/`Some`
correctly. `video_tests.rs` adds two new hardware-gated tests (both soft-skip without a real VA-API
device): a 7-frame `gop_size=3` cadence check via `Packet::is_keyframe`, and a lost-reference-
surface guard test that manually poisons a DPB slot's surface to confirm `push_frame` returns
`Err(EncodeError::Backend)` rather than panicking or misencoding.
