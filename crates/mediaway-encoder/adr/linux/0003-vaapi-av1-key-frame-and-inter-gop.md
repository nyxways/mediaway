# ADR-0003: VA-API AV1 encode — `KEY_FRAME`-only baseline + single-forward-reference `INTER_FRAME` GOP (ports from `vulkan/av1_params.rs`/`av1_gop.rs`, `windows/d3d12_video_encode/bitstream_av1.rs`)

- **Status**: Proposed — design complete, but genuinely **blocked** on a `cros-libva` gap this
  ADR cannot resolve by itself (see § Blocking dependency). Not "Accepted" like this folder's
  ADR-0001/0002, which had no external blocker of this kind.
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`src/linux/vaapi/`)

## Note on this ADR's design brief

The task that produced this ADR asserted this crate already has a landed "VA-API HEVC" ADR
(`adr/linux/0003-vaapi-hevc-p-frame-gop.md`) to reuse as a porting-methodology precedent, and a
sibling `mediaway-decoder` Vulkan AV1 decode implementation (`vulkan/av1_params.rs`,
`av1_params/av1_frame_header.rs`, `av1_refs.rs`, `adr/vulkan/0002-av1-decode-keyframe-first.md`)
to reuse for OBU parsing. **Both are re-checked against this repository and do not exist.** What
actually exists, confirmed directly:

- `crates/mediaway-encoder/adr/linux/` has exactly two ADRs: `0001` (H.264 CPU-upload,
  all-IDR) and `0002` (H.264 single-forward-reference P-frame GOP, dated the same day as this
  one). **There is no VA-API HEVC ADR anywhere in this workspace** — HEVC exists only on the
  Vulkan backend (`mediaway-decoder::vulkan::decoder_hevc`, `mediaway-encoder::vulkan::hevc_gop`/
  `hevc_params`). This ADR is therefore `0003` (the next real available number), not `0004`, and
  its porting-methodology precedent is `0002` (H.264 P-frame GOP), not a nonexistent HEVC ADR.
- `mediaway-decoder::vulkan` has no `av1_params.rs`/`av1_refs.rs`/AV1 anything, and
  `mediaway-decoder::adr::vulkan` has only `0001-vulkan-video-decode.md`. `docs/roadmap.md` §2
  states this explicitly and currently: "**AV1 decode has not been started**" (any backend,
  workspace-wide). This ADR's decode-side sibling (`mediaway-decoder/adr/linux/0003`) therefore
  cannot port an OBU parser from a Vulkan AV1 decoder — none exists — and says so.
- What **does** exist and **is** real, useful porting material (confirmed by reading the files
  directly, this session): `mediaway-encoder::vulkan::av1_params`/`av1_gop` (AV1 encode,
  `KEY_FRAME` base + single-forward-reference `INTER_FRAME` GOP, structurally hardware-verified
  API-call-sequence-wise but known to emit invalid OBU bytes on this workspace's reference RTX
  4090 — a confirmed driver-maturity limitation per `docs/roadmap.md` §2 / this crate's own
  `adr/vulkan/0001`+`0002` AV1 addenda, not a Mediaway bug), and
  `mediaway-encoder::windows::d3d12_video_encode::bitstream_av1` (a **real, spec-cited, sans-io,
  `forbid(unsafe_code)`** AV1 `sequence_header_obu()`/`uncompressed_header()` byte-level bit
  writer, `KEY_FRAME`-only, single-tile). Both are cited extensively below.

This note exists so a reader comparing this ADR against the original task brief does not conclude
a citation was invented — every citation below was independently re-verified by reading the named
file this session, not copied from the brief.

## Context

