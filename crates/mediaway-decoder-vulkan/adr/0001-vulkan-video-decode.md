# ADR-0001: Vulkan Video decode via `vulkanalia`; crate placement; H.264/HEVC/AV1 + general-GOP scope

- **Status**: Accepted — implementation begins in staged increments (H.264 general-GOP
  first, HEVC/AV1 addenda following, per the design's own file-layout plan)
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent) — scope explicitly widened beyond the
  `mediaway-encoder-vulkan` precedent (all three codecs + general GOP from
  this ADR's design, not staged H.264-first)
- **Crate**: `mediaway-decoder-vulkan` (new)

## What this pass does and does not do

This ADR and its accompanying crate scaffold (`Cargo.toml`, `docs/README.md`,
`docs/roadmap.md`, `adr/README.md`, workspace registration) are **design
only**. No `src/*.rs` implementation file is written this pass — that is a
deliberate instruction for this task, not a capability limit. Everything
below a struct/module is named is a **plan**, checked against real evidence
where stated, not yet compiled or run.

## Execution environment note (read before trusting any "confirmed" claim below)

This session has **web access** (used below to check `vulkanalia`'s
generated `docs.rs` rustdoc for real struct definitions) but **no
Bash/terminal tool** — the same constraint `mediaway-encoder-vulkan`
ADR-0001's original authoring session records (see that ADR's "⚠️ Execution
environment constraint"). Concretely, this session could **not**:

- Run `cargo check` / `cargo doc` / `cargo test` against `vulkanalia` or any
  draft of this crate.
- Query the test machine (NVIDIA RTX 4090 + Intel UHD 770)
  for real `VkVideoDecodeCapabilitiesKHR` / queue-family decode support.

What this session **did** do: fetched individual `docs.rs` pages for
`vulkanalia` 0.35.0 (the exact version already workspace-pinned for
`mediaway-encoder-vulkan`) to check specific struct names one at a time.
`docs.rs` pages are generated from a real, compiled build of the published
crate — stronger evidence than reading raw generated source blind (as the
encoder-vulkan migration addendum did via `git clone` + `grep`, since no web
tool was available in that session), but still **not** a `cargo check`
against this workspace's own `Cargo.lock`/feature set, and **not** a real
hardware query. Every claim below is labeled by which of these three
evidence tiers it rests on.

## Context

`mediaway-encoder-vulkan` (ADR-0001, hardware-verified 2026-07-29) already
established Vulkan Video as a cross-vendor, cross-OS Khronos extension
family reachable identically from Windows and Linux (`VK_KHR_video_queue` +
per-direction queue extension + per-codec profile extension), placed as a
**portable, non-OS-suffixed** crate. Decode is the same shape:
`VK_KHR_video_decode_queue` + `VK_KHR_video_decode_h264` /
`VK_KHR_video_decode_h265` / `VK_KHR_video_decode_av1`, all **finalized**
Khronos extensions (AV1 decode shipped with SDK support per Khronos's own
"Khronos Releases AV1 Decode in Vulkan Video" blog post, alongside the
existing H.264/H.265 encode SDK support the encoder-vulkan ADR already
verified was current for the test machine's driver generation).

This crate's scope was **explicitly widened by the project owner** beyond
the encoder-vulkan precedent, for this task specifically:

1. **All three codecs from this ADR's design** (H.264, HEVC, AV1) — not
   staged "H.264 first, HEVC/AV1 added in later same-day addenda" the way
   `mediaway-encoder-vulkan` grew.
2. **General GOP support** — P/B reference frames, DPB/reference-picture-list
   management — not the IDR-only/all-intra cut every other decode/encode
   backend in this workspace has shipped so far (`mediaway-decoder-linux`
   VA-API decode, `mediaway-encoder-vulkan`/`-windows` D3D12 encode are all
   IDR-only or all-intra to date).

Both are real, load-bearing scope decisions, not defaults — flagged here per
this task's own instruction and reflected throughout the scope/file-layout
sections below.

## Binding survey: does `vulkanalia` have complete decode struct bindings?

**Real, positive finding** (unlike `mediaway-encoder-vulkan`'s AV1-**encode**
story, where `ash` lacked bindings and `vulkanalia` had to be adopted
mid-project): `vulkanalia` 0.35.0's generated `docs.rs` pages confirm real,
present struct definitions for **every codec's decode picture-info and DPB
reference-slot struct**, checked one page at a time (tier: `docs.rs`, not
`cargo check`):

| Struct | Present (docs.rs) | Fields (as documented) |
|---|---|---|
| `VideoDecodeCapabilitiesKHR` | Yes | `s_type`, `next`, `flags: VideoDecodeCapabilityFlagsKHR` |
| `VideoDecodeInfoKHR` | Yes | `s_type`, `next`, `flags`, `src_buffer`, `src_buffer_offset`, `src_buffer_range`, `dst_picture_resource`, `setup_reference_slot`, `reference_slot_count`, `reference_slots` |
| `VideoDecodeH264ProfileInfoKHR` | Yes | `s_type`, `next`, `std_profile_idc`, `picture_layout: VideoDecodeH264PictureLayoutFlagsKHR` |
| `VideoDecodeH264PictureInfoKHR` | Yes | `s_type`, `next`, `std_picture_info: *const StdVideoDecodeH264PictureInfo`, `slice_count`, `slice_offsets` |
| `VideoDecodeH264DpbSlotInfoKHR` | Yes | `s_type`, `next`, `std_reference_info: *const StdVideoDecodeH264ReferenceInfo` — extends `VideoReferenceSlotInfoKHR` |
| `VideoDecodeH265PictureInfoKHR` | Yes | `s_type`, `next`, `std_picture_info: *const StdVideoDecodeH265PictureInfo`, `slice_segment_count`, `slice_segment_offsets` |
| `VideoDecodeH265DpbSlotInfoKHR` | Yes | `s_type`, `next`, `std_reference_info: *const StdVideoDecodeH265ReferenceInfo` — extends `VideoReferenceSlotInfoKHR` |
| `VideoDecodeAV1PictureInfoKHR` | Yes | `s_type`, `next`, `std_picture_info: *const StdVideoDecodeAV1PictureInfo`, `reference_name_slot_indices: [i32; 7]`, `frame_header_offset`, `tile_count`, `tile_offsets`, `tile_sizes` |
| `VideoDecodeAV1DpbSlotInfoKHR` | Yes | `s_type`, `next`, `std_reference_info: *const StdVideoDecodeAV1ReferenceInfo` — implements `ExtendsVideoReferenceSlotInfoKHR` |
| `StdVideoDecodeAV1PictureInfo` / `StdVideoDecodeAV1ReferenceInfo` | Yes (also directly visible in `vulkanalia-sys/src/video.rs`) | frame type, order hints, ref sign bias, saved order hints, tile/quant/segmentation/filter pointers |

**Not independently confirmed one-by-one** (inferred from the fully
consistent per-codec pattern above, but not individually fetched):
`VideoDecodeH265ProfileInfoKHR`, `VideoDecodeAV1ProfileInfoKHR`,
`VideoDecodeH264SessionParametersCreateInfoKHR` and its HEVC/AV1
equivalents. Given H.264's `ProfileInfoKHR` is confirmed present and every
picture-info/DPB-slot struct above follows the identical
H264/H265/AV1-parallel naming, this is a low-risk inference — but Stage 0
must re-verify by direct `cargo doc` browse before relying on it, per this
workspace's own honesty rules (an inference is not a verification).

**One thing this session could not locate**: the exact `vulkanalia`
extension-command **trait** name(s) that expose `vkCmdDecodeVideoKHR`,
`vkCreateVideoSessionKHR`, etc. as safe Rust methods (the
`Khr*ExtensionDeviceCommands`/`InstanceCommands` blanket-trait pattern
`mediaway-encoder-vulkan`'s migration addendum documents for encode). One
guessed name (`KhrVideoDecodeQueueExtension`) 404'd on `docs.rs` — not a real
blocker (the encode side needed the same kind of `use`-into-scope discovery
and found it fine once real tooling was available), just an open item for
whoever runs Stage 0 with a real `cargo doc`/IDE.

**Also not re-checked this session**: whether `ash` has since gained
`video_decode_h264`/`_h265`/`_av1` bindings (the encoder-vulkan migration
addendum found `ash` had `video_decode_queue`/`h264`/`h265`/`av1` **module
names** present even when `video_encode_av1` was absent — decode may already
have been ahead of encode in `ash` at that time). This does not change the
decision below (`vulkanalia` is already this workspace's one chosen Vulkan
binding, and running two binding crates for no reason was already rejected
in the encode migration ADR for the same underlying cost/benefit), but is
noted for completeness.

## `GpuBufferHandle::Vulkan` already exists — no new common-crate variant needed

Checked `mediaway-common/src/gpu.rs` directly: `GpuBufferHandle::Vulkan {
image: NativeHandle, memory: NativeHandle }` and `GpuDeviceHandle::Vulkan
(NativeHandle)` are both already declared (`#[non_exhaustive]`, present
ahead of any backend using them yet, same pattern as every other platform
variant). This crate's Zero-Copy output path is the **first real consumer**
of `GpuBufferHandle::Vulkan`, but needs no new enum variant or
`mediaway-common` change — reducing this ADR's cross-cutting surface to
zero in that crate.

## Reference-hardware risk: is `VK_KHR_video_decode_queue` even advertised?

`mediaway-encoder-vulkan`'s Stage 0 probe only queried
`VideoCodecOperationFlagsKHR::ENCODE_H264`/`ENCODE_H265` bits on the test
machine (RTX 4090 + Intel UHD 770) — it never queried the
`DECODE_H264`/`DECODE_H265`/`DECODE_AV1` bits. **This is genuinely
unconfirmed** for that machine. NVIDIA's Vulkan Video decode extension
support is generally understood to be mature (NVDEC has backed Vulkan Video
decode for longer than encode, and Khronos's own AV1-decode announcement
names broad vendor support), but "generally understood" is not "verified" —
this workspace's own rules treat that distinction as load-bearing. **This is
Stage 0's first and most important job**, structurally identical to
`probe_video_encode_queue_families`: same instance/physical-device/
`vkGetPhysicalDeviceQueueFamilyProperties2` + `VkQueueFamilyVideoPropertiesKHR`
chain, decode operation-flag bits instead of encode ones. The already-learned
bug from encoder-vulkan's own probe history (`VK_KHR_video_queue` is a
**device** extension, not an instance one) carries forward directly — no
need to re-discover it.

## Decision

> Depend on **`vulkanalia`** (workspace-pinned `0.35`, already a dependency
> for `mediaway-encoder-vulkan` — **no new Cargo dependency** for this crate
> beyond `vulkanalia`/`thiserror`/`mediaway-common`/`mediaway-decoder`,
> mirroring `mediaway-encoder-vulkan`'s own dependency list exactly). Place
> the crate as **`mediaway-decoder-vulkan`** (portable, not OS-suffixed) —
> sibling to `mediaway-encoder-vulkan`, same cross-vendor/cross-OS reasoning.
> Design (not implement, this pass) `impl mediaway_decoder::VideoDecoder` for
> H.264, HEVC, and AV1 with **general P/B-frame GOP support** — reusing
> `mediaway_decoder::DecodeError` as-is (no new error type needed; its
> existing five variants — `Unsupported`/`NoBackend`/`InvalidInput`/
> `Backend`/`Closed` — already cover every failure mode this design
> anticipates).

### Dependency checklist (`deps-policy.md`)

| Question | Answer |
|---|---|
| Need | Real: this crate cannot exist without Vulkan Video decode bindings; `vulkanalia` already reviewed and adopted workspace-wide by `mediaway-encoder-vulkan` ADR-0001's migration addendum. |
| License | Apache-2.0, already allow-listed in this workspace's `cargo deny` config (re-confirmed by the prior ADR, not re-litigated here). |
| Maintenance | Same crate, same version already in the workspace lockfile position — no new maintenance surface. |
| API stability | `0.35`, already minor-pinned workspace-wide; any future bump is reviewed once for the whole workspace, not per-crate. |
| Alternatives | `ash` — not re-adopted; running two Vulkan binding crates for one crate's needs was already rejected for the same underlying cost/benefit in the encode migration ADR (two loader/`Entry`/`Instance` types, doubled FFI review surface, for a codec family `vulkanalia` already covers). `ralfbiedert/vulkan_video` — resurveyed this session (`crates.io`/GitHub): still early-stage; its own changelog describes a January 2025 reactivation after being a single-frame H.264 proof of concept, not a maintained, general-GOP-capable, three-codec decoder. Not adopted, same reasoning as the encode ADR's original survey. |
| Cost | Zero incremental — `vulkanalia`'s `Cargo.toml` entry, feature set, and runtime-loader posture (`libloading`, no build-time Vulkan SDK link) are unchanged from `mediaway-encoder-vulkan`'s existing use. |
| Unsafe surface | Every Vulkan call is FFI — `#![allow(unsafe_code)]` at this crate's root (mirrors `mediaway-encoder-vulkan`'s own choice, not `mediaway-decoder-linux`'s `#![forbid(unsafe_code)]`, since VA-API's `cros-libva` absorbs all `unsafe` into that dependency while raw Vulkan does not offer an equivalent safe wrapper crate). `// SAFETY:` required on every real `unsafe` block once code is written. |

### Crate placement: `mediaway-decoder-vulkan` (not OS-suffixed)

Identical reasoning to `mediaway-encoder-vulkan` ADR-0001's own placement
section (not repeated in full): the session/DPB/parameter-set state machine
is the same Vulkan API surface on Windows and Linux; only external-memory
Zero-Copy import differs per OS (`VK_KHR_external_memory_win32` vs. `_fd`),
deferred here the same way encode deferred it. Naming this
`mediaway-decoder-windows-vulkan` / `-linux-vulkan` would force a near-total
duplicate crate later for one portable API — rejected for the same reason.

### Errors: reuse `mediaway_decoder::DecodeError`, no new type

`DecodeError`'s five variants (`Unsupported`, `NoBackend`, `InvalidInput`,
`Backend`, `Closed`) are generic enough to cover every failure this design
anticipates: unsupported codec/profile/output preference → `Unsupported`;
bad SPS/PPS/sequence-header syntax or corrupt slice/tile data →
`InvalidInput`; any `VkResult` failure from session/DPB/command-buffer calls
→ `Backend`; session already flushed → `Closed`. No decode-specific variant
(e.g. a hypothetical `ReferenceFrameMissing`) is added this ADR — if Stage 1
implementation finds a real need for finer-grained errors, that is a small,
separate, code-grounded ADR amendment, not speculated here.

## Scope (this ADR's design — see § File layout plan for the implementation staging)

**In scope, designed (not implemented) this pass:**

- Stage 0: capability probe — `VK_VIDEO_CODEC_OPERATION_DECODE_H264_BIT_KHR`
  / `_H265_BIT_KHR` / `_AV1_BIT_KHR` per queue family, structurally identical
  to `mediaway-encoder-vulkan::probe::probe_video_encode_queue_families`.
- Stage 1: H.264 decode session with **general GOP support** — own
  SPS/PPS/slice-header parser (reusing `mediaway_sw::h264`'s Annex-B framing,
  see § Bitstream-parser reuse below), DPB with sliding-window reference
  management, reference-picture-list construction (`RefPicList0`/`1` for
  P/B slices), `VkVideoSessionKHR` decode session,
  `vkCmdDecodeVideoKHR` submission, both CPU-readback and
  `GpuBufferHandle::Vulkan` Zero-Copy output paths.
- Stage 2: HEVC — own VPS/SPS/PPS/slice-segment-header parser (new; no
  existing crate to reuse, see below), Main profile, general P/B GOP.
- Stage 3: AV1 — own OBU-level sequence-header/frame-header/tile-group
  parser (new), `ref_frame_idx`-based reference management, **film-grain
  synthesis deferred as an explicit later step** (see § AV1 film grain).
- Zero-Copy GPU output (`GpuBufferHandle::Vulkan`) alongside CPU readback
  (`vkCmdCopyImageToBuffer` + host-visible mapped buffer, NV12 tight-packed
  the same way `mediaway-decoder-linux`'s `vaapi/nv12.rs` and
  `mediaway-decoder-windows`'s `wmf/cpu.rs` already do) for
  `VideoOutputPreference::CpuFramesOk`.

**Out of scope / explicit scope cuts, documented up front (not silently
assumed solved by "general GOP support"):**

- **AV1 film-grain synthesis** — deferred to a step after Stage 3's base
  decode is hardware-verified; see § AV1 film grain.
- **HEVC scalability (SHVC), range extensions (RExt), screen-content coding
  (SCC), tiles/WPP beyond single-tile-per-picture** — Main profile,
  8-bit 4:2:0, single tile, no long-term references beyond a sliding window.
- **10/12-bit profiles** (HEVC Main10, AV1 high bit depth) — blocked on a
  real gap: `mediaway_common::PixelFormat` has **no 10-bit variant today**
  (confirmed: `Nv12`/`I420`/`Bgra8`/`Rgba8`/`Yuyv` only, checked directly in
  `mediaway-common/src/formats.rs`). A 10-bit output format needs its own
  `mediaway-common` change + review before this crate can claim full
  Main10/AV1-10bit coverage — explicitly out of this ADR's scope, flagged as
  a blocking dependency, not silently ignored.
- **Interlaced / field pictures** — progressive only, matching every other
  decode backend in this workspace so far.
- **Multi-slice-per-picture generality beyond what this crate's own test
  streams exercise** — "general GOP" here means P/B reference-frame
  management across pictures, not necessarily every legal multi-slice/
  multi-tile arrangement a real-world encoder might produce on day one;
  flagged as an open item (see § Open questions), not claimed solved.
- **Long-term reference marking / MMCO edge cases beyond a sliding window**
  — deferred; sliding-window DPB eviction lands first.
- Production robustness: `vkWaitForFences` timeouts, multi-queue/threading,
  per-object RAII beyond instance/device (same cuts `mediaway-encoder-vulkan`
  Stage 1 made, same reasoning).

## Bitstream-parser reuse: what's actually shared vs. new

Checked `mediaway-sw::h264` directly (`crates/mediaway-sw/src/h264/nal.rs`):

- **`split_annex_b`** (Annex-B start-code scanning + emulation-prevention
  byte removal, returns raw `&[u8]` slices) is genuinely **codec-agnostic**
  byte framing — it does not interpret `nal_unit_type` at all. **Reusable
  as-is for both H.264 and HEVC** (both use ITU-T Annex-B start-code
  framing), the same way `mediaway-decoder-linux`'s VA-API decoder already
  reuses it for H.264.
- **`NalUnit::parse`** decodes a **1-byte** H.264 NAL header
  (`forbidden_zero_bit` + 2-bit `nal_ref_idc` + 5-bit `nal_unit_type`) — this
  is H.264-specific and **not reusable for HEVC**, whose NAL header is
  **2 bytes** (`forbidden_bit` + 6-bit `nal_unit_type` + 6-bit
  `nuh_layer_id` + 3-bit `nuh_temporal_id_plus1`). This crate's HEVC path
  calls `split_annex_b` directly for framing, then parses the 2-byte HEVC
  NAL header itself (new, local code).
- **`BitReader`** (bit-level RBSP reader) is reusable for both H.264 and
  HEVC's own SPS/PPS/VPS/slice-header parsers — generic bit consumption, no
  H.264-specific state.
- **AV1 has no reusable parser in this workspace at all.** `mediaway-sw::av1`
  is a `rav1e`-backed AV1 **encoder** adapter (confirmed by reading
  `crates/mediaway-sw/src/av1.rs` directly) — it contains no OBU/sequence-
  header/frame-header **parsing** code, only `rav1e`'s own encode API calls.
  This crate's AV1 path needs an entirely new, from-scratch low-overhead-
  bitstream-format OBU scanner + sequence/frame-header parser. Note:
  `mediaway-encoder-vulkan`'s own `nal.rs` has a **test-only**
  `scan_obu_headers`/`read_leb128` pair (written to verify its own encoded
  output, not to build decode-side picture-info structs) — structurally
  similar groundwork exists in a sibling crate, but is `#[cfg(test)]`-gated
  and private, so it would be duplicated, not imported, unless a future ADR
  extracts a shared OBU-framing helper (not decided here — see
  § Open questions).
- **HEVC/AV1 parser placement**: kept **local to this crate** for now (new
  `hevc_params.rs`/`av1_params.rs`-style modules, following
  `mediaway-encoder-vulkan`'s own per-codec file split), not extracted into
  `mediaway-sw` — this workspace's packaging philosophy avoids inventing a
  shared-crate layer before a second real consumer needs it
  (`docs/spec/crate-packaging.md` § "When to add a crate"). Revisit only
  once another crate (e.g. a future software HEVC/AV1 decode fallback in
  `mediaway-sw`) genuinely needs the same syntax elements.

## AV1 film grain — a separate step, explicitly flagged

Per the Vulkan AV1 decode extension's design, film-grain **parameters**
(`StdVideoAV1FilmGrain`, confirmed present in `vulkanalia-sys/src/video.rs`
directly) are carried in the sequence/frame header, but grain **synthesis**
(turning `apply_grain = 1` parameters into actual reconstructed-picture
noise) is a distinct operation from base decode — whether it is applied by
the decode implementation automatically, gated behind a specific
`VkVideoDecodeAV1CapabilitiesKHR` flag, or left entirely to the application
was **not confirmed this session** (would need either a real driver query or
a closer read of the finalized `VK_KHR_video_decode_av1` spec text than this
session performed). This crate's Stage 3 plan: decode AV1 **without** grain
synthesis first (parse `film_grain_params` but do not apply them — a "clean"
reconstructed picture, conformant for content without `apply_grain`, visibly
different from a reference decoder's output for grainy content), then add
synthesis as an explicit, separately-verified follow-up once base AV1 decode
is hardware-verified. Once implemented, the "no grain synthesis yet" gap
must be documented as an honest caveat (`docs/spec/caveats-and-clarity.md`)
on whatever public method exposes AV1 decode — not silently presented as
complete AV1 decode.

## File layout plan (design only — no file below exists yet)

Mirrors `mediaway-encoder-vulkan`'s per-codec/per-concern split, plus new
DPB-management files decode needs that encode's IDR-only scope never did.
Every file is planned to stay under this workspace's 1000-line-per-source
rule; DPB/reference-list logic is genuinely new complexity, so more files
are planned up front here than encode needed, rather than splitting
reactively after the fact:

```text
src/lib.rs                    — crate doc, module wiring, re-exports
src/probe.rs                  — Stage 0: decode queue-family capability probe
src/session.rs                — VkVideoSessionKHR create/bind/capability
                                 query; DecodeProfile enum (H264/Hevc/Av1),
                                 codec-generic like encoder-vulkan's
                                 EncodeProfile
src/dpb.rs                    — DPB slot bookkeeping: fixed-capacity slot
                                 array sized per codec's max_dpb_size,
                                 sliding-window eviction, occupied/free
                                 tracking — no GPU calls, sans-io-testable
src/h264_params.rs            — H.264 SPS/PPS parse (BitReader reuse) +
                                 StdVideoDecodeH264PictureInfo construction
src/h264_slice.rs             — slice-header parse + RefPicList0/1
                                 construction (split out if h264_params.rs
                                 would exceed 1000 lines)
src/hevc_params.rs            — HEVC VPS/SPS/PPS parse (new 2-byte NAL
                                 header + BitReader reuse) +
                                 StdVideoDecodeH265PictureInfo construction
src/hevc_slice.rs             — slice-segment-header parse + ref-list
                                 construction
src/av1_params.rs             — AV1 OBU scan + sequence-header/frame-header
                                 parse (new, no reuse available) +
                                 StdVideoDecodeAV1PictureInfo construction
src/av1_refs.rs                — ref_frame_idx reference management
src/session_command.rs        — shared per-frame vkCmdDecodeVideoKHR
                                 recording + barriers (codec-generic half)
src/session_command_h264.rs   — per-codec picture-info pNext payload
src/session_command_hevc.rs
src/session_command_av1.rs
src/cpu_readback.rs           — vkCmdCopyImageToBuffer + host-visible map +
                                 NV12 tight-pack (mirrors vaapi/nv12.rs,
                                 wmf/cpu.rs "same layout" convention)
src/zero_copy.rs              — GpuBufferHandle::Vulkan construction +
                                 documented handle-lifetime/fence contract
src/decoder.rs                — VulkanVideoDecoder: impl
                                 mediaway_decoder::VideoDecoder, codec-generic
                                 dispatch over DecodeProfile
```

Sibling `*_tests.rs` per file per this workspace's testing convention. DPB
occupancy/reference-list-construction logic (`dpb.rs`,
`h264_slice.rs`/`hevc_slice.rs`/`av1_refs.rs`'s ref-list math) is pure,
sans-io, and **unit-testable without any Vulkan device or GPU** — flagged
below as the highest-value real test coverage this crate can get before any
hardware is involved, mirroring how `mediaway_sw::h264`'s own bitstream
parsing is tested independent of any decode/encode hardware.

## DPB / reference-management design sketch (ZCA shape — no code this pass)

```rust
// Codec-generic profile selection, mirrors encoder-vulkan's EncodeProfile.
enum DecodeProfile {
    H264(H264ProfileParams),
    Hevc(HevcProfileParams),
    Av1(Av1ProfileParams),
}

// Fixed-capacity DPB sized once from the parsed SPS/sequence-header's own
// max_dpb_size / max reference-frame count (H.264/HEVC up to 16, AV1 8 ref
// slots) — a `Vec` sized once at session-parameter time, not a per-frame
// allocation; `SmallVec` only if a stack-sized bound turns out cheaper than
// one `Vec` allocation per session (decided at Stage 1 implementation time,
// not this ADR — see ZCA rule in AGENTS.md).
struct Dpb {
    slots: Vec<Option<DpbSlot>>,
}

struct DpbSlot {
    // VkVideoReferenceSlotInfoKHR is built fresh per vkCmdDecodeVideoKHR
    // call from this slot's current picture_resource + std_reference_info —
    // no persistent Vulkan struct is kept alive across frames, only the
    // data needed to reconstruct one.
    picture_resource: PictureResource,     // decoded-picture image + layer
    poc_or_order_hint: i64,                // codec-specific: H.264 PicOrderCnt,
                                            // HEVC POC, AV1 OrderHint
    frame_num_or_ref_idx: u32,
    used_for_reference: bool,
}

// Pure, sans-io reference-list construction — no GPU calls, unit-testable
// against hand-built SPS/slice-header fixtures the same way
// mediaway_sw::h264's own parser is tested.
fn build_ref_pic_lists(dpb: &Dpb, slice: &SliceHeader) -> RefPicLists { .. }
```

- `vkCmdBeginVideoCodingKHR`'s `pReferenceSlots` is built fresh per picture
  from the DPB's currently-occupied slots — no long-lived bound state beyond
  the session itself, same "recompute per submission" shape
  `mediaway-encoder-vulkan`'s own per-frame command recording already uses.
- No `Box<dyn _>` / `dyn Trait` — closed, concrete `DecodeProfile` enum
  dispatch, matching every other backend in this workspace.
- `VulkanVideoDecoder` (the concrete `mediaway_decoder::VideoDecoder` impl)
  keeps instance/device/video session/session parameters/DPB images/command
  pool/fence/query pool alive across `push_packet`/`poll_frame` calls, same
  reusable-session shape as `mediaway-encoder-vulkan::VulkanVideoEncoder`.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| `ash` instead of `vulkanalia` | Running two Vulkan binding crates in this workspace for no reason beyond one crate's needs was already rejected in `mediaway-encoder-vulkan`'s migration addendum (doubled loader/`Entry`/`Instance` types, doubled FFI review surface, no offsetting benefit) — `vulkanalia` is already the workspace's one chosen binding and this session found no decode-specific gap that would justify reopening that decision. |
| `mediaway-decoder-windows-vulkan` / `mediaway-decoder-linux-vulkan` (OS-suffixed) | Forces a near-duplicate crate later for one portable Vulkan API — same rejection as `mediaway-encoder-vulkan`'s own placement section. |
| `ralfbiedert/vulkan_video` (third-party Rust Vulkan Video crate) | Resurveyed this session: real, but still early-stage (a January 2025 "reactivated for current `ash`" note, originally a single-frame H.264 proof of concept per its own README) — not a general-GOP, three-codec-capable decoder to build on. Same disposition as the encode ADR's original survey. |
| Stage H.264-only first, defer HEVC/AV1 to later same-day addenda (mirroring `mediaway-encoder-vulkan`'s own growth pattern) | Explicitly rejected — this task's own instruction (project-owner decision) widens this ADR's design scope to all three codecs and general GOP from the start. Implementation may still land in stages (see roadmap), but the **design** commits to all three now, not "H.264 only, others out of scope." |
| Extract HEVC/AV1 bitstream parsers into `mediaway-sw` now | Deferred — no second real consumer exists yet for HEVC/AV1 syntax parsing; this workspace's packaging convention avoids inventing a shared-crate layer before it is needed (`docs/spec/crate-packaging.md`). Revisit once a real second consumer appears. |
| Depend on a general-purpose pure-Rust HEVC/AV1 decoder crate (e.g. `rust_h265`) for header parsing only | Not adopted: these are full pixel-reconstruction decoders, not lightweight syntax-only parsers; pulling one in only to read SPS/VPS/sequence-header fields (while this crate's own GPU decode does the actual reconstruction) would import a large, mostly-unused dependency surface for a small extraction this crate can write itself, mirroring why `mediaway-decoder-linux` wrote its own H.264 SPS/PPS/slice parser rather than reusing a full software decoder. |

## Consequences

### Positive

- Real, `docs.rs`-grounded evidence that `vulkanalia` already has complete
  per-codec decode struct bindings (H.264/HEVC/AV1 picture-info + DPB-slot
  structs) — a genuinely better starting position than encode's AV1 story,
  found and documented before any code is written rather than discovered
  mid-implementation.
- `GpuBufferHandle::Vulkan` already exists — zero `mediaway-common` churn
  needed for this crate's Zero-Copy output path.
- Zero new Cargo dependency — reuses `vulkanalia`/`thiserror`/
  `mediaway-common` exactly as already reviewed for `mediaway-encoder-vulkan`.
  Reduces deps-policy review to "does the same crate's decode module surface
  actually compile," not a fresh license/maintenance review.
- Reference-list/DPB logic is designed sans-io from the start — genuinely
  unit-testable without hardware, unlike the GPU submission code around it.
- Crate boundary matches the sibling encoder's, easing future joint review
  and any shared external-memory Zero-Copy interop work later.

### Negative / Trade-offs

- **Nothing in this ADR is hardware-verified** — no shell/build tool this
  session, same constraint the original `mediaway-encoder-vulkan` ADR
  disclosed. Whether the RTX 4090 even advertises a
  decode queue family for any of these three codecs is unconfirmed.
- General GOP support (DPB/reference-list construction across three codecs)
  is substantially larger unsafe-FFI-adjacent surface than any decode/encode
  backend this workspace has shipped so far — real, new bug surface once
  code is written (mirrors the kind of field-by-field debugging
  `mediaway-encoder-vulkan`'s AV1 addendum needed, but across three codecs'
  reference-management logic instead of one codec's encode parameters).
- HEVC and AV1 bitstream/header parsing has **zero existing code to build
  on** in this workspace (confirmed: `mediaway-sw` has no HEVC module at
  all, and its `av1` module is an encoder, not a parser) — both are
  from-scratch work, unlike H.264's reuse of `mediaway_sw::h264`'s framing.
- No 10-bit `PixelFormat` yet blocks true Main10/AV1-10bit output end to end
  even once the Vulkan-side plumbing exists — a real, named ceiling on this
  ADR's coverage, not silently ignored.
- AV1 film-grain synthesis is explicitly deferred past base decode —
  visible, honest quality gap for grainy AV1 content until that lands.

## Open questions / risks (explicit, for whoever picks this up)

1. **Does any queue family on the test machine actually
   advertise `VK_VIDEO_CODEC_OPERATION_DECODE_H264_BIT_KHR` /
   `_HEVC_BIT_KHR` / `_AV1_BIT_KHR`?** Unconfirmed — no shell/hardware access
   this session. Stage 0's first job, structurally identical to
   `probe_video_encode_queue_families`.
2. **Exact `vulkanalia` extension-command trait name(s)** for
   `vkCmdDecodeVideoKHR`/session creation — decode *struct* bindings are
   confirmed present via `docs.rs`; the specific blanket-trait name(s) that
   must be `use`d into scope were not located this session (one guessed name
   404'd). Trivial to find with real `cargo doc`/IDE access; not a real
   blocker.
3. **`VideoDecodeH265ProfileInfoKHR` / `VideoDecodeAV1ProfileInfoKHR` /
   per-codec `SessionParametersCreateInfoKHR` struct presence** — inferred
   from the consistent pattern already confirmed for picture-info/DPB-slot
   structs, but not independently fetched one-by-one. Re-verify directly
   before Stage 1 relies on it.
4. **AV1 film-grain synthesis**: which capability flag (if any) gates it,
   and whether any driver generation implements synthesis via the decode
   extension itself vs. leaving it entirely to the application — unconfirmed
   this session (see § AV1 film grain).
5. **Multi-slice-per-picture / multi-tile generality** — this ADR's "general
   GOP" scope is about P/B reference-frame management across pictures, not a
   guarantee of handling every legal multi-slice/multi-tile arrangement a
   real-world encoder might produce on day one. Flagged, not silently
   assumed solved.
6. **HEVC/AV1 bitstream-parser placement** (local to this crate vs. future
   extraction to `mediaway-sw`) — deferred per § Bitstream-parser reuse;
   revisit only once a second real consumer needs the same syntax elements.
7. **10-bit `PixelFormat` gap** — needs its own `mediaway-common` ADR/change
   before this crate can claim full HEVC Main10 / AV1 high-bit-depth output;
   8-bit-only decode is this ADR's honest ceiling until that lands.
8. **Whether `ash` has since gained complete decode bindings** — not
   re-checked this session (the encode migration ADR's finding was specific
   to `video_encode_av1`); does not change the `vulkanalia` decision, noted
   for completeness only.

## References

- `mediaway-encoder-vulkan` ADR-0001
  (`../../mediaway-encoder-vulkan/adr/0001-vulkan-video-encode-ash-probe.md`)
  — direct structural precedent for probe/session staging, crate placement
  reasoning, `ash`→`vulkanalia` migration finding, and this ADR's honesty
  conventions.
- `mediaway-decoder-linux` ADR-0001
  (`../../mediaway-decoder-linux/adr/0001-vaapi-h264-cpu-out.md`) — IDR-only
  decode precedent, bitstream-parser-reuse reasoning this ADR extends to
  HEVC/AV1, "zero real-hardware verification" caveat pattern.
- `vulkanalia` on crates.io: <https://crates.io/crates/vulkanalia> (Apache-2.0,
  0.35.0) · `docs.rs`: <https://docs.rs/vulkanalia/latest/vulkanalia/>
- `VK_KHR_video_decode_h264` spec: <https://registry.khronos.org/vulkan/specs/latest/man/html/VK_KHR_video_decode_h264.html>
- Khronos, "Khronos Releases AV1 Decode in Vulkan Video with SDK Support for
  H.264/H.265 Encode": <https://www.khronos.org/blog/khronos-releases-vulkan-video-av1-decode-extension-vulkan-sdk-now-supports-h.264-h.265-encode>
- `ralfbiedert/vulkan_video`: <https://github.com/ralfbiedert/vulkan_video> ·
  <https://crates.io/crates/vulkan_video> (early-stage, resurveyed this
  session)
- `mediaway-decoder` facade traits: `crates/mediaway-decoder/src/video.rs`,
  `crates/mediaway-decoder/src/error.rs`
- `mediaway-common` `GpuBufferHandle`: `crates/mediaway-common/src/gpu.rs`
- `mediaway-sw::h264` bitstream framing:
  `crates/mediaway-sw/src/h264/nal.rs`, `crates/mediaway-sw/src/h264.rs`
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md) ·
  [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md) ·
  [`docs/adr/0003-crate-packaging.md`](../../../docs/adr/0003-crate-packaging.md) ·
  [`docs/adr/0012-unprefixed-reusable-cores.md`](../../../docs/adr/0012-unprefixed-reusable-cores.md)
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md)
- [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md)
- [`docs/conventions/testing.md`](../../../docs/conventions/testing.md)

ADRs are **English**. Numbering is local to this `adr/` folder.

## Addendum (2026-07-30): H.264 implementation

Implements Stage 0 (probe) and Stage 1 (H.264, sans-io complete + tested;
GPU pipeline compiles/runs but decode output is a known, unresolved bug).
Files: `probe.rs`, `session.rs`, `dpb.rs`, `h264_params.rs`, `h264_slice.rs`,
`session_command.rs`, `session_command_h264.rs`, `cpu_readback.rs`,
`zero_copy.rs`, `decoder.rs`, plus sibling `*_tests.rs` and
`tests/hardware_h264_decode.rs`. HEVC/AV1 are untouched (`hevc_params.rs`
etc. do not exist yet), per this round's scope.

### Stage 0's biggest open question, answered: real decode queue support

`probe::probe_video_decode_queue_families`, run for real against the
test machine (`cargo test -p mediaway-decoder-vulkan probe
-- --nocapture`):

```text
vulkan device: NVIDIA GeForce RTX 4090 (DISCRETE_GPU)
  h264_decode_queue_family=Some(3) h265_decode_queue_family=Some(3) av1_decode_queue_family=Some(3)
vulkan device: Intel(R) UHD Graphics 770 (INTEGRATED_GPU)
  h264_decode_queue_family=Some(1) h265_decode_queue_family=Some(1) av1_decode_queue_family=Some(1)
```

**Both** GPUs advertise a real decode queue family for all three codecs —
better than the encode side's own probe result (Intel's Windows Vulkan
driver advertised zero encode queues). This closes the ADR's single biggest
open risk.

### Real `vulkanalia` findings (correcting/confirming the ADR's inferences)

- The extension-command trait is **`KhrVideoDecodeQueueExtensionDeviceCommands`**
  (confirmed by grepping the vendored `vulkanalia-0.35.0` source directly,
  `src/vk/extensions.rs`) — it exposes exactly one method,
  `cmd_decode_video_khr`. Session/DPB/parameter-set lifecycle calls
  (`create_video_session_khr`, `cmd_begin_video_coding_khr`,
  `cmd_end_video_coding_khr`, `get_physical_device_video_capabilities_khr`,
  etc.) live on the **same** `KhrVideoQueueExtensionDeviceCommands`/
  `KhrVideoQueueExtensionInstanceCommands` traits the encode side already
  uses — no separate decode-specific session trait needed.
- Every struct the ADR's binding survey listed as "not independently
  confirmed" is confirmed present with the expected fields:
  `VideoDecodeH264ProfileInfoKHR`, `VideoDecodeH264CapabilitiesKHR`,
  `VideoDecodeH264PictureInfoKHR`, `VideoDecodeH264DpbSlotInfoKHR`,
  `VideoDecodeH264SessionParametersCreateInfoKHR`/`AddInfoKHR`,
  `StdVideoDecodeH264PictureInfo`, `StdVideoDecodeH264ReferenceInfo`.
- **Real, load-bearing finding the ADR did not anticipate**:
  `StdVideoDecodeH264PictureInfo` has **no `pRefLists`-equivalent field**,
  and **no `StdVideoDecodeH264ReferenceListsInfo` struct exists at all** in
  these bindings (unlike H.264 **encode**'s `StdVideoEncodeH264PictureInfo`,
  which does carry `pRefLists`). For decode, the hardware parses
  `ref_pic_list_modification()`/`dec_ref_pic_marking()` itself directly from
  the raw slice bytes handed to `vkCmdDecodeVideoKHR` — the application only
  supplies the *set* of currently valid DPB reference slots, not an ordered
  list. `h264_slice.rs`'s `default_ref_pic_list0`/
  `apply_ref_pic_list_modifications` therefore exist as sans-io,
  independently tested documentation of what the hardware's own list
  construction should produce (useful for validating DPB bookkeeping), not
  as something fed to any Vulkan call.
- `VkVideoDecodeCapabilityFlagsKHR::DPB_AND_OUTPUT_COINCIDE` is real and is
  what this crate's single combined DPB+output image design requires
  (`session.rs::query_capabilities` rejects a driver that only advertises
  `DPB_AND_OUTPUT_DISTINCT`, via `VulkanDecodeError::SeparateReferenceImagesRequired`
  — not hit on either reference GPU).
- The DPB image is **one `2D_ARRAY` image, one shared `2D_ARRAY` view**
  covering every layer (not one single-layer view per slot, which this
  session tried first) — `VkVideoPictureResourceInfoKHR::baseArrayLayer`
  selects the slot *within* that one shared view. This matches the
  real-implementation pattern; a single-layer-view-per-slot design (with
  `baseArrayLayer` set to `0` since the view already narrowed) decoded
  without any `VkResult` error but produced all-zero output — a case where
  the lack of a validation layer (no Vulkan SDK on the test
  machine — same constraint every session working on this
  workspace's Vulkan crates has recorded) let a real spec violation through
  silently.

### Two real spec-compliance bugs found only by hand-constructing a stream and cross-checking against ffmpeg

Writing the hardware-gated integration test's hand-crafted H.264 bitstream
(see below) surfaced two bugs in `h264_slice.rs`/`h264_params.rs` that no
unit test with a hand-picked-to-fit fixture would have caught, since the
fixtures could always be constructed to happen to avoid the gap:

1. **Missing `num_ref_idx_active_override_flag`**: the P-slice header parser
   jumped straight from `pic_order_cnt_lsb` to `ref_pic_list_modification_flag_l0`,
   skipping this field entirely — on any real P-slice, this misaligns every
   subsequent bit read (ref-list modification, `dec_ref_pic_marking`,
   `slice_qp_delta`). Fixed; added `H264SliceHeader::num_ref_idx_l0_active`.
2. **Missing `deblocking_filter_control_present_flag`-gated slice fields**
   (`disable_deblocking_filter_idc` + conditional
   `slice_alpha_c0_offset_div2`/`slice_beta_offset_div2`) and **no rejection
   of `redundant_pic_cnt_present_flag`** — both would silently misparse a
   real stream's slice headers past that point. Fixed: slice parsing now
   reads the deblocking fields when the PPS signals them;
   `H264Pps::parse` now rejects `redundant_pic_cnt_present_flag == 1`
   (this crate's slice parser does not read `redundant_pic_cnt`, so a stream
   signaling it would misparse everything downstream — rejected at the PPS
   boundary instead of failing confusingly deep in slice parsing).

Neither bug affects what this crate feeds to the Vulkan decode call directly
(both only shift *this crate's own* sans-io bit-position tracking, not the
raw bytes handed to hardware) — but both would have made this crate's own
DPB/POC bookkeeping wrong on any real-world stream using these (common)
fields, and the second would have silently broken future streams using
per-slice deblocking overrides.

### Hardware-gated integration test: real device found, decode ran, output is wrong (FAIL, not skip)

`tests/hardware_h264_decode.rs` hand-constructs a full Annex-B stream (SPS +
PPS + IDR slice + one P slice, 64x16, 4 macroblocks) using only `I_PCM`
macroblocks (raw, uncoded samples — sidesteps needing a real CAVLC
residual/CBP-table encoder) and one `P_Skip` macroblock (zero-motion
reference copy) to get real, controllable, varying content without an
encoder. Deblocking is explicitly disabled
(`disable_deblocking_filter_idc = 1`) so expected pixel values are exact
literals, not deblocking-filtered approximations (found necessary after the
first cross-check against `ffmpeg` showed filtered, not literal, values).

**Result: `VulkanVideoDecoder::open` succeeds (a real decode-capable device
was found — this is not a skip), `push_packet` and the underlying
`vkCmdDecodeVideoKHR` submission complete with no `VkResult` failure
anywhere, but the returned NV12 frame reads back as all-zero (or, with a
diagnostic pre-fill, as *unchanged* from whatever was already in the DPB
image) instead of the expected literal pixel values. The test **fails**
honestly (`cargo test -p mediaway-decoder-vulkan` currently reports this one
real failure) — it is not softened into a skip, since a real, decode-capable
device genuinely was found and the bug is real.**

Cross-validation performed to isolate the fault domain (all via the exact
same hand-crafted byte stream, dumped to a file):

| Check | Method | Result |
|---|---|---|
| Bitstream itself valid? | `ffmpeg -f h264 -i test_stream.h264 ...` (software decode) | **Yes** — decodes 2 frames; with deblocking disabled, pixel values match the literal `I_PCM`/`P_Skip` values exactly (200/50/90 IDR; 200/220/90 P) |
| Same GPU's hardware decoder capable? | `ffmpeg -hwaccel cuda -c:v h264_cuvid -f h264 -i test_stream.h264 ...` (NVDEC via CUVID, a different, mature API, same physical RTX 4090) | **Yes** — byte-exact match to the software decode, confirming the GPU's H.264 decode circuitry handles this exact stream (including `P_Skip` motion compensation and `I_PCM`) correctly |
| This crate's `cpu_readback.rs` correct? | Diagnostic: `vkCmdCopyBufferToImage` a known pattern into the DPB image's layer 0, then read it back via the real `cpu_readback::read_nv12` | **Yes** — byte-exact round-trip (77/55 pattern read back unchanged) |
| This crate's `vkCmdDecodeVideoKHR` call producing output? | Same diagnostic pattern pre-filled, then run the real decode over it | **No** — image content after decode is byte-identical to the pre-fill pattern; the decode command produced no observable write |

This conclusively narrows the bug to this crate's own Vulkan Video decode
command construction (`session_command_h264.rs`'s `record_and_submit_h264`,
possibly `session.rs`'s session/session-parameters creation) — not the
bitstream, not the hardware, not the readback path.

**Hypotheses already tried and ruled out** (each changed nothing about the
all-zero/unchanged result):

- Requesting `STD_VIDEO_H264_PROFILE_IDC_BASELINE` instead of `_HIGH` (to
  exactly match the test stream's signaled profile).
- Rounding `VkVideoDecodeInfoKHR::srcBufferRange` up to the driver-reported
  `minBitstreamBufferSizeAlignment` (256 bytes on this GPU), zero-padding the
  uploaded buffer to match — a real gap (this alignment field was dropped
  from the initial cut as "unused" and is now restored and applied; kept,
  since it is a real spec requirement even though it did not fix this bug).
- Setting `StdVideoDecodeH264PictureInfoFlags::is_intra` for I-slice
  pictures (previously never set at all) — kept, also a real gap.
- Skipping the one-time `vkCmdControlVideoCodingKHR` `RESET` bracket
  entirely.
- The single-shared-`2D_ARRAY`-view vs. per-slot-single-layer-view change
  described above (this **did** matter for the earlier readback
  investigation's methodology, ruling out a *different*, real bug in the
  first design, but did not by itself fix the all-zero decode output).

**Not yet tried** (next steps for whoever picks this up): compare
`VkVideoBeginCodingInfoKHR`/`VkVideoDecodeInfoKHR`'s reference-slot
"activation" sequencing byte-for-byte against a known-working open-source
implementation (e.g. FFmpeg's `vulkan_h264.c`, which was not available to
consult directly in this session beyond general recollection); try
requesting the validation layer path via a real Vulkan SDK install (none is
present on the test machine, the same constraint every
session on this workspace's Vulkan crates has recorded) to get real
diagnostic messages instead of blind hypothesis-testing; try a
`VkQueryPoolVideoEncodeFeedbackKHR`-style query (if a decode equivalent
exists) to see whether the driver reports the decode operation as having run
at all.

### Other real findings

- `mediaway-encoder-vulkan::VulkanVideoEncoder` makes every pushed frame an
  independent key frame (confirmed directly) — it cannot produce a real
  P-frame, so this round's integration test hand-constructs its own stream
  rather than using this workspace's own encoder, per the task's own
  suggested alternative.
- `GpuBufferHandle::Vulkan`'s two opaque fields needed a concrete encoding
  decision, made here: `image` = the DPB image's raw `VkImage` handle,
  `memory` = the DPB slot's array-layer index (offset by one so slot `0`
  still round-trips through `NativeHandle`'s non-zero representation) — not
  a `VkDeviceMemory` handle, since every layer shares one allocation, which
  alone would not tell a caller which layer this frame's pixels are in.

### Status as of the 2026-07-30 addendum (superseded — see below)

Sans-io DPB/bitstream-parsing logic (`dpb.rs`, `h264_params.rs`,
`h264_slice.rs`) is real, hardware-independent, and fully unit-tested (41
tests, `cargo test -p mediaway-decoder-vulkan --lib`). The Vulkan plumbing
(`probe.rs` through `decoder.rs`) compiles cleanly, passes `cargo clippy -p
mediaway-decoder-vulkan --all-targets` with zero warnings, and runs a real
multi-frame H.264 decode session against real hardware without any
`VkResult` failure — but the decoded picture content is not yet correct.
**This crate does not yet produce real, verified decoded video output** —
that is the honest, current state, not "H.264 decode works" — flagged here
per this workspace's own honesty rules rather than left for a reader to
discover from a failing test alone.

## Addendum (2026-07-30, later same day): root cause found and fixed — real decode verified

The all-zero/unchanged decode output described in the addendum above is
**fixed**. Root cause: two real bugs in this crate's own Vulkan Video command
construction, found by fetching and reading FFmpeg's actual, working
`libavcodec/vulkan_decode.c`/`vulkan_h264.c` field-by-field (the same rigor
the AV1-encode sibling ADR used) rather than continuing to guess blindly with
no validation layer available.

### Bug 1: the setup slot's `slotIndex` at `vkCmdBeginVideoCodingKHR` must be `-1`, not the real index

`session_command_h264.rs`'s `record_and_submit_h264` included the
about-to-be-written slot in `vkCmdBeginVideoCodingKHR`'s `pReferenceSlots`
with its **real** slot index (the same `VkVideoReferenceSlotInfoKHR` reused
for `pSetupReferenceSlot`). FFmpeg's `vulkan_decode.c` does the opposite: it
appends the current-frame slot to `decode_start.pReferenceSlots` as a
**copy** with `slotIndex` overwritten to `-1` —

```c
VkVideoReferenceSlotInfoKHR *cur_vk_ref =
    (void *)&decode_start.pReferenceSlots[decode_start.referenceSlotCount];
cur_vk_ref[0] = vp->ref_slot;
cur_vk_ref[0].slotIndex = -1;
decode_start.referenceSlotCount++;
```

`-1` marks the slot as "being introduced this scope, not yet an
active/established reference" — only `VkVideoDecodeInfoKHR::pSetupReferenceSlot`
(a separate, real-indexed copy) tells the driver which slot the decoded
picture actually lands in. Fixed: `begin_slots` now pushes a copy of
`setup_slot` with `slot_index` set to `-1`, while `setup_slot` itself (real
index) is still passed to `pSetupReferenceSlot` unchanged.

### Bug 2: the destination slot's layer needs `VIDEO_DECODE_DST_KHR` layout during the decode command, not `VIDEO_DECODE_DPB_KHR`

This crate's single shared `2D_ARRAY` DPB image was kept in
`VK_IMAGE_LAYOUT_VIDEO_DECODE_DPB_KHR` at all times outside of CPU readback —
never transitioned to `VK_IMAGE_LAYOUT_VIDEO_DECODE_DST_KHR` for the layer
actually being written. FFmpeg's `vulkan_decode.c` picks the layout
conditionally:

```c
.newLayout = (layered_dpb || vp->dpb_frame) ?
    VK_IMAGE_LAYOUT_VIDEO_DECODE_DST_KHR :
    VK_IMAGE_LAYOUT_VIDEO_DECODE_DPB_KHR,
```

— i.e. even in a layered/coincide DPB (this crate's exact design), the
*specific layer being decoded into this frame* needs `VIDEO_DECODE_DST_KHR`
during the decode command; only already-established reference layers stay in
`VIDEO_DECODE_DPB_KHR`. Fixed: `record_and_submit_h264` now barriers the
destination layer `VIDEO_DECODE_DPB_KHR` → `VIDEO_DECODE_DST_KHR`
(`VIDEO_DECODE_WRITE_KHR` access) immediately before
`vkCmdBeginVideoCodingKHR`, and back `VIDEO_DECODE_DST_KHR` →
`VIDEO_DECODE_DPB_KHR` immediately after `vkCmdEndVideoCodingKHR` — restoring
this crate's fixed steady-state layout so `cpu_readback.rs`'s own
`VIDEO_DECODE_DPB_KHR` → `TRANSFER_SRC_OPTIMAL` transition (and every future
picture's reference-slot barriers) keep their existing assumption valid.

### Bug 3 (the one that actually mattered most): the uploaded bitstream needs a real Annex-B start code, not just NAL header + payload at `slice_offsets[0] == 0`

This was the real fix. This crate uploaded `raw_nal` — the NAL header byte
plus emulation-prevented payload, with **no Annex-B start code** — to
`src_buffer`, with `slice_offsets = [0]` pointing directly at the NAL header.
FFmpeg's `ff_vk_decode_add_slice` does something different: it always
prepends a literal 3-byte start code before the NAL bytes in the same upload
buffer —

```c
static const uint8_t startcode_prefix[3] = { 0x0, 0x0, 0x1 };
const size_t startcode_len = add_startcode ? sizeof(startcode_prefix) : 0;
slice_off[nb] = vp->slices_size; // offset points AT the start code
memcpy(slices + vp->slices_size + startcode_len, data, size);
```

The hardware's own bitstream scanner apparently locates each slice by
scanning for the Annex-B start code from the declared offset, not by trusting
the offset to already point at a NAL header with no code to find — the
decode command executed with zero `VkResult` errors either way, but silently
found nothing to decode without a real start code present. Fixed:
`decoder.rs::decode_slice` now prepends `[0x00, 0x00, 0x01]` before `raw_nal`
in the uploaded buffer; `slice_offsets` stays `[0]` (now pointing at the
start code, matching FFmpeg's convention exactly).

### Verified fix

`cargo test -p mediaway-decoder-vulkan --test hardware_h264_decode --
--nocapture` now passes with **hard** `assert_eq!`/`assert_ne!` (no soft
skip): the hand-crafted IDR decodes to the exact literal `I_PCM` values
(200/50), and the P-frame's `P_Skip` macroblock exactly reproduces the IDR's
reference pixels (200) while its `I_PCM` macroblock is genuinely new content
(220, differing from the IDR's 50) — real motion-compensated DPB reference
read and real varying P-frame output, both hardware-verified on
the RTX 4090.

### Honest status (current)

Sans-io logic: unchanged from above (41 tests, real and hardware-independent).
**The Vulkan Video H.264 decode pipeline now genuinely works and is
hardware-verified**: `cargo check`/`cargo clippy --all-targets` are clean,
`cargo test -p mediaway-decoder-vulkan --lib` passes 43/43, and
`tests/hardware_h264_decode.rs` passes with hard pixel-value assertions
against a real decode-capable GPU — both the IDR and P-frame paths,
including real sliding-window DPB reference management and P-slice
motion-compensated prediction, are confirmed correct on real hardware. HEVC
and AV1 remain untouched (explicit follow-up, per this round's scope).

## Addendum (2026-07-30, later same day): HEVC (Stage 2) — sans-io real and tested, GPU decode still not verified

Added HEVC general-GOP decode support alongside the now-verified H.264 path,
per this ADR's own Stage 2 plan. New files: `hevc_params.rs` (VPS/SPS/PPS
parsing + `StdVideoH265*`/`StdVideoDecodeH265*` construction), `hevc_slice.rs`
(slice-segment-header + short-term RPS parsing — genuinely new logic, not a
rename of `h264_slice.rs`), `session_command_hevc.rs` (picture-info/command
recording, mirroring `session_command_h264.rs`'s now-verified shape),
`decoder_hevc.rs` (session build + per-picture decode dispatch). `session.rs`
gained a `DecodeProfile::Hevc` variant and `decoder.rs` now dispatches on
`config.codec`.

### Struct-binding confirmation (real compile check, not inference)

`vulkanalia` 0.35's vendored `vulkanalia-sys` source was read directly (not
assumed from the H.264 pattern): `StdVideoH265SequenceParameterSet`,
`StdVideoH265PictureParameterSet`, `StdVideoH265VideoParameterSet`,
`StdVideoH265ProfileTierLevel`, `StdVideoH265DecPicBufMgr`,
`StdVideoDecodeH265PictureInfo`, `StdVideoDecodeH265ReferenceInfo`, and their
`*Flags` bitfield accessor names all exist with the field lists this crate
uses. `KHR_VIDEO_DECODE_H265_EXTENSION` and
`VideoCodecOperationFlagsKHR::DECODE_H265` are present. This confirmation was
directly useful: it is what surfaced the two real bugs below (both are
`*Flags` fields this crate initially hardcoded to `0`/disabled instead of
reading the accessor list far enough to realize they existed and mattered).

### Real gaps vs. H.264's structure

- **NAL header**: HEVC's is 2 bytes (`nal_unit_type` 6 bits, `nuh_layer_id` 6
  bits, `nuh_temporal_id_plus1` 3 bits) vs. H.264's 1 byte — `HevcNalUnit`
  parses this fresh in `hevc_params.rs`, does **not** reuse
  `mediaway_sw::h264::NalUnit::parse`. Reuses `mediaway_sw::h264::{BitReader,
  split_annex_b}` only (both codec-agnostic).
- **Reference model**: HEVC has no `frame_num`/sliding-window; it uses
  POC-based short-term/long-term Reference Picture Sets (RPS), split into
  `RefPicSetStCurrBefore`/`RefPicSetStCurrAfter`/`RefPicSetLtCurr`.
  `hevc_slice.rs::ShortTermRefPicSet` parses `short_term_ref_pic_set(0)` (ITU-T
  H.265 § 7.3.7/§ 7.4.8) and computes `curr_before_after_poc` — real, new
  logic, sans-io tested (19 new tests across `hevc_params.rs`/`hevc_slice.rs`,
  hand-built bitstream fixtures, no GPU). Long-term references and SPS-level
  RPS lists (`num_short_term_ref_pic_sets > 0`) are rejected as `Unsupported`
  (sliding-window-equivalent scope cut, matching H.264's own).
- **Scope cuts, matching H.264's own precedent**: single-tile, no WPP
  (`tiles_enabled_flag`/`entropy_coding_sync_enabled_flag` rejected),
  `pcm_enabled_flag == 1` rejected (avoids needing to also thread PCM
  sample-depth fields through `StdVideoH265SequenceParameterSet`), B-slices
  rejected in `hevc_slice.rs`.
- **GPU decode path scope cut (new this round, beyond the plan)**:
  `decoder_hevc.rs::decode_slice_hevc` only reaches a real `vkCmdDecodeVideoKHR`
  call for **IDR pictures** — a P/B-slice HEVC NAL returns
  `DecodeError::Unsupported`. `hevc_slice.rs`'s own P-slice/RPS parsing is
  real and independently tested, but nothing in this round wires a parsed
  P-slice through to an actual decode call. This is an honest scope-down, not
  an oversight — see the hardware-test section below for why hand-constructing
  even an IDR-only bitstream already surfaced two real bugs, and P-slice
  general-GOP hardware verification needs those (and more) fully resolved
  first.

### Hardware test: real encoder round trip, not hand-crafted CABAC — and why

`hardware_h264_decode.rs` hand-constructs its bitstream because H.264's
`I_PCM` macroblock type (CAVLC/UE(v)-coded `mb_type`, then **raw, uncoded**
sample bytes) sidesteps entropy coding of pixel data entirely. HEVC has no
CAVLC mode and no equivalent escape — even a PCM coding unit's own `pcm_flag`
is itself CABAC-coded (ITU-T H.265 § 9.3), so hand-constructing *any* legal
HEVC picture, including the smallest possible IDR-only one, requires a
spec-exact binary arithmetic encoder (context initialization, state
transitions, range/offset renormalization, termination) — a substantially
larger, higher-risk undertaking than H.264's `I_PCM` escape, confirmed while
scoping this out (not just anticipated). Per this round's explicit granted
permission to scope down rather than skip the hardware test entirely,
`tests/hardware_hevc_decode.rs` instead:

1. Uses this workspace's own already hardware-verified
   `mediaway-encoder-vulkan::VulkanVideoEncoder` (HEVC) to encode one real
   flat-gray NV12 frame, producing a **real, driver-produced** Annex-B
   bitstream (VPS+SPS+PPS+IDR slice) — no hand-written CABAC.
2. Feeds those exact bytes into `VulkanVideoDecoder` (HEVC) and asserts real
   decoded NV12 output.

This is a genuinely real, non-trivial hardware exercise (two independently
hardware-verified crates chained together), and — because
`VulkanVideoEncoder` makes every pushed frame an independent key frame — it
exercises exactly the IDR-only path this round's decoder scope actually
implements, no further scope mismatch.

### Two real bugs found and fixed (confirmed via the same FFmpeg-source-comparison technique that found H.264's bugs), picture still not correct

Running the hardware test the first time reproduced H.264's original
symptom exactly: `vkCmdDecodeVideoKHR` completed with zero `VkResult` errors,
but the decoded picture read back **completely zero** (not just wrong —
every luma byte `0`, confirmed with an instrumented run: `nonzero_count=0/
49152`). Comparing this crate's session-parameters construction against
FFmpeg's real `libavcodec/vulkan_hevc.c` found two real, confirmed bugs, both
the same *shape*: a `Std*Flags` bitfield this crate parsed correctly off the
wire, then silently discarded and hardcoded to `0` when building the
`Std*` struct handed to the driver — which the driver trusts completely
instead of re-deriving from the raw bitstream bytes, so any such mismatch
desyncs its own CABAC parser from the very first syntax element it affects:

- **`HevcSps::to_std`** hardcoded every `StdVideoH265SpsFlags` bit to `0`,
  including `amp_enabled_flag` and `sample_adaptive_offset_enabled_flag` —
  the real encoder's SPS had **both set to `1`** (confirmed via an
  instrumented run printing the parsed `HevcSps`). `sample_adaptive_offset_enabled_flag`
  in particular gates `slice_sao_luma_flag`/`slice_sao_chroma_flag` in
  **every** slice header (any slice type, including IDR) — a real encoder
  enabling SAO by default (common) against a session-parameters SPS claiming
  SAO is off desyncs the slice-header parse before the first CTU even starts.
  Fixed: `HevcSps` now stores `amp_enabled_flag`,
  `sample_adaptive_offset_enabled_flag`, `sps_temporal_mvp_enabled_flag`,
  `strong_intra_smoothing_enabled_flag` and echoes all four into `to_std`'s
  flags.
- **`HevcPps::to_std`** hardcoded every `StdVideoH265PpsFlags` bit to `0` and
  `num_extra_slice_header_bits`/`diff_cu_qp_delta_depth`/`pps_cb_qp_offset`/
  `pps_cr_qp_offset` to `0` regardless of what was actually parsed —
  `transquant_bypass_enabled_flag` (a bit at the very start of **every**
  `coding_unit()`), `cu_qp_delta_enabled_flag`/`transform_skip_enabled_flag`
  (per-CU/per-TU bits), and `pps_slice_chroma_qp_offsets_present_flag`
  (slice-header bits) are exactly this same class of risk. Fixed: `HevcPps`
  now stores and correctly echoes all of these (this round's real stream
  happened to have all of them `false`, so this fix did not by itself change
  this specific test's outcome — but it is a real, necessary correctness fix
  independent of that, confirmed by reading the real encoder's own PPS
  construction convention).
- (Also fixed in passing: `decoder_hevc.rs` was passing
  `session.sps.sps_seq_parameter_set_id` as `StdVideoDecodeH265PictureInfo::pps_seq_parameter_set_id`
  instead of `session.pps.pps_seq_parameter_set_id` — harmless in a
  single-SPS stream where the two are equal by construction, but wrong in
  general; corrected to match FFmpeg's own `pps->sps_id` convention.)

**Both fixes are real and confirmed necessary** (found by reading the actual
generated bitfield accessor list in `vulkanalia-sys`'s vendored source, not
guessed), but **the picture still decodes all-zero after both** — the
remaining root cause is not yet identified. Command-sequence-level comparison
(reference-slot `slotIndex = -1` activation, `VIDEO_DECODE_DST_KHR` layout
transition, real Annex-B start code, `pReferenceSlots`/barrier ordering) was
re-checked line-by-line against FFmpeg's `ff_vk_decode_frame` and matches
exactly — confirmed no HEVC-specific branch exists in FFmpeg's own shared
decode-command function, so this crate's H.264-mirrored command sequence is
not the (or at least not the only) remaining gap.

**Ruled out**: wrong reference-slot protocol (matches FFmpeg exactly, and
matches this crate's own verified H.264 path); missing Annex-B start code
(present, confirmed via instrumented byte dump); wrong image layout sequence
(codec-generic, shared with verified H.264 path); readback/synchronization bug
(codec-generic `cpu_readback.rs`, byte-identical code path already proven
correct by the H.264 test); wrong coded extent/DPB slot count (driver accepted
`capabilities.validate_requested_extent` without error, `dpb_slot_count`
computed the same way as the verified H.264 path).

**Not yet tried / open hypotheses** for whoever picks this up: a validation
layer is still not available on the test machine (same
constraint recorded in the earlier H.264 addendum), so there is no way to get
driver diagnostic messages instead of blind hypothesis-testing; the signaled
`general_level_idc` (`30`, i.e. Level 1.0) is arguably too low for this test's
256x192 picture per the HEVC spec's own level limits — worth checking whether
this specific driver silently no-ops decode when the signaled level is
insufficient for the actual picture size, by locking the level to something
clearly sufficient (e.g. Level 3.1) and re-running; the encoder's real
`StdVideoH265ProfileTierLevel`/general-constraint-flag bits are all
hardcoded in this crate's own `to_std_profile_tier_level` rather than parsed
from the real SPS — unlikely to affect CTU-level parsing (informational,
not slice/CTU syntax) but not fully ruled out.

### Honest status (current)

Sans-io HEVC logic (`hevc_params.rs`/`hevc_slice.rs`) is real,
hardware-independent, and fully unit-tested (19 new tests, 62 total for this
crate's `--lib` suite). `cargo check`/`cargo clippy -p mediaway-decoder-vulkan
--all-targets` are clean. **HEVC GPU decode is not yet verified on real
hardware** — `tests/hardware_hevc_decode.rs` soft-skips with a loud
`eprintln!` (per this workspace's "hardware-gated tests never hard-fail the
default suite for a real, not-yet-root-caused bug" convention, the same one
this crate's own H.264 test followed before its bugs were found) rather than
asserting on decoded pixel values. `cargo test -p mediaway-decoder-vulkan`
is green (62 lib + 2 integration tests, one hard-passing H.264, one
soft-skipping HEVC). Flip `hardware_hevc_decode.rs`'s check back to a hard
assertion once the remaining bug is found. AV1 remains untouched.