`mediaway-encoder::linux::vaapi` (this crate) encodes H.264 only today (ADR-0001 baseline,
ADR-0002 GOP). `mediaway_common::CodecKind::Av1` already exists workspace-wide (`mediaway-common`
crate, used by `mediaway-encoder::vulkan`/`::windows::d3d12_video_encode`'s own AV1 backends) —
this crate's `linux/vaapi/codec.rs::video_profile`/`is_supported_video_codec` simply have no `Av1`
arm yet (`codec.rs:12-27`, current file: `match codec { CodecKind::H264 => ..., _ =>
Err(EncodeError::Unsupported) }`).

This ADR designs adding `CodecKind::Av1` support to this crate: a `KEY_FRAME`-only baseline
(mirroring ADR-0001's H.264 "every frame independent" starting point) plus, in the same pass,
single-forward-reference `INTER_FRAME` GOP structure (mirroring ADR-0002's H.264 GOP extension) —
**both in one ADR**, unlike the H.264 pair's two separate ADRs, because a real, already-designed,
same-shape precedent for *both* pieces already exists together in this crate's own
`vulkan::av1_params`/`av1_gop` modules (added in one pass there too, per those modules' own doc
comments: "ADR-0002's AV1 follow-up adds real single-forward-reference `INTER_FRAME`
construction... alongside this module's original `KEY_FRAME`-only path", `vulkan/av1_params.rs`
module doc lines 18-28). Splitting this ADR into two would not mirror any real precedent this
workspace already has for AV1 specifically.

### Why AV1 is a structurally different porting problem than H.264/HEVC on this backend

H.264/HEVC's VA-API encode buffers (`EncSequenceParameterBufferH264`/`EncPictureParameterBufferH264`
etc.) are pure **C-struct field bags** — the driver derives the actual SPS/PPS/slice-header NAL
bytes from those fields itself; this crate never constructs H.264/HEVC bitstream bytes by hand.
**AV1 is not that.** Reading `cros-libva` 0.0.13's real vendored `src/buffer/av1.rs` directly
(`C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cros-libva-0.0.13\src\buffer\av1.rs`,
not paraphrased) and FFmpeg's real `libavcodec/vaapi_encode_av1.c` (fetched this session, see
§ VA-API-specific plumbing) together confirm: `VAEncPictureParameterBufferAV1` carries six fields —
`bit_offset_qindex`, `bit_offset_segmentation`, `bit_offset_loopfilter_params`,
`bit_offset_cdef_params`, `size_in_bits_cdef_params`, `byte_offset_frame_hdr_obu_size`,
`size_in_bits_frame_hdr_obu` (`av1.rs:1053-1059`, `EncPictureParameterBufferAV1::new`'s trailing
parameters) — that only make sense if the **application itself writes the real, complete
`frame_header_obu()` bitstream bytes** into a separate packed-header buffer, and tells the driver
exactly which bit offsets within that buffer carry `base_q_idx`/segmentation/loop-filter/CDEF
parameters so the driver's own rate-control logic can patch them in place after encode, without
re-serializing the whole header. FFmpeg's encoder confirms this directly: it calls a real OBU
writer (`vaapi_encode_av1_write_obu`) to fill `priv->fh_data`/`fh_data_len`, then sets exactly
these six fields from that written buffer's own byte/bit positions, and submits the packed
header through the codec-generic VA-API packed-header buffer mechanism (`ff_vaapi_encode_receive_packet`'s
shared packed-header infrastructure — the same one H.264 uses for its own SPS/PPS NAL bytes, just
AV1 has no SPS/PPS split so it is one `VAEncPackedHeaderRawData`-typed buffer per frame instead of
two per-IDR buffers). **This crate must therefore write real AV1 OBU bytes by hand for the first
time** — H.264/HEVC never needed this on VA-API.

### Why this is not "re-derive from spec" work — two real, existing, cited ports close most of the risk

1. **OBU byte-serialization**: `mediaway-encoder::windows::d3d12_video_encode::bitstream_av1`
   (`crates/mediaway-encoder/src/windows/d3d12_video_encode/bitstream_av1.rs`, 315 lines, `forbid
   (unsafe_code)`, zero D3D12 types anywhere in the file) is **already** a real,
   AV1-spec-section-cited (`§5.5` sequence header, `§5.5.2` color config, `§5.9.2`
   `uncompressed_header()`, `§5.9.5` frame/render size, `§5.9.12` quantization, `§5.9.15` tile
   info, `§5.9.11` loop filter, `§5.9.19` CDEF, `§5.9.20` loop restoration) bit-level OBU writer
   for exactly this ADR's target profile: Main, 8-bit 4:2:0, `KEY_FRAME`-only, single tile, no
   CDEF/restoration/segmentation/film-grain. It was written for a **different reason** (D3D12
   native encode needs the app to supply its own SPS/PPS-equivalent bytes because the driver only
   writes the compressed tile payload) but the byte-serialization logic itself
   (`write_leb128`/`obu_header_byte`/`wrap_obu`/`write_tile_info`/`write_sequence_header`/
   `write_frame_header`, `bitstream_av1.rs:40-314`) is 100% AV1-bitstream-spec-level, zero
   D3D12-API-level — the same "port, don't re-derive" case ADR-0002 already made for
   `vulkan::h264_gop`.
2. **Bitstream-field *values*/GOP state machine**: `mediaway-encoder::vulkan::av1_params`/
   `av1_gop` (`crates/mediaway-encoder/src/vulkan/av1_params.rs`,
   `crates/mediaway-encoder/src/vulkan/av1_gop.rs`) already made every non-obvious AV1 encode
   design decision this crate would otherwise have to re-derive from scratch: `enable_order_hint =
   1` with a GOP-selected `order_hint_bits_minus_1` (not `reduced_still_picture_header`, which
   `av1_params.rs`'s own doc records as a **real, hardware-verified-invalid** earlier mistake,
   `av1_params.rs:143-156`), `error_resilient_mode = 1` for both `KEY_FRAME`/`INTER_FRAME`,
   `disable_cdf_update`/`disable_frame_end_update_cdf = 0` (not `1` — also a
   hardware-verified-invalid earlier mistake, `av1_params.rs:419-439`), never-null
   segmentation/loop-filter/CDEF/loop-restoration/global-motion/extension-header structs
   (all-disabled content, but real structs, never omitted), `PRIMARY_REF_NONE` for both frame
   types, single-forward-reference `LAST_FRAME`-only GOP with an 8-frame-wide DPB ring reusing one
   physical slot per AV1 reference-name slot. These are AV1-bitstream-semantics decisions, not
   Vulkan-API decisions — directly informative for this ADR's own field values even though the
   destination struct shape (`cros-libva`'s `EncSequenceParameterBufferAV1`/
   `EncPictureParameterBufferAV1`, a flat C struct) differs from Vulkan's `StdVideoAV1*` structs.

Re-deriving either of these independently from the AV1 spec text would re-risk the exact bug class
`vulkan/av1_params.rs`'s own doc comments already record having hit and fixed once
(`reduced_still_picture_header`, `disable_cdf_update`, null optional-tool pointers — three
separate real, hardware-verified-invalid mistakes on the way to the current, FFmpeg-cross-checked
values).

## Decision

> Add `CodecKind::Av1` to `mediaway-encoder::linux::vaapi`: `KEY_FRAME`-only baseline (default,
> `gop_size <= 1`) plus single-forward-reference `INTER_FRAME` GOP (`gop_size > 1`,
> capability-gated), by (1) porting `vulkan::av1_gop::GopState` verbatim into a new
> `linux/vaapi/av1_gop.rs`; (2) porting `windows::d3d12_video_encode::bitstream_av1`'s OBU
> byte-writer into a new `linux/vaapi/av1_obu.rs`, extended with the bit-position tracking that
> destination requires and D3D12 never needed; (3) using `vulkan::av1_params`'s
> already-hardware-cross-checked field **values** (not its struct shapes) to fill
> `cros-libva::EncSequenceParameterBufferAV1`/`EncPictureParameterBufferAV1`. **This ADR's design
> is complete, but its implementation is blocked** on a real, confirmed gap in `cros-libva`
> 0.0.13's safe API surface — see § Blocking dependency, the reason this ADR's Status is
> "Proposed," not "Accepted."

### Scope

**In (this ADR's design):**

- AV1 Main profile (`seq_profile == 0`), 8-bit 4:2:0 (matches NV12), one operating point, no
  scalability, no film grain, no CDEF, no loop restoration, no segmentation, no superres, no
  screen-content tools, no warped motion — the exact all-disabled-tool subset
  `vulkan::av1_params`/`windows::bitstream_av1` already establish and hardware/spec-cross-check.
- `KEY_FRAME`-only baseline: every pushed frame independent, `refresh_frame_flags = 0xFF`,
  `PRIMARY_REF_NONE`, `error_resilient_mode = 1`. Reproduces `VideoEncoderConfig::gop_size <= 1`'s
  existing cross-backend contract (same disposition ADR-0001/0002 already give H.264).
- Single-forward-reference `INTER_FRAME` GOP (`gop_size > 1`, capability-gated): `LAST_FRAME`-only
  prediction, one active reference, `order_hint`-keyed (not `frame_num`/POC — AV1 has neither),
  ported `GopState` from `vulkan::av1_gop` verbatim.
- Real packed `frame_header_obu()` bytes constructed per frame via the ported/extended OBU writer,
  with real `bit_offset_*`/`size_in_bits_*`/`byte_offset_*` fields describing where within that
  buffer the driver may patch `base_q_idx`/segmentation/loop-filter/CDEF (segmentation/CDEF always
  disabled in this scope, so those offsets point at header regions the driver's patch logic should
  find already-zero/no-op, matching the all-disabled-tool scope above).
- Single tile group, single tile (`tile_cols == tile_rows == 1`), matching
  `windows::bitstream_av1::write_tile_info`'s own always-one-tile scope for every resolution this
  crate's existing `validate()` already accepts (macroblock/superblock-aligned dimensions well
  under VA-API's per-tile size/area limits).

**Out (deferred):**

- Zero-Copy DMA-BUF surface import — unrelated axis, ADR-0001's own deferral, unchanged.
- CDEF, loop restoration, segmentation, film grain, superres, screen-content tools, warped motion,
  multi-reference (`LAST2`/`LAST3`/`GOLDEN`/`BWDREF`/`ALTREF2`/`ALTREF`), B-frames, multi-tile —
  all permanent non-goals for this pass, mirroring `vulkan::av1_gop`'s own identical scope cut
  (module doc: "keeps the same single-forward-reference scope as H.264/HEVC... never
  `LAST2`/`LAST3`/`GOLDEN`/`BWDREF`/`ALTREF2`/`ALTREF`").
- VBR/CBR rate control (`VideoEncoderConfig::rate_control` stays unread by this backend — same
  disposition H.264 VA-API gives it, and Vulkan gives HEVC/AV1). CQP-only (`VA_RC_CQP`), fixed
  `base_q_idx`.
- CDF forward-adaptation across frames (`primary_ref_frame` stays `PRIMARY_REF_NONE` always, even
  for `INTER_FRAME` — matches `vulkan::av1_params::build_inter_frame_picture_info`'s own explicit
  reasoning for not attempting this: "adds real bookkeeping this crate cannot itself verify... for
  no benefit provable on this hardware", a reasoning this ADR's own zero-hardware-verification
  status shares).

## Blocking dependency: `cros-libva` 0.0.13 has no packed-header buffer wrapper

**This is this ADR's single highest-priority finding — a real, confirmed gap, not an inference.**
Reading `cros-libva` 0.0.13's `src/buffer.rs` `BufferType` enum in full
(`buffer.rs:299-322`) confirms its only variants are: `PictureParameter`, `SliceParameter`,
`IQMatrix`, `Probability`, `SliceData(Vec<u8>)`, `EncSequenceParameter`, `EncPictureParameter`,
`EncSliceParameter`, `EncMacroblockParameterBuffer`, `EncCodedBuffer(usize)`,
`EncMiscParameter`. **There is no `EncPackedHeaderParameter`/`EncPackedHeaderData` variant, and no
generic raw-byte "arbitrary `VABufferType`" escape hatch.** Real libva has
`VAEncPackedHeaderParameterBufferType`/`VAEncPackedHeaderDataBufferType` (a two-buffer mechanism:
one small struct describing the packed header's type/bit-length/whether it needs emulation
prevention, one raw-byte buffer holding the actual header bytes) — this is the exact mechanism
FFmpeg's `vaapi_encode_av1.c` uses to submit its hand-written `frame_header_obu()` bytes (see
§ Context). `cros-libva` 0.0.13 simply never wrapped it — H.264/HEVC/VP8/VP9 VA-API encode as this
crate/`cros-libva` already use them do not need packed headers (the driver derives their NAL bytes
from the struct fields alone), so `cros-libva` had no prior reason to add this wrapper.

**Why this crate cannot work around it locally**: `linux/vaapi/mod.rs` declares
`#![forbid(unsafe_code)]` for this entire module (`mod.rs:7`) — this crate's own established
policy (ADR-0001's Decision: "All FFI unsafety lives in `cros-libva`, not this crate"). A raw
`vaCreateBuffer` FFI call bypassing `cros-libva`'s safe `BufferType` enum, written directly in
this crate, would violate that `forbid` and this crate's own stated architecture. The only
options, in order of preference:

1. **Extend `cros-libva` itself** (a small, self-contained addition: two new `BufferType`
   variants wrapping `VAEncPackedHeaderParameterBuffer` + a raw `Vec<u8>` payload, mirroring how
   `SliceData(Vec<u8>)` already wraps a raw byte buffer) — either as an upstream PR to
   `chromeos/cros-libva`, or (if upstream review would block this crate's own timeline) a
   workspace-vendored patched fork pinned the same way `deps-policy.md` already expects for a
   heavy, deliberate dependency change. This keeps every `unsafe` FFI call inside `cros-libva`,
   preserving this crate's `forbid(unsafe_code)` invariant exactly as ADR-0001 designed it.
2. **Do not implement AV1 encode on this backend until (1) lands.** This ADR's design (OBU writer
   port, GOP state port, field-value derivation) stays valid and ready either way — only the final
   "submit the packed header buffer to the driver" step is blocked.

This ADR does **not** pick between "upstream PR" vs. "vendored fork" — that is a `deps-policy.md`-
governed decision needing its own review (license/maintenance/cost checklist), out of this ADR's
own scope. Flagged prominently, not silently assumed away.

### Precise porting plan: `windows::bitstream_av1` → `linux/vaapi/av1_obu.rs`

New file `crates/mediaway-encoder/src/linux/vaapi/av1_obu.rs`, sans-io, `#![forbid(unsafe_code)]`
(consistent with this module's existing crate-wide forbid), zero `cros_libva` types:

| New (`linux/vaapi/av1_obu.rs`) | Ported from (cited source) | Change from source |
|---|---|---|
| `write_leb128` | `bitstream_av1.rs:40-51` | Verbatim |
| `obu_header_byte` | `bitstream_av1.rs:54-56` | Verbatim |
| `wrap_obu` | `bitstream_av1.rs:59-65` | Verbatim |
| `bit_length`/`tile_log2` | `bitstream_av1.rs:67-77` | Verbatim |
| `write_tile_info` | `bitstream_av1.rs:87-128` | Verbatim (this ADR's scope is single-tile-only too, same as the source) |
| `write_sequence_header` | `bitstream_av1.rs:132-192` | **Changed**: `enable_order_hint` becomes `1` (not `0`) with a GOP-selected `order_hint_bits_minus_1`, matching `vulkan::av1_params::build_sequence_header`'s own real, hardware-cross-checked value (the source's `enable_order_hint = 0` was a valid choice *for D3D12's all-KEY_FRAME-only scope*, but this ADR's GOP mode needs `order_hint` signaling) — every other field (profile, tile/CDEF/restoration/superres all disabled, color config) stays verbatim |
| `write_frame_header` | `bitstream_av1.rs:204-282` | **Changed**: gains an `is_key: bool`/`order_hint: u8`/`ref_frame_idx: Option<u8>` parameter set (the source hardcodes `frame_type == KEY_FRAME`, `OrderHintBits == 0` so no `order_hint` field is ever read/written) — `INTER_FRAME` needs `frame_type` (`f(2)`), `order_hint` (`f(OrderHintBits)`, only when `enable_order_hint`), `refresh_frame_flags` becomes an explicit `f(8)` (no longer spec-inferred `allFrames`, since `INTER_FRAME` frames only refresh their own setup slot), and a `ref_frame_idx[LAST_FRAME]` `f(3)` read/write chain per AV1 spec §5.9.2's `frame_reference_mode()`/reference-selection block that the source's all-`KEY_FRAME` scope never reaches. Bit-offset **capture** (see below) is also new. |
| *(none in source — new)* | — | **New**: `bit_position(&self) -> u32` method on the ported writer struct (a straightforward `bytes.len() * 8 + bit_count`, `RbspWriter`'s private fields already hold both halves, `bitstream.rs:23-27`) — the source has no equivalent because D3D12's own bitstream API needs no bit-offset patching contract; this destination's `cros-libva::EncPictureParameterBufferAV1` does (see § Blocking dependency). Every `write_*` call site inside `write_frame_header` that corresponds to one of the six `bit_offset_*`/`size_in_bits_*`/`byte_offset_*` fields records `bit_position()` immediately before/after that field write. |

`RbspWriter`'s underlying primitives (`write_bit`/`write_bits`/`write_u8`/`byte_align_zero`,
`bitstream.rs:38-107`) are **also** ported verbatim into this new file (not imported —
`d3d12_video_encode/bitstream.rs`'s writer is `pub(super)`-scoped to that platform module, and
porting rather than cross-module-importing across platform backends is this crate's own
established precedent, ADR-0002's own Alternatives table). `write_ue`/`write_se`/
`rbsp_trailing_bits` (H.264/HEVC Exp-Golomb helpers, `bitstream.rs:69-93`) are **not** ported — AV1
uses only `f(n)`/`leb128`/`byte_align_zero`, confirmed by the source's own module doc
("`f(n)` only for this backend's AV1 scope... `ue(v)`/`se(v)` Exp-Golomb are H.264/HEVC-only...
unused by the AV1 writer", `bitstream.rs:16-22`).

### Precise porting plan: `vulkan::av1_gop::GopState` → `linux/vaapi/av1_gop.rs`

New file `crates/mediaway-encoder/src/linux/vaapi/av1_gop.rs`, sans-io, zero `cros_libva` types
— same porting shape ADR-0002 already used for `linux/vaapi/gop.rs` ↔ `vulkan::h264_gop`:

| New (`linux/vaapi/av1_gop.rs`) | Ported from (cited source) | Change from source |
|---|---|---|
| `ORDER_HINT_BITS_MINUS_1_GOP: u8 = 7` | `vulkan/av1_gop.rs:57` | Verbatim |
| `DpbSlot { order_hint, is_key }` | `vulkan/av1_gop.rs:63-67` | Verbatim |
| `Dpb { slots, next_slot }` + `Default` | `vulkan/av1_gop.rs:70-83` | Verbatim — this crate's own `WORKSPACE_DPB_CAP` (`super::gop::WORKSPACE_DPB_CAP`, already `= 4`, aliased from the H.264 GOP port) is reused directly rather than re-declaring a duplicate constant, since both this crate's H.264 and AV1 GOP ports already use the same physical VA-API surface pool size |
| `FrameRequest { Auto, ForceKey }` | `vulkan/av1_gop.rs:95-98` | Verbatim (`ForceKey` stays an unwired hook, same disposition the H.264 port already gives its own `ForceIdr`) |
| `FrameDecision { is_key, order_hint, setup_slot, reference }` | `vulkan/av1_gop.rs:104-109` | Verbatim |
| `GopState { gop_size, frames_since_key, order_hint, dpb, last_written }` | `vulkan/av1_gop.rs:117-123` | Verbatim |
| `GopState::new`/`decide` | `vulkan/av1_gop.rs:125-192` | Verbatim — this function is already zero-Vulkan-dependency pure Rust, confirmed by reading it in full |

### VA-API-specific plumbing (distinct from the ported OBU writer / GOP state above)

**Confirmed by reading `cros-libva` 0.0.13's real vendored source directly**
(`.../cros-libva-0.0.13/src/buffer/av1.rs`, line numbers below refer to this file):

- `EncSequenceParameterBufferAV1::new(seq_profile, seq_level_idx, seq_tier, hierarchical_flag,
  intra_period, ip_period, bits_per_second, seq_fields: &AV1EncSeqFields,
  order_hint_bits_minus_1)` (`av1.rs:641-673`) — one call per session (AV1 has no
  per-IDR-repeated SPS the way H.264 does; this crate sends it once at `open_cpu` time, or — to
  mirror this crate's own H.264 "SPS only on IDR" convention exactly — once per `KEY_FRAME`,
  matching `EncSequenceParameter` semantics being logically "this session's/this GOP's sequence
  header," never a per-picture buffer). `AV1EncSeqFields::new` (`av1.rs:566-636`) takes 18
  `bool`/`u32` parameters directly (not a bitfield this crate hand-packs) — `enable_order_hint`
  is parameter 9 of 18, set `true` whenever `effective_gop_size > 1` (mirrors this crate's own
  H.264 `gop_active` gate), matching `vulkan::av1_params::build_sequence_header`'s identical
  condition.
- `EncPictureParameterBufferAV1::new(...)` (`av1.rs:1006-1140`) — the large struct this ADR's own
  § Context already dissected for its `bit_offset_*`/`byte_offset_*`/`size_in_bits_*` trailing
  parameters (`av1.rs:1053-1059`). `reconstructed_frame: VASurfaceID` is this frame's own setup
  slot's surface (mirrors this crate's H.264 `curr_pic`); `reference_frames: [VASurfaceID; 8]` /
  `ref_frame_idx: [u8; 7]` are **raw surface IDs**, not a `PictureAV1`-with-flags wrapper type
  like H.264's `PictureH264` — AV1 decode/encode buffers carry no per-reference flags bitmask at
  all in real libva (confirmed absent from this file; every reference is addressed purely by
  DPB-slot-index + surface ID). `hierarchical_level_plus1: u8` is `#[cfg(libva_1_19_or_higher)]`
  (`av1.rs:1082`) — this ADR sets it `1` (no hierarchy, matching `vulkan::av1_gop`'s own flat,
  non-hierarchical single-forward-reference structure) whenever that cfg is active; this crate
  cannot itself confirm which `libva` version this workspace's WSL2/target build resolves to
  without a real build (flagged in § Open questions).
- `EncTileGroupBufferAV1::new(tg_start: u8, tg_end: u8)` (`av1.rs:1145-1156`) — submitted via
  `BufferType::EncSliceParameter(EncSliceParameter::AV1(...))` (`buffer.rs:205-208`, `441-442` —
  real libva's `VAEncSliceParameterBufferType` is reused generically for AV1's tile-group
  metadata too, not a separate buffer type; confirmed by `buffer.rs`'s own `EncSliceParameter`
  enum). Single tile group covering the whole frame: `tg_start = tg_end = 0`, matching this
  crate's existing single-tile scope.
- Profile: `VAProfileAV1Profile0` (Main profile) — this exact identifier is a real, stable,
  long-standing libva enum name (used identically by FFmpeg's own AV1 profile table, confirmed
  this session), but its concrete numeric value comes from build-time `bindgen` output
  (`cros-libva`'s `bindings.rs` is generated at build time from system headers,
  `buffer.rs:5`/`lib.rs:23`'s `pub use bindings::*;` — not checked into the crates.io source
  tree, same disposition ADR-0002 already documented for `VAConfigAttribEncMaxRefFrames`), so its
  exact presence/value in this workspace's real WSL2 bindgen output is unconfirmed this session
  (§ Open questions).
- Entrypoint: **unconfirmed, and this ADR's second-highest risk after § Blocking dependency.**
  FFmpeg's own `vaapi_encode_av1.c` does not show its entrypoint selection directly (delegated to
  shared `ff_vaapi_encode_init` infrastructure not fetched this session) but real-world AV1
  VA-API encode driver support today is dominated by `VAEntrypointEncSliceLP` ("low power" —
  most Intel/AMD AV1 hardware encode blocks only expose this entrypoint, not the classic
  `VAEntrypointEncSlice` this crate's H.264 path already uses). This ADR designs `open_cpu` to
  **probe both**, `VAEntrypointEncSlice` first then `VAEntrypointEncSliceLP` (via
  `Display::query_config_entrypoints(profile)`, the same query-first-never-assume style
  ADR-0002 already established for `VAConfigAttribEncMaxRefFrames`), rather than hardcoding
  either — not independently driver-confirmed this session.

### `VaapiAv1Encoder` struct shape (ZCA sketch — ownership, no `Box`/`dyn`)

```rust
// linux/vaapi/av1_obu.rs — new file, sans-io, no cros_libva types. Ported bit-writer
// (see porting table above) plus per-field bit-offset capture.
pub(super) struct Av1ObuWriter { /* ported RbspWriter fields, + bit_position() */ }
pub(super) struct Av1FrameHeaderBits {
    pub(super) bytes: Vec<u8>,
    pub(super) bit_offset_qindex: u32,
    pub(super) bit_offset_segmentation: u32,
    pub(super) bit_offset_loopfilter_params: u32,
    pub(super) bit_offset_cdef_params: u32,
    pub(super) size_in_bits_cdef_params: u32,
    pub(super) byte_offset_frame_hdr_obu_size: u32,
    pub(super) size_in_bits_frame_hdr_obu: u32,
}
pub(super) fn build_av1_session_prefix(width: u32, height: u32, gop_active: bool) -> Vec<u8> { .. }
pub(super) fn build_av1_frame_header(
    is_key: bool, order_hint: u8, ref_slot: Option<u8>, base_q_idx: u8, width: u32, height: u32,
) -> Av1FrameHeaderBits { .. }

// linux/vaapi/av1_gop.rs — new file, sans-io. Verbatim port of vulkan::av1_gop's types.
pub(super) const ORDER_HINT_BITS_MINUS_1_GOP: u8 = 7;
#[derive(Debug, Clone, Copy)]
pub(super) struct DpbSlot { pub(super) order_hint: u8, pub(super) is_key: bool }
#[derive(Debug, Clone, Copy)]
pub(super) struct FrameDecision {
    pub(super) is_key: bool, pub(super) order_hint: u8, pub(super) setup_slot: usize,
    pub(super) reference: Option<(usize, DpbSlot)>,
}
#[derive(Debug)]
pub(super) struct GopState { /* verbatim fields */ }
impl GopState { pub(super) fn new(gop_size: u32) -> Self { .. } pub(super) fn decide(&mut self, request: FrameRequest) -> FrameDecision { .. } }

// linux/vaapi/av1.rs — new file, sibling of video.rs (H.264), same shape.
pub(crate) struct VaapiAv1Encoder {
    context: Rc<Context>,
    _config: Config,
    info: StreamInfo,
    width: u32,
    height: u32,
    entrypoint: cros_libva::VAEntrypoint::Type, // EncSlice or EncSliceLP, resolved at open_cpu
    nv12_bytes: usize,
    surfaces: Vec<Option<Surface<()>>>,
    gop: GopState,
    effective_gop_size: u32,
    pending: VecDeque<Packet>,
    flushed: bool,
}
```

No `Box<dyn _>`/`dyn Trait` anywhere — mirrors every other backend in this workspace. `codec.rs`
grows a `video_profile`/`is_supported_video_codec` `CodecKind::Av1` arm returning
`VAProfileAV1Profile0`, following the existing `match` shape exactly (`codec.rs:12-27`).
`VaapiVideoEncoder` (H.264, `video.rs`) is **not** widened into a multi-codec enum dispatcher this
pass — a new sibling type (`VaapiAv1Encoder`) plus a small dispatcher in whatever public
`VideoEncoder::open` entry point selects between them by `config.codec`, mirroring how
`mediaway-encoder::vulkan` already keeps `h264_gop`/`hevc_gop`/`av1_gop` as separate, non-shared
types rather than one generic GOP state machine (`av1_gop.rs`'s own module doc: "a separate type
from both siblings... same reasoning `hevc_gop`'s module doc already gives for not sharing with
`h264_gop`").

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Re-derive AV1 `uncompressed_header()`/`sequence_header_obu()` bit-writing from the AV1 spec text independently | Rejected — `windows::bitstream_av1` is a real, already-spec-section-cited, already-working (D3D12-verified byte layout for the identical `KEY_FRAME`/single-tile/all-disabled-tools case) source; re-deriving independently re-risks the same bug classes `vulkan::av1_params`'s own doc history already shows this workspace hitting three times for the *field-value* side of this exact codec. |
| Wait for a hypothetical future Vulkan AV1 decode port instead of using the D3D12 encoder as the porting source | Rejected — no such port exists (see § Note on this ADR's design brief); the D3D12 encoder is a real, cited, in-repo AV1 bit-writer available today, and — despite being a writer, not a parser — its conditional-field-presence chain (`§5.9.2`'s inference rules: which fields are read/written under which flag combinations) is exactly the same logic this ADR's own writer needs, regardless of direction. |
| Hand-roll a raw `vaCreateBuffer` FFI call in this crate for the missing packed-header buffer type, bypassing `cros-libva`'s safe wrapper | Rejected — violates `linux/vaapi/mod.rs`'s own `#![forbid(unsafe_code)]` and this crate's ADR-0001-established architecture ("all FFI unsafety lives in `cros-libva`"). See § Blocking dependency for the only two options this ADR considers legitimate. |
| Ship `KEY_FRAME`-only without the `INTER_FRAME` GOP extension, as a smaller first increment (mirroring H.264's two-ADR split) | Considered, rejected — unlike H.264, a real, already-designed, same-crate-family precedent for *both* pieces together already exists in `vulkan::av1_gop`/`av1_params` (added in one pass there). Splitting this ADR would not track any real precedent and would defer real, already-cited-and-ready design work for no stated benefit — the packed-header blocker (§ Blocking dependency) affects both scopes identically, so narrowing scope does not unblock implementation any sooner. |
| Target only `VAEntrypointEncSlice` (skip the `VAEntrypointEncSliceLP` probe) | Rejected — real-world AV1 VA-API encode driver support is dominated by the LP entrypoint; hardcoding the non-LP entrypoint this crate's H.264 path already uses would likely make this backend fail to find *any* usable AV1 encode config on real hardware, defeating the ADR's own purpose. Probing both, in order, costs one extra `query_config_entrypoints` call and matches this crate's own "probe first, never assume" precedent (ADR-0002's `VAConfigAttribEncMaxRefFrames` gate). |

## Consequences

### Positive

- A real, cited port closes almost all of this ADR's bitstream-correctness risk before
  implementation even starts — the same risk-reduction argument ADR-0002 already made for H.264
  GOP, now extended to a codec with meaningfully higher intrinsic complexity (tiles, OBUs,
  packed headers).
- Found and named a real, concrete, actionable blocking dependency (`cros-libva`'s missing packed-
  header wrapper) before any implementation time was spent discovering it the hard way — this is
  the single most valuable output of this ADR.
- `gop_size <= 1` design mirrors this crate's own established default-path-byte-identical
  discipline (once unblocked, the base path should be a faithful `KEY_FRAME`-only implementation
  with zero surprise behavior change for existing callers, none of whom currently request AV1 on
  this backend since it does not exist yet).

### Negative / Trade-offs

- **Genuinely not implementable against `cros-libva` 0.0.13 as pinned today** — this is a real
  external blocker, not a soft risk; this ADR cannot itself resolve it (see § Blocking
  dependency). Status is "Proposed," not "Accepted," for exactly this reason.
- Zero real-hardware verification for this crate's AV1 path, compounded by this workspace's own
  existing finding that AV1 VA-API/Vulkan/D3D12 encode driver maturity is broadly weak right now
  (`docs/roadmap.md`'s Vulkan AV1 entry: "structurally hardware-verified but every frame's OBU
  output is invalid — confirmed driver-maturity limitation"). Even a fully unblocked, fully
  implemented version of this ADR may hit the same class of driver-side AV1 encode immaturity
  Vulkan already did, on different hardware/drivers, discoverable only by real testing this
  session cannot perform.
- The `bit_offset_*`/`size_in_bits_*` bit-position-capture addition to the ported OBU writer is
  new code with no precedent in either porting source (D3D12 never needed it; `vulkan::av1_params`
  passes whole native structs, no manual bit tracking) — a genuinely new, if small and mechanical,
  correctness-critical surface this ADR's ports do not fully de-risk.
- `VAEntrypointEncSliceLP` vs `VAEntrypointEncSlice` selection is inferred from general industry
  knowledge, not fetched/confirmed against a specific driver or FFmpeg source line this session —
  flagged in § Open questions.

## Test plan (for the implementation pass that follows this ADR, and only after § Blocking dependency is resolved)

- **Sans-io, hardware-independent (highest-value, run first)**: `linux/vaapi/av1_obu_tests.rs` —
  hand-verify `write_leb128`/`obu_header_byte`/`wrap_obu` against known-good byte sequences
  (mirrors `bitstream_av1.rs`'s own D3D12-side correctness, now cross-checked once more on a
  second, independent port); `write_sequence_header`/`write_frame_header` byte output for a small
  fixed resolution, hand-computed bit-for-bit against the AV1 spec sections already cited in the
  source; `bit_position()` returns the exact expected value at each of the six capture points for
  a `KEY_FRAME` and an `INTER_FRAME` case.
- **Sans-io GOP**: `linux/vaapi/av1_gop_tests.rs` — port `vulkan`'s own AV1 GOP test coverage
  (`encoder_tests.rs::push_seven_av1_frames_gop_or_skip`'s cadence assertions, adapted to a
  sans-io unit-test shape the way ADR-0002 already gave H.264's GOP port a unit tier the Vulkan
  side never had).
- **`av1.rs` integration** (hardware-gated, `_or_skip_without_hw`-style, expected to skip in this
  session/CI): `KEY_FRAME`-only push, then a `gop_size = 3` GOP push sequence.
- **Oracle validation**: pipe an encoded AV1 stream through system `ffprobe`/`ffmpeg -i` (this
  workspace's standing oracle, [ADR-0002](../../../../docs/adr/0002-system-oracle.md)) — the
  single most valuable test this ADR can specify, since it validates the hand-written OBU bytes
  against a real, independent AV1 parser, not just this workspace's own logic.
- **WSL2 real-Linux compile verification**: confirms `VAProfileAV1Profile0`'s real name/value,
  `VAEntrypointEncSliceLP`'s real name, `EncPictureParameterBufferAV1`'s
  `#[cfg(libva_1_19_or_higher)]` `hierarchical_level_plus1` gate resolution, and (once § Blocking
  dependency is resolved) the new packed-header `BufferType` variant's real signature — every item
  in § Open questions.
- Default `cargo test --workspace` (no system FFmpeg, no VA-API hardware) must keep passing —
  the OBU-writer and GOP-state sans-io tests above require neither.

## Addendum (2026-08-19, packed-header blocker independently confirmed via real WSL2 source read)

This ADR's own blocking finding is now independently re-confirmed, not just cited. Read directly
(not `grep`-inferred) from `cros-libva-0.0.13/src/buffer.rs:299-322`:

```rust
pub enum BufferType {
    PictureParameter(PictureParameter),
    SliceParameter(SliceParameter),
    IQMatrix(IQMatrix),
    Probability(vp8::ProbabilityDataBufferVP8),
    SliceData(Vec<u8>),
    EncSequenceParameter(EncSequenceParameter),
    EncPictureParameter(EncPictureParameter),
    EncSliceParameter(EncSliceParameter),
    EncMacroblockParameterBuffer(EncMacroblockParameterBuffer),
    EncCodedBuffer(usize),
    EncMiscParameter(EncMiscParameter),
}
```

Exactly 11 variants (10 buffer kinds + doc comment miscounted as 10 in this ADR's own body —
immaterial). A crate-wide `grep -rn "PackedHeader\|packed_header\|VAEncPackedHeader"` across
every `.rs` file in `cros-libva-0.0.13/src/` returns **zero matches** — confirming there is no
partial/hidden packed-header support anywhere in this crate, not just missing from the
`BufferType` enum specifically. `EncSequenceParameter::AV1`/`EncPictureParameter::AV1`/
`EncSliceParameter::AV1(av1::EncTileGroupBufferAV1)` **do** exist (confirmed present, real AV1
seq/pic/tile-group buffer support) — the gap is narrow and specific: only the packed
`frame_header_obu()` submission mechanism VA-API's own AV1 encode design requires is missing,
not AV1 encode support in general. This ADR's Status stays **Proposed — blocked**; this session
does not implement it. A future increment would need either a `cros-libva` fork/PR upstream
adding `PackedHeader`/`PackedHeaderData` variants, or (out of scope for this crate, which
`#![forbid(unsafe_code)]`s) a raw FFI escape hatch.

## Open questions / risks (explicit, for whoever picks up the implementation pass)

1. **§ Blocking dependency itself** — the top-priority, must-resolve-first item. No implementation
   can proceed until `cros-libva` gains a packed-header buffer wrapper (fork or upstream).
2. **`VAProfileAV1Profile0`'s real bindgen name/value** — inferred from general libva/FFmpeg
   knowledge, not confirmed against this workspace's real WSL2 bindgen output this session (no
   shell/Bash tool available in this pass — see `mediaway-decoder`'s sibling ADR for the same
   disposition on its own unconfirmed constants).
3. **`VAEntrypointEncSliceLP` vs `VAEntrypointEncSlice` real-world driver support** — inferred from
   general industry knowledge (Intel/AMD AV1 hardware encode block conventions), not fetched from
   a specific FFmpeg source line or driver doc this session.
4. **`EncPictureParameterBufferAV1`'s `#[cfg(libva_1_19_or_higher)]`-gated `hierarchical_level_plus1`
   field** (`av1.rs:1082`) — this workspace's real target `libva` version (WSL2's `libva-dev`
   2.20.0, per ADR-0001's own confirmation) is almost certainly `>= 1.19`, but this was not
   re-verified against `cros-libva`'s own `libva_*_or_higher` cfg-detection build-script logic
   this session.
5. **Whether real AV1 VA-API encode drivers actually honor/require the `bit_offset_*` patch
   contract as documented, or silently ignore it under `VA_RC_CQP`** (fixed-QP, no rate-control
   adaptation) — this ADR provides real, correct offsets defensively (matching FFmpeg's own
   unconditional practice) but its real necessity under CQP specifically is unconfirmed.
6. **Real driver behavior when `VAEncPackedHeaderParameterBuffer`'s own `has_emulation_prevention`
   flag is/isn't set for AV1** (AV1 OBUs use `leb128` length prefixes, not H.264-style start-code
   emulation prevention — this ADR's writer needs no emulation-prevention pass, but whether the
   packed-header buffer's own metadata struct still expects a `has_emulation_prevention` bit set to
   `0` explicitly is unconfirmed pending § Blocking dependency's own resolution, which would
   reveal the real wrapped struct's exact field set).

## References

- [ADR-0001](0001-vaapi-cros-libva-h264-cpu-upload.md) · [ADR-0002](0002-vaapi-h264-p-frame-gop.md)
  — this crate's H.264 baseline/GOP precedent, porting-methodology template
- `crates/mediaway-encoder/src/linux/vaapi/{codec,gop,video}.rs` — current H.264-only
  implementation this ADR adds an AV1 sibling to (`codec.rs:12-27`'s `match` shape)
- `crates/mediaway-encoder/src/vulkan/av1_params.rs` — AV1 encode field-value porting source,
  including its own documented history of three real hardware-verified-invalid mistakes now
  avoided (`reduced_still_picture_header`, `disable_cdf_update`, null optional-tool pointers)
- `crates/mediaway-encoder/src/vulkan/av1_gop.rs` — `GopState`/`Dpb`/`DpbSlot`/`FrameDecision`/
  `FrameRequest`/`ORDER_HINT_BITS_MINUS_1_GOP` porting source
- `crates/mediaway-encoder/src/windows/d3d12_video_encode/bitstream_av1.rs` — AV1 OBU
  byte-writer porting source (`write_leb128`/`obu_header_byte`/`wrap_obu`/`write_tile_info`/
  `write_sequence_header`/`write_frame_header`, lines 40-314)
- `crates/mediaway-encoder/src/windows/d3d12_video_encode/bitstream.rs` — shared `RbspWriter`
  primitives porting source (`write_bit`/`write_bits`/`write_u8`/`byte_align_zero`, lines 23-107)
- `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cros-libva-0.0.13\src\buffer\av1.rs`
  — real vendored `cros-libva` 0.0.13 source read directly for every `AV1*`/`Enc*BufferAV1`
  signature cited above (lines 1-1157 read in full this session); `src/buffer.rs:299-322`
  (`BufferType` enum, confirming the missing packed-header variant), `src/buffer.rs:150-209`
  (`EncSequenceParameter`/`EncPictureParameter`/`EncSliceParameter` dispatch, confirming AV1's
  tile-group buffer reuses `VAEncSliceParameterBufferType`)
- FFmpeg `libavcodec/vaapi_encode_av1.c` — fetched this session (`raw.githubusercontent.com`),
  confirmed: real packed-OBU-header submission (not driver-synthesized), `bit_offset_*`/
  `byte_offset_*` field purpose (driver-side in-place bitstream patching), `VAProfileAV1Profile0`
  usage, mandatory-fields-even-for-all-keyframe list (tile geometry, `base_q_idx`, `tx_mode`)
- `docs/roadmap.md` §2 — "AV1 decode has not been started" (workspace-wide), Vulkan AV1 encode's
  own confirmed driver-maturity-limitation status this ADR's own Consequences section cites
- [`docs/spec/sans-io.md`](../../../../docs/spec/sans-io.md) ·
  [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md) ·
  [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) ·
  [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md) ·
  [`docs/adr/0002-system-oracle.md`](../../../../docs/adr/0002-system-oracle.md)

ADRs are **English**. Numbering is local to this `adr/` folder.
