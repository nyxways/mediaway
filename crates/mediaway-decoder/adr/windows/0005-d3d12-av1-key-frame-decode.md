# ADR-0005: D3D12 native AV1 decode — `KEY_FRAME`-only, Main profile

- **Status**: Proposed
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder-windows`

## ⚠️ Read this before touching any hardware

Same constraint as [ADR-0004](0004-d3d12-hevc-single-forward-ref-p-slice-decode.md), restated in
full because it is the single most important fact governing this ADR too, not a boilerplate
repeat:

1. **This ADR is a design-only pass — no implementation code is written here.**
2. **`mediaway-decoder-windows`'s existing D3D12 H.264 decode path has never once produced a
   correct decode on real hardware and has caused 8 confirmed `DXGI_ERROR_DEVICE_HUNG` GPU hangs
   (TDRs)**, most recently 2026-08-07 (ADR-0002's own addenda). Root cause remains
   **unresolved**. Every low-level D3D12 mechanism this AV1 ADR reuses unchanged — `setup.rs`'s
   decoder/heap/DPB-texture-array creation, `ops.rs`'s `DecodeFrame1` submission/barrier sequence
   shape (a **new, parallel** `av1_ops.rs` copies this exact shape, retyped — see § Decision) —
   is exactly the machinery that is currently unproven for H.264 **and** still unrun for HEVC
   (ADR-0004, same branch, sans-io-only). Treat "the shared D3D12 submission plumbing might
   itself be broken, independent of any codec-specific bug" as a live, real hypothesis.
2. **Do not run the existing H.264 D3D12 decode hardware test** (`d3d12_video_decode_tests.rs`)
   or **the HEVC hardware test** (`d3d12_video_decode_hevc_tests.rs`) as any part of implementing
   or verifying this ADR — both are real, disruptive GPU-hang risks on this workspace's dev
   machine.
3. **This ADR's own future implementation must not run its new AV1 hardware-gated integration
   test either** (see § Test plan) — same TDR risk, compounded further here: unlike HEVC (which
   at least has a real, hardware-verified-for-IDR bitstream-parsing precedent to port), this
   ADR's parsing logic is new, from-scratch work grounded only in spec text and a same-crate
   **encoder's** static field choices (see § Context), and — a genuinely new risk this ADR's
   sibling ADRs did not have — **the one realistic same-workspace source of a real AV1 test
   bitstream (`mediaway-encoder-windows`'s own D3D12 AV1 encoder) is not confirmed to produce a
   spec-legal, decodable stream at all** (§ Context, § Open questions #1). Compile, `clippy`, and
   sans-io unit tests only. Verification here means "does this design's static shape and every
   hand-defined `repr(C)` struct layout hold up against an independent, citable primary source" —
   not "does it decode a real frame."

## Context

`mediaway-decoder-windows`'s `d3d12_video_decode` module (`src/windows/d3d12_video_decode.rs` +
siblings) is H.264-only in its registered/public shape, plus a parallel, **sans-io-verified-only,
zero-hardware-run** HEVC decode path that just landed this session on the same branch
(`feat/d3d12-decode-hevc-av1`, uncommitted to `main`; see ADR-0004). The module is self-contained
and **unregistered** (`mod d3d12_video_decode;`, not `pub mod`, per `src/lib.rs`).

ADR-0002 (the module's founding ADR) already named AV1 as this module's second follow-up after
HEVC — confirmed independently this session: `docs/standards/registry.toml`'s own
`av1-bitstream-spec` entry (id `av1-bitstream-spec`, already cached at
`local/standards/av1-bitstream-spec/av1-spec.pdf`) states its purpose as *"OBU/sequence-header/
frame-header/tile syntax ground truth for mediaway-decoder-windows's planned D3D12 AV1 decode
(ADR-0002)"* — this ADR is that planned follow-up, not a speculative new scope.

### Correcting the task's premise: no Vulkan or VA-API AV1 **decode** exists on this branch

The originating task described real, hardware-verified Vulkan AV1 decode
(`crates/mediaway-decoder/src/vulkan/{av1_params,av1_params/av1_frame_header,av1_refs,
decoder_av1,session_command_av1}.rs`) and VA-API AV1 decode
(`crates/mediaway-decoder/src/linux/vaapi/av1*.rs`), both flagged as possibly living on other,
unmerged branches. **Checked directly this session, on `feat/d3d12-decode-hevc-av1`**: neither
path exists — `Glob` for both patterns returned zero matches. Per the task's own fallback
instruction, this ADR uses the **encode**-side sources instead:

- **`mediaway-encoder-windows`'s D3D12 AV1 encoder is real and hardware-verified**
  (`src/windows/d3d12_video_encode/{av1.rs,ops_av1.rs,bitstream_av1.rs}`, `gop.rs`/`gop_hevc.rs`
  exist but **no `gop_av1.rs`** — confirmed by `Glob`, i.e. the encoder's own AV1 path has no GOP/
  P-frame state machine at all, it is genuinely all-intra/`KEY_FRAME`-only end to end). This is
  the single most valuable asset this ADR has: real, driver-accepted D3D12 AV1
  profile-negotiation/feature-query code, and a complete, spec-cited OBU sequence-header +
  frame-header **writer** whose every field-inference comment doubles as a map of what a
  **reader** must handle for the same all-intra shape.
- **`mediaway-sw::av1`** (`crates/mediaway-sw/src/av1.rs`) exists but is **not** a bitstream/OBU
  parser — it is a thin sans-io wrapper around `rav1e` (a real, BSD-2-Clause, pure-Rust AV1
  *encoder*, already a dependency in this workspace). Checked directly to avoid a plausible false
  lead: it has zero OBU-header/leb128/sequence-header parsing logic to port from. Flagged again in
  § Alternatives Considered so a future implementer does not waste time investigating it as a
  decode-parsing source. It **is**, however, a real candidate for producing a legal AV1 test
  bitstream if the D3D12 encoder's own output proves undecodable — see § Open questions #1 and
  § Test plan.

### Real, hardware-verified D3D12 AV1 encode findings this ADR carries forward

Two real, hardware-confirmed findings from `mediaway-encoder-windows`'s AV1 encoder (its own
module doc, `d3d12_video_encode/av1.rs:14-47`, this session's own project memory,
2026-08-06/07):

1. **`D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` always reports `CODEC_NOT_SUPPORTED` for AV1** — the
   real query is `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT1`. The **decode**-side equivalent this
   module already uses, `D3D12_FEATURE_VIDEO_DECODE_SUPPORT` (`setup::check_decode_support`,
   reused unchanged by H.264 and HEVC), has **no** `_SUPPORT1`/second-generation sibling anywhere
   in the pinned `windows` crate (checked: no `D3D12_FEATURE_VIDEO_DECODE_SUPPORT1` symbol exists)
   — so this specific encode-side trap has no decode-side analog to fall into. Not proof decode
   has no AV1-specific quirk of its own (§ Open questions #2), but this particular one is ruled
   out structurally.
2. **The driver (this workspace's reference RTX 4090) mandates `AUTO_SEGMENTATION |
   CDEF_FILTERING | LOOP_RESTORATION_FILTER` be declared *available* at the encode session level**
   (`D3D12_VIDEO_ENCODER_AV1_CODEC_CONFIGURATION::FeatureFlags`) even though the encoder's actual
   per-frame bitstream disables all three (`bitstream_av1.rs::write_sequence_header`:
   `enable_cdef`/`enable_restoration` both hardcoded `0`, `av1.rs`'s module doc explains this is a
   **session-level capability declaration**, independent of per-frame use). **Decode has no
   directly analogous "declare tool support" struct** (`D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT`
   has no `CodecConfiguration`-shaped input the way encode's `SUPPORT1` does) — this finding does
   not obviously transfer, but is cited here because it is the clearest existing evidence that
   this exact driver's AV1 pipeline has real, non-obvious mandatory-declaration quirks that only
   surfaced via `ID3D12InfoQueue` debug-layer messages, not documentation. Expect the unrun AV1
   decode path to have its own such surprises.
3. **Real, load-bearing, previously undiscovered-until-this-citation risk**:
   `docs/standards/registry.toml`'s `av1-bitstream-spec` entry states outright — *"for diffing
   mediaway-encoder-windows's D3D12 AV1 encode output (currently EncodeFrame succeeds but the
   bitstream is not decodable by libdav1d)"*. **This crate's own D3D12 AV1 encoder's output is
   not confirmed to be a spec-legal, decodable AV1 bitstream.** Every sibling ADR in this
   workspace that used "chain this crate's own hardware-verified encoder into the decoder under
   test" as its hardware-test technique (HEVC's `gop_hevc.rs`, H.264's WMF encoder) had no reason
   to doubt the encoder's output was legal. This one does, on record, in this exact repository's
   own standards registry. See § Open questions #1 — this is this ADR's single largest,
   AV1-specific new risk, not inherited from any sibling ADR.
4. **`AV1_INVALID_DPB_RESOURCE_INDEX = 0xFF`** (`ops_av1.rs:71`) and the real debug-layer message
   this backend's encoder hit before fixing its zeroed-`Default` reference descriptors — *"AV1
   Picture control structure - Key Frames must not use any references"* (`ops_av1.rs:69-70`) —
   is directly load-bearing for this ADR's own scope: `DXVA_PicParams_AV1`'s
   `RefFrameMapTextureIndex[8]` and `DXVA_PicEntry_AV1::Index` use the **identical** `0xFF`
   "invalid/unused" convention (Microsoft Learn's own field docs, § DXVA struct definitions) —
   independent confirmation, from two unrelated D3D12 AV1 API surfaces on the same driver, that
   `0xFF` is the real sentinel this ADR's own all-empty reference state must use.

## Scope decision: `KEY_FRAME`-only, Main profile, 8-bit 4:2:0, single-tile

> **Every decoded picture must be an independent AV1 `KEY_FRAME`** (`frame_type == 0`,
> `show_frame == 1`, `show_existing_frame == 0`) — no `INTER_FRAME`/`INTRA_ONLY_FRAME`/`S_FRAME`,
> no reference-frame use of any kind (`primary_ref_frame` always spec-inferred
> `PRIMARY_REF_NONE == 7`). **Main profile** (`seq_profile == 0`), **8-bit**
> (`high_bitdepth == 0`), **4:2:0** (`subsampling_x == subsampling_y == 1`, `mono_chrome == 0`,
> matches NV12), **single tile** (`tiles.cols == tiles.rows == 1`), `reduced_still_picture_header
> == 0` (a real multi-`KEY_FRAME` stream, e.g. an all-intra GOP — not AV1's single-frame "still
> picture" mode, matching the encoder's own choice).

Explicitly **out of scope, rejected as `DecodeError::Unsupported`, not silently mishandled**:
`INTER_FRAME`/`INTRA_ONLY_FRAME`/`S_FRAME`; `show_existing_frame == 1`; any non-Main profile;
non-8-bit / non-4:2:0 / monochrome; `segmentation_enabled == 1`; `film_grain_params_present ==
1`; `enable_superres == 1` (rejected at the sequence-header level, before any frame-level
`use_superres` could ever be reached — the earliest, most honest cut, mirroring HEVC's
reject-at-SPS/PPS-time convention); `enable_cdef == 1`; `enable_restoration == 1`;
`allow_screen_content_tools == 1`/`allow_intrabc == 1`; `using_qmatrix == 1`; more than one tile
(`tiles.cols > 1 || tiles.rows > 1`); `reduced_still_picture_header == 1`.

### Why this scope, not a smaller or larger one

1. **AV1 has no separate "IDR" concept — `KEY_FRAME` already *is* the smallest non-trivial cut**,
   unlike HEVC (where ADR-0004 chose single-forward-reference P-slice as a deliberately larger
   cut than IDR-only). There is no smaller useful alternative below this one except AV1's
   single-frame "still picture" mode (rejected, see § Alternatives Considered) — this ADR's scope
   is already the AV1 analog of H.264's original IDR-only ADR-0002 stage / HEVC's rejected
   "IDR-only" alternative, not a P-slice-equivalent increment.
2. **This crate's own D3D12 AV1 *encoder* has never produced anything beyond `KEY_FRAME`-only,
   all-intra output** (no `gop_av1.rs` exists at all, confirmed by `Glob` — contrast HEVC's real
   `gop_hevc.rs`) — there is no same-workspace GPU-verified AV1 reference-frame precedent on
   *either* side of the codec (encode or decode), on any backend, making a `KEY_FRAME`-only decode
   scope the natural mirror of the only AV1 shape this workspace has ever gotten a driver to
   accept.
3. **AV1's reference model is structurally novel relative to H.264/HEVC's DPB-list model** — a
   persistent 8-slot virtual reference-frame array (`ref_frame_idx[]`, addressed via
   `RefFrameMapTextureIndex[8]` + per-slot global-motion parameters in `DXVA_PicEntry_AV1`), not
   an active/inactive list built fresh per picture. Taking on that model's real complexity
   (`frame_refs[7]` population, global-motion `wmmat[6]` tracking, `order_hint`-based reference
   selection) on top of an entirely unverified decode path, with zero same-crate precedent for
   *any* part of it, is a materially larger risk than HEVC's P-slice increment was (which at
   least reused a real, hardware-verified-for-IDR RPS/POC parser). `KEY_FRAME`-only sidesteps
   this reference model **entirely** — see § Decision, no `av1_refs.rs`-equivalent module is even
   needed.
4. **Rejecting segmentation/CDEF/restoration/film-grain/superres/screen-content/qmatrix is a
   deliberate, honest interop narrowing, not incidental laziness**: this scope decodes exactly
   the class of stream this workspace's own AV1 encoder is capable of producing (every one of
   those tools hardcoded off in `bitstream_av1.rs`), which is also the only realistically
   obtainable same-workspace test source. A real caveat this narrowing creates: many real-world
   AV1 encoders (libaom, SVT-AV1) enable CDEF/restoration by default even in all-intra mode, so
   this decoder's practical interop is narrower than "any conformant Main-profile intra AV1
   stream" — see § Open questions #8, an explicit, acknowledged trade-off.

## Decision

> Add AV1 support to `mediaway-decoder-windows`'s D3D12 native decode module as **new files
> only**, reusing `dpb.rs`/`setup.rs`/`util.rs` **unchanged** (same codec-generic reuse ADR-0004
> already established for HEVC) and touching `d3d12_video_decode.rs` with **exactly one
> additive change**: new `mod` declarations + a re-export for the new AV1 types. Zero edits to
> any existing H.264 or HEVC type, function body, or struct layout.

### Reuse vs. new

- **`dpb.rs`/`setup.rs`/`util.rs`**: reused as-is, same reasoning ADR-0004 already gave (both
  already generic over the per-codec reference-metadata type `M: Copy` / take profile `GUID`
  parameters rather than hardcoding one codec).
- **`mediaway_sw::h264::BitReader`**: its `read_bit`/`read_bits` (MSB-first fixed-width reads,
  `bitreader.rs:34-53`) are pure bit-level mechanics with no H.264-specific behavior — reused
  directly for AV1's `f(n)` fields, same precedent ADR-0004's `hevc_vps_sps_pps.rs` already set
  for HEVC. **Not reused**: `read_ue`/`read_se` (`bitreader.rs:62-93`) — H.264's Exp-Golomb
  codes, structurally unrelated to AV1's own `uvlc()`/`leb128()`/`su(n)` variable-length codes
  (AV1 spec §4.10.3-§4.10.6), which this ADR's new `av1_obu.rs`/`av1_sequence_header.rs`/
  `av1_frame_header.rs` implement from scratch, grounded in the cached
  `local/standards/av1-bitstream-spec/av1-spec.pdf`. One real, minor wart inherited from this
  reuse: `BitReader::read_bit`/`read_bits` return `mediaway_sw::h264::error::H264Error`, an
  H.264-branded error type, for AV1 bit-reads — cosmetic, mapped to `DecodeError` at this
  module's own boundary exactly like every other reused-`BitReader` call site already does, not
  a new pattern.
- **`mediaway_sw::h264::split_annex_b` is NOT reused** — AV1 does not use Annex-B start-code
  framing at all. Packets are a sequence of length-prefixed OBUs (`leb128`-coded `obu_size`, no
  emulation-prevention bytes, AV1 spec §5.2/§5.3). This ADR's new `av1_obu.rs::split_obus` is the
  read-side mirror of `mediaway-encoder-windows`'s own `bitstream_av1.rs::{obu_header_byte,
  write_leb128, wrap_obu}` (`bitstream_av1.rs:31-65`) — same real spec syntax, reversed direction,
  a genuine "port the shape, not the code" opportunity even though the source is a writer.
- **`ops.rs`/`hevc_ops.rs` are not reused as-is** — same reasoning as ADR-0004's own "why a
  parallel `hevc_ops.rs`" section (§ below reuses that reasoning verbatim for AV1, not
  re-litigated).
- **No `av1_refs.rs`-equivalent module** — this ADR's largest structural simplification versus
  ADR-0004: since every decoded picture is an independent, reference-free `KEY_FRAME`, DPB
  "eviction" collapses to "evict every currently-held reference before this picture" (the same
  branch H.264's/HEVC's own IDR case already takes), and `DXVA_PicParams_AV1`'s
  `frame_refs[7]`/`RefFrameMapTextureIndex[8]` are always the trivial all-`0xFF`/empty state
  (§ Context finding #4). No RPS/POC/sliding-window logic of any kind is needed.

### File layout plan (design only — no file below exists yet)

```text
src/windows/d3d12_video_decode.rs   # EXISTING FILE — only change: add
                                     #   mod av1; mod av1_obu; mod av1_sequence_header;
                                     #   mod av1_frame_header; mod av1_pic_params;
                                     #   mod av1_decoder;
                                     # + pub(crate) use av1_decoder::{
                                     #     D3d12VideoDecoderAv1, DecodedFrameAv1, DecodedOutputAv1};
                                     # + #[cfg(test)] #[path = "d3d12_video_decode_av1_tests.rs"]
                                     #   mod av1_hardware_tests;
                                     # Zero edits to any existing H.264/HEVC type/fn/struct.

src/windows/d3d12_video_decode/
  av1.rs                 # NEW — open-time feature query + decoder/heap creation for AV1.
                          # Mirrors h264.rs/hevc.rs field-for-field: calls
                          # setup::check_decode_support/setup::create_decoder with
                          # D3D12_VIDEO_DECODE_PROFILE_AV1_PROFILE0 / DXGI_FORMAT_NV12
                          # (the windows crate has this GUID — confirmed present, see
                          # § windows crate survey — Main profile 8-bit, *not*
                          # `_12BIT_PROFILE2`/`_PROFILE1`/`_PROFILE2`). ~40 lines, no new logic.

  av1_obu.rs              # NEW — leb128()/obu_header() read-side (AV1 spec §4.10.5/§5.3.2),
                          # split_obus() (packet -> Vec<(obu_type, payload)>). Read-side mirror
                          # of mediaway-encoder-windows's bitstream_av1.rs write-side functions
                          # (same spec sections, reversed direction — see § Reuse).

  av1_sequence_header.rs  # NEW — sequence_header_obu() + color_config() parsing (AV1 spec
                          # §5.5.1/§5.5.2) into a local SequenceHeader struct. Field-by-field
                          # cross-checked against bitstream_av1.rs::write_sequence_header's own
                          # exhaustive inference-rule comments (encoder is a writer, but every
                          # "not read because X" comment there names exactly which reader-side
                          # branch this module must take for the same all-fixed shape).
                          # Rejects (Unsupported): seq_profile != 0, high_bitdepth == 1,
                          # mono_chrome == 1, subsampling != (1,1), reduced_still_picture_header
                          # == 1, enable_cdef == 1, enable_restoration == 1, enable_superres == 1
                          # (this ADR's earliest, honest rejection point — before any frame-level
                          # use_superres could ever be reached).

  av1_frame_header.rs     # NEW — uncompressed_header() (AV1 spec §5.9.2) + tile_info() (§5.9.15,
                          # same tile_log2()/uniform_tile_spacing computation shape
                          # bitstream_av1.rs::write_tile_info already demonstrates, ported
                          # read-side) + quantization_params()/segmentation_params()/
                          # loop_filter_params()/cdef_params()/lr_params()/read_tx_mode()/
                          # frame_reference_mode()/skip_mode_params()/global_motion_params()/
                          # film_grain_params() — every one of the latter group is a spec-mandated
                          # zero-bit no-op for this scope's FrameIsIntra/all-tools-disabled case,
                          # documented (not omitted) the same way bitstream_av1.rs::
                          # write_frame_header documents each one on its own write side.
                          # Rejects (Unsupported): frame_type != KEY_FRAME(0), show_existing_frame
                          # == 1, segmentation_enabled == 1, using_qmatrix == 1, tiles.cols > 1,
                          # tiles.rows > 1.

  av1_pic_params.rs       # NEW — hand-defined repr(C) DxvaPicEntryAv1/DxvaTileAv1/
                          # DxvaPicParamsAv1 (§ DXVA struct definitions below) + build_pic_params/
                          # build_tile. frame_refs[7]/RefFrameMapTextureIndex[8] are always
                          # DxvaPicEntryAv1::UNUSED (0xFF) — no reference-list construction of any
                          # kind (§ Decision's "no av1_refs.rs" note). Exactly one DxvaTileAv1
                          # entry per picture (this scope's single-tile cut).

  av1_decoder.rs          # NEW — SessionAv1 (mirrors SessionHevc: same D3D12 object fields,
                          # dpb: DpbPool<Av1RefMeta>, though Av1RefMeta carries no real payload —
                          # a unit-like marker, since no picture in this scope is ever referenced,
                          # kept as a real type rather than DpbPool<()> to match the existing
                          # generic's own Copy bound and stay consistent with a future inter-frame
                          # follow-up's likely field additions) + D3d12VideoDecoderAv1 (mirrors
                          # D3d12VideoDecoderHevc's open/ensure_session_ready/push_packet/
                          # decode_picture/poll_frame/flush/release_output shape, retyped: push_packet
                          # splits OBUs via av1_obu::split_obus instead of Annex-B NALs) +
                          # DecodedFrameAv1/DecodedOutputAv1 (same shape as DecodedOutputHevc,
                          # separate small type per ADR-0004's own "not worth the coupling"
                          # precedent).

  av1_ops.rs              # NEW, parallel to ops.rs/hevc_ops.rs (§ Reuse) — decode_frame_av1
                          # (write_bitstream / DecodeFrame1 / barrier sequence shape, retyped for
                          # DxvaPicParamsAv1/DxvaTileAv1; ReferenceFrames always the empty-NumTexture2Ds
                          # branch ops.rs already has, since this scope never has a real reference;
                          # **no INVERSE_QUANTIZATION_MATRIX frame argument at all** — AV1 has no
                          # separate qmatrix blob the way H.264/HEVC do, qm_y/qm_u/qm_v are plain
                          # scalar fields inside DxvaPicParamsAv1.quantization itself, a real
                          # structural difference from both sibling codecs, not an oversight) +
                          # readback_dpb_slot_to_cpu_av1 (near-byte-identical copy of
                          # ops::readback_dpb_slot_to_cpu, same NV12 two-plane readback shape).

  av1_obu_tests.rs / av1_sequence_header_tests.rs / av1_frame_header_tests.rs /
  av1_pic_params_tests.rs   # sibling *_tests.rs per this workspace's convention — all pure
                          # sans-io, fully writable and runnable this pass without any hardware.

src/windows/d3d12_video_decode_av1_tests.rs   # NEW top-level hardware-gated integration test,
                          # mirroring d3d12_video_decode_hevc_tests.rs's soft-skip pattern.
                          # Written this pass (or in this ADR's eventual implementation pass);
                          # MUST NOT be run — see § Test plan, doubly cautioned given § Open
                          # questions #1.
```

Every new file is planned to stay under this workspace's 1000-line-per-source rule.

## `windows` crate binding survey (real compile-adjacent check)

Checked the vendored `windows-0.62.2` source directly (same crate version ADR-0002/ADR-0004
confirmed):

- **`D3D12_VIDEO_DECODE_PROFILE_AV1_PROFILE0`/`_PROFILE1`/`_PROFILE2`/`_12BIT_PROFILE2`/
  `_12BIT_PROFILE2_420` GUID constants are present**
  (`Win32/Media/MediaFoundation/mod.rs:3771-3775`) — this ADR uses `_PROFILE0` (Main, 8/10-bit
  4:2:0; this scope further restricts to 8-bit only, see § Scope decision).
- **`DXVA_PicParams_AV1`, `DXVA_PicEntry_AV1`, `DXVA_Tile_AV1` are absent from the crate's
  generated bindings entirely** — grepped the full vendored source tree, zero matches beyond the
  profile GUIDs above. Same situation ADR-0002 (H.264) and ADR-0004 (HEVC) already found for
  their own codecs — the D3D12 decode *plumbing* is present and reused as-is; the DXVA-shaped
  per-codec structs must be hand-defined, `repr(C)`, ground-truthed against a real source (this
  ADR uses Microsoft's own official driver DDI reference, a **primary** source — see next
  section, a stronger footing than ADR-0004 had for HEVC, whose Wine mirror carried a known,
  unresolved discrepancy against Microsoft Learn).
- **`D3D12_VIDEO_DECODE_ARGUMENT_TYPE` has no dedicated "tile control" variant** — only
  `PICTURE_PARAMETERS`(0)/`INVERSE_QUANTIZATION_MATRIX`(1)/`SLICE_CONTROL`(2)/`HUFFMAN_TABLE`(3)
  exist (`mod.rs:3538-3542`). `DXVA_Tile_AV1[]` is the payload carried under the same
  `SLICE_CONTROL` argument-type slot H.264/HEVC use for their own per-slice control arrays —
  confirmed by cross-referencing Microsoft's `DXVA_Tile_AV1` page (below) against this enum; no
  AV1-specific argument type exists, "slice control" is evidently the generic name for "one
  entry per coded fragment," not H.264/HEVC-specific.
- **`D3D12_FEATURE_VIDEO_DECODE_SUPPORT` has no `_SUPPORT1` sibling** — unlike encode's real
  `SUPPORT`→`SUPPORT1` AV1 trap (§ Context finding #1), grepped and confirmed absent; this
  specific class of encode-side bug structurally cannot recur on the decode side.

## DXVA struct definitions (ground truth, cited — primary source)

Fetched directly this session from Microsoft's own official Windows Driver DDI reference
(`learn.microsoft.com/en-us/windows-hardware/drivers/ddi/dxva/...`, header `dxva.h`, "Minimum
supported server: Windows Server 2022" — i.e. these are genuinely new-enough structs that the
Wine mirror ADR-0004 used for HEVC may well not even have an AV1 entry yet; this ADR did not
separately check Wine's `dxva.h` for AV1 given this primary source already exists). FFmpeg is
independently known to gate its own DXVA AV1 hwaccel on the presence of this exact symbol
(per this session's own web search of `libavcodec/d3d12va_decode.c`'s ecosystem) — corroborating
this struct is the real, currently-shipping API surface, not a stale/abandoned one.

```c
/* learn.microsoft.com/.../ns-dxva-dxva_picentry_av1 — fetched this session */
typedef struct _DXVA_PicEntry_AV1 {
  UINT   width;
  UINT   height;
  INT    wmmat[6];
  union {
    struct {
      UCHAR wminvalid : 1;
      UCHAR wmtype : 2;
      UCHAR Reserved : 5;
    };
    UCHAR GlobalMotionFlags;
  } DUMMYUNIONNAME;
  UCHAR  Index;            // index into RefFrameMapTextureIndex[]; 0xFF = invalid/unused
  UINT16 Reserved16Bits;
} DXVA_PicEntry_AV1, *LPDXVA_PicEntry_AV1;

/* learn.microsoft.com/.../ns-dxva-dxva_tile_av1 — fetched this session */
typedef struct _DXVA_Tile_AV1 {
  UINT   DataOffset;
  UINT   DataSize;
  USHORT row;
  USHORT column;
  UINT16 Reserved16Bits;
  UCHAR  anchor_frame;     // 0xFF when not part of a Tile List OBU (this scope, always)
  UCHAR  Reserved8Bits;
} DXVA_Tile_AV1, *LPDXVA_Tile_AV1;
```

`DXVA_PicParams_AV1` (Microsoft Learn, `learn.microsoft.com/.../ns-dxva-dxva_picparams_av1`,
fetched this session — reproduced verbatim, this ADR's own field-group naming below mirrors it
1:1 rather than renaming for brevity, same "ground-truth over readability" convention
`hevc_pic_params.rs` already uses for `ucScalingLists0-3`):

```c
typedef struct _DXVA_PicParams_AV1 {
  UINT              width;
  UINT              height;
  UINT              max_width;
  UINT              max_height;
  UCHAR             CurrPicTextureIndex;
  UCHAR             superres_denom;
  UCHAR             bitdepth;
  UCHAR             seq_profile;
  struct {
    UCHAR  cols;
    UCHAR  rows;
    USHORT context_update_id;
    USHORT widths[64];
    USHORT heights[64];
  } tiles;
  union { struct {
    UINT use_128x128_superblock : 1;
    UINT intra_edge_filter : 1;
    UINT interintra_compound : 1;
    UINT masked_compound : 1;
    UINT warped_motion : 1;
    UINT dual_filter : 1;
    UINT jnt_comp : 1;
    UINT screen_content_tools : 1;
    UINT integer_mv : 1;
    UINT cdef : 1;
    UINT restoration : 1;
    UINT film_grain : 1;
    UINT intrabc : 1;
    UINT high_precision_mv : 1;
    UINT switchable_motion_mode : 1;
    UINT filter_intra : 1;
    UINT disable_frame_end_update_cdf : 1;
    UINT disable_cdf_update : 1;
    UINT reference_mode : 1;
    UINT skip_mode : 1;
    UINT reduced_tx_set : 1;
    UINT superres : 1;
    UINT tx_mode : 2;
    UINT use_ref_frame_mvs : 1;
    UINT enable_ref_frame_mvs : 1;
    UINT reference_frame_update : 1;
    UINT Reserved : 5;
  }; UINT32 CodingParamToolFlags; } coding;
  union { struct {
    UCHAR frame_type : 2;
    UCHAR show_frame : 1;
    UCHAR showable_frame : 1;
    UCHAR subsampling_x : 1;
    UCHAR subsampling_y : 1;
    UCHAR mono_chrome : 1;
    UCHAR Reserved : 1;
  }; UCHAR FormatAndPictureInfoFlags; } format;
  UCHAR             primary_ref_frame;
  UCHAR             order_hint;
  UCHAR             order_hint_bits;
  DXVA_PicEntry_AV1 frame_refs[7];
  UCHAR             RefFrameMapTextureIndex[8];   // 0xFF = unused entry
  struct {
    UCHAR  filter_level[2];
    UCHAR  filter_level_u;
    UCHAR  filter_level_v;
    UCHAR  sharpness_level;
    union { struct {
      UCHAR mode_ref_delta_enabled : 1;
      UCHAR mode_ref_delta_update : 1;
      UCHAR delta_lf_multi : 1;
      UCHAR delta_lf_present : 1;
      UCHAR Reserved : 4;
    }; UCHAR ControlFlags; } DUMMYUNIONNAME;
    CHAR   ref_deltas[8];
    CHAR   mode_deltas[2];
    UCHAR  delta_lf_res;
    UCHAR  frame_restoration_type[3];
    USHORT log2_restoration_unit_size[3];
    UINT16 Reserved16Bits;
  } loop_filter;
  struct {
    union { struct {
      UCHAR delta_q_present : 1;
      UCHAR delta_q_res : 2;
      UCHAR Reserved : 5;
    }; UCHAR ControlFlags; } DUMMYUNIONNAME;
    UCHAR  base_qindex;
    CHAR   y_dc_delta_q;
    CHAR   u_dc_delta_q;
    CHAR   v_dc_delta_q;
    CHAR   u_ac_delta_q;
    CHAR   v_ac_delta_q;
    UCHAR  qm_y;    // 0xFF when using_qmatrix == 0 (this scope, always — rejected if 1)
    UCHAR  qm_u;
    UCHAR  qm_v;
    UINT16 Reserved16Bits;
  } quantization;
  struct {
    union { struct {
      UCHAR damping : 2;
      UCHAR bits : 2;
      UCHAR Reserved : 4;
    }; UCHAR ControlFlags; } DUMMYUNIONNAME;
    /* per-strength primary:6/secondary:2 packed bytes, y_strengths[8]/uv_strengths[8] */
  } cdef;   // enable_cdef rejected at sequence-header time (this scope) — always zeroed/unused
  UCHAR             interp_filter;
  struct {
    union { struct {
      UCHAR enabled : 1;
      UCHAR update_map : 1;
      UCHAR update_data : 1;
      UCHAR temporal_update : 1;
      UCHAR Reserved : 4;
    }; UCHAR ControlFlags; } DUMMYUNIONNAME;
    UCHAR  Reserved24Bits[3];
    /* feature_mask[8] (8 one-bit sub-flags each), feature_data[8][8] */
  } segmentation;   // segmentation_enabled rejected (this scope) — always disabled/zeroed
  struct {
    /* apply_grain/scaling_shift/ar_coeff_lag/... + full scaling-point/AR-coefficient tables */
  } film_grain;   // film_grain_params_present rejected (this scope) — always disabled/zeroed
  UINT              Reserved32Bits;
  UINT              StatusReportFeedbackNumber;
} DXVA_PicParams_AV1, *LPDXVA_PicParams_AV1;
```

`cdef`/`segmentation`/`film_grain`'s full field lists are omitted above for length (all
individually cited in-tool during this session's fetch of the primary source; every field is
irrelevant/zeroed under this ADR's rejection-heavy scope, § Scope decision) — implementation must
re-fetch the same Microsoft Learn page for the complete field-for-field layout before writing
`av1_pic_params.rs`'s `repr(C)` struct, not transcribe from this ADR's abridged reproduction.

**Real structural difference from H.264/HEVC, not a gap**: `DXVA_PicParams_AV1` has **no separate
qmatrix struct/DXVA argument** — `qm_y`/`qm_u`/`qm_v` are plain scalar fields inline in
`quantization`. `av1_ops.rs` must therefore build only **two** `D3D12_VIDEO_DECODE_FRAME_ARGUMENT`
entries (`PICTURE_PARAMETERS` + `SLICE_CONTROL`/tiles), not three like `ops.rs`/`hevc_ops.rs`.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| AV1 "still picture" mode only (`reduced_still_picture_header == 1`, exactly one frame per stream, no `sequence_header_obu` repeated between pictures) | Rejected — strictly narrower and less useful than `KEY_FRAME`-only-with-repeated-headers: forecloses even a trivial all-intra multi-frame GOP, which is the realistic shape of any obtainable same-workspace test source (this crate's own AV1 encoder emits a session-prefix + per-frame `OBU_FRAME`, i.e. exactly the repeated-header shape this ADR supports, not still-picture mode). |
| General GOP (inter-frame, persistent 8-slot reference-frame model, `frame_refs`/global-motion tracking) | Rejected for this pass — genuinely novel reference-model complexity (§ Scope decision reason 3) stacked on an entirely unverified decode path, with zero same-crate GPU-verified AV1 inter-prediction precedent on *either* encode or decode, on any backend. Deferred, not designed here. |
| Reuse `mediaway-sw::av1` (`rav1e` wrapper) as a decode-parsing source | Considered, rejected as a parsing port target (§ Context) — it is a complete software *encoder* around `rav1e`'s own opaque `Context`/`Frame`/`Packet` API, with no OBU/sequence-header/frame-header bitstream-syntax logic exposed to port. It remains a real candidate as an *alternative test-bitstream source* (§ Open questions #1, § Test plan) precisely because it is a real, already-vendored, BSD-2-Clause, spec-conformant AV1 encoder — a different role than "parsing precedent." |
| Generify `Session<M>`/`ops.rs` now, share one implementation across H.264/HEVC/AV1 | Considered — rejected this pass, same reasoning ADR-0004 already gave: requires editing the existing H.264-bearing files whose current state is the last known-consistent baseline for an unresolved real hardware hang. Deferred to a follow-up once at least one codec's GPU decode path is hardware-verified (now three codecs' worth of duplicated `ops*.rs` shape once this ADR is implemented — a stronger case for that eventual refactor than ADR-0004 had, still not undertaken here). |
| Build `av1_refs.rs`/reference-list scaffolding now even though this scope never exercises it (forward-looking symmetry with `h264_refs.rs`/`hevc_refs.rs`) | Considered, rejected — no real inter-frame AV1 reference model exists anywhere in this workspace to ground such a module in yet (unlike, say, porting a real-but-unwired parser the way HEVC did); an empty scaffold would be speculative, non-tested code with no test able to exercise it under this ADR's own scope. Left for the general-GOP follow-up alternative above. |

## Consequences

### Positive

- **Primary-source DXVA struct ground-truthing** (Microsoft's own official driver DDI reference,
  not a third-party mirror) is a stronger footing than either ADR-0002 (Wine `dxva.h` mirror) or
  ADR-0004 (Wine mirror + an acknowledged, unresolved Microsoft Learn rendering discrepancy) had
  for their own codecs.
- **`dpb.rs`/`setup.rs`/`util.rs` need zero edits** — same reuse win ADR-0004 already established,
  now proven across a third codec.
- **No reference-list/RPS/POC module needed at all** — this ADR's `KEY_FRAME`-only scope is
  structurally simpler than HEVC's own P-slice cut (ADR-0004), itself already the smallest
  non-trivial HEVC increment; AV1's `frame_refs`/`RefFrameMapTextureIndex` collapse to a constant,
  trivially-correct empty state under this scope.
- **Two independent, unrelated pieces of same-driver evidence** (`AV1_INVALID_DPB_RESOURCE_INDEX`
  from the encoder, `DXVA_PicEntry_AV1::Index`'s own documented `0xFF` sentinel from the decode
  spec) agree on the same convention — a genuine, if narrow, real-hardware-adjacent cross-check
  this ADR's design did not have to assume blind.
- The additive-only file layout means implementing this ADR carries **zero risk of silently
  changing H.264 or HEVC decode behavior**.

### Negative / Trade-offs

- **This ADR's realistic test-bitstream source is itself unverified as spec-legal**
  (`docs/standards/registry.toml`'s own "not decodable by libdav1d" finding, § Context) — a
  materially larger, AV1-specific risk than either sibling ADR carried; even a future hardware
  attempt with informed consent may need to source its input bitstream from `rav1e`
  (`mediaway-sw::av1`) instead of this crate's own D3D12 AV1 encoder, adding real setup cost
  (`rav1e` is not currently a `mediaway-decoder-windows` dev-dependency).
- **This scope's rejection list narrows practical interop below "any conformant Main-profile
  intra AV1 stream"** (§ Scope decision reason 4) — real third-party all-intra AV1 output (e.g.
  default libaom/SVT-AV1 settings) may routinely fail this module's own `Unsupported` checks even
  though it is fully spec-legal all-intra content.
- No GPU-verified AV1 decode precedent exists anywhere in this workspace, on any backend, on any
  branch (§ Correcting the task's premise) — every claim about `DXVA_PicParams_AV1` field
  semantics is, at best, cross-checked against a primary spec/DDI-reference document and this
  crate's own encoder's static field choices, never against a real driver.
- Real, acknowledged code duplication (`av1_ops.rs` alongside `ops.rs`/`hevc_ops.rs`) — same
  deliberate, documented trade-off ADR-0004 already made, now a third copy of the same shape.
- Every open question ADR-0002/ADR-0004 leave unresolved (DPB-eviction backpressure error shape,
  whether per-codec high-level-syntax parsing should become a shared crate, COM `.clone()`
  discipline, `GpuBufferHandle::DirectX12`'s missing `subresource` field) still applies here,
  unre-litigated.

## Open questions / risks

1. **Is this crate's own D3D12 AV1 encoder's output actually a legal, decodable AV1 bitstream?**
   `docs/standards/registry.toml`'s `av1-bitstream-spec` entry states plainly that it currently is
   not (per `libdav1d`). This is this ADR's single largest, AV1-specific open risk — the § Test
   plan's planned "chain this crate's own encoder into the decoder under test" technique (used
   successfully for H.264/HEVC) may simply not have a valid input to chain from. **Before any real
   hardware attempt**, an implementer must either (a) confirm this finding is stale / has since
   been fixed on the encoder side, or (b) source a test bitstream from `mediaway-sw::av1`
   (`rav1e`) or another independently-known-conformant encoder instead.
2. **Does D3D12 AV1 decode have its own undiscovered feature-query or codec-configuration quirk**,
   analogous to encode's real `SUPPORT`→`SUPPORT1` trap or its mandatory
   `AUTO_SEGMENTATION`/`CDEF_FILTERING`/`LOOP_RESTORATION_FILTER` session-level declaration? Ruled
   out structurally for the exact `SUPPORT1` shape (no such sibling struct exists for decode), but
   not otherwise investigated — first thing to watch for via `ID3D12InfoQueue` debug-layer output
   once a real hardware attempt is authorized.
3. **`DXVA_PicParams_AV1.tiles.widths[64]`/`heights[64]`/`context_update_id` exact semantics for
   the `cols == rows == 1` case** — believed straightforward (`widths[0]` = total superblock
   columns, `heights[0]` = total superblock rows, `context_update_id == 0`, matching this scope's
   only legal tile grid) but **not independently cross-checked against a second real DXVA-AV1
   producer** (e.g. FFmpeg's `libavcodec/d3d12va_decode.c`, found but not fetched this session) —
   flagged as the first implementation-time verification task, mirroring ADR-0004's own Open
   Question #2/#3 precedent for the same class of gap.
4. **`RefFrameMapTextureIndex[8]`/`frame_refs[7]` all-`0xFF` for every `KEY_FRAME`-only stream**
   — believed correct by spec text plus this scope's own guarantee that no picture is ever
   referenced (§ Context finding #4's cross-check), but not hardware-confirmed.
5. **DPB sizing for this scope**: since no picture is ever held as a reference, the DPB only needs
   enough slots to absorb `CALLER_HEADROOM`-style outstanding-Zero-Copy-handle latency (no
   `sps_max_dec_pic_buffering`-equivalent signaled value to size against, unlike H.264/HEVC) — this
   ADR proposes a fixed small constant (e.g. `CALLER_HEADROOM + 1`), not validated against any real
   stream's behavior.
6. **Follow-up refactor: generify `Session<M>`/`ops.rs`** across all three codecs now that a third
   near-identical `*_ops.rs` copy would exist — same deferred-not-forgotten status ADR-0004 already
   flagged, now with a stronger duplication case.
7. **`mediaway_common::GpuBufferHandle::DirectX12`'s missing `subresource` field** — same
   cross-crate gap ADR-0002/ADR-0004 already flagged, applies identically to `DecodedOutputAv1::Gpu`.
8. **Practical interop narrower than "any conformant Main-profile intra AV1 stream"** (§ Scope
   decision reason 4, § Consequences) — real third-party encoders' default all-intra output may
   routinely trip this module's own tool-rejection checks; not resolved here, an acknowledged,
   named trade-off for this first increment.
9. **Should `av1_obu.rs`/`av1_sequence_header.rs`/`av1_frame_header.rs`'s parsing logic be
   extracted into a shared, graphics-API-agnostic crate/module**, given this workspace has now
   independently written AV1 encode-side OBU-writing logic (`mediaway-encoder-windows`) and would
   gain decode-side OBU-reading logic here, with no current sharing between them (mirrors ADR-0004's
   own Open Question #6 for HEVC's Vulkan/D3D12 duplication, not decided here either)?

## Test plan

**Sans-io unit tests — write and run this pass, no hardware involved, no risk:**

- `av1_obu_tests.rs` — `leb128` round-trip (encode-then-decode against hand-picked values
  including multi-byte boundary cases), `obu_header()` parse (type/has-size-field bit),
  `split_obus` against a hand-built multi-OBU byte sequence (temporal delimiter + sequence header
  + frame OBU), truncated/malformed-input error cases.
- `av1_sequence_header_tests.rs` — parse against a fixture built with this crate's own
  `mediaway-encoder-windows::bitstream_av1::build_av1_session_prefix`-shaped bytes (same all-fixed
  field values, hand-constructed independently — not by literally calling the encoder crate, to
  keep this a real sans-io round-trip test rather than a tautology); explicit tests for every
  scope-cut rejection (`seq_profile != 0`, `high_bitdepth == 1`, `mono_chrome == 1`, non-4:2:0
  subsampling, `reduced_still_picture_header == 1`, `enable_cdef == 1`, `enable_restoration == 1`,
  `enable_superres == 1`).
- `av1_frame_header_tests.rs` — parse the fixed all-intra `uncompressed_header()` shape
  `bitstream_av1.rs::write_frame_header` documents; explicit rejection tests for
  `frame_type != KEY_FRAME`, `show_existing_frame == 1`, `segmentation_enabled == 1`,
  `using_qmatrix == 1`, multi-tile (`tiles.cols > 1`/`tiles.rows > 1`).
- `av1_pic_params_tests.rs` — `DxvaPicParamsAv1`/`DxvaPicEntryAv1`/`DxvaTileAv1` field-packing
  against hand-built `SequenceHeader`/`FrameHeader` fixtures; `repr(C)` size/offset sanity checks
  per field group (no `windows`-crate reference struct to compare via `size_of`, same situation
  ADR-0004's `hevc_pic_params_tests.rs` already handles); explicit check that `frame_refs`/
  `RefFrameMapTextureIndex` are always fully `0xFF`.

Run via `cargo check -p mediaway-decoder --all-targets --features video`, `cargo clippy -p
mediaway-decoder --all-targets --all-features -- -D warnings`, `cargo test -p mediaway-decoder
--lib --features video` — all achievable and expected to be **required to pass** before this
ADR's implementation is considered done, with **zero hardware/device involvement**.

**Hardware-gated integration test — write, do NOT run:**

> ⚠️ **`d3d12_video_decode_av1_tests.rs` must be written this pass (or in this ADR's eventual
> implementation pass) following the exact soft-skip convention
> `d3d12_video_decode_hevc_tests.rs`/`d3d12_video_decode_tests.rs` already use — but it must NOT
> be executed by whoever implements this ADR.** Per the safety banner, running it risks a real
> `DXGI_ERROR_DEVICE_HUNG` TDR, on code that has never been run at all, **and** (§ Open questions
> #1) may not even have a confirmed-legal input bitstream to test with in the first place. A
> human/agent with informed, deliberate, separate consent for a real hardware attempt must decide
> whether and when to run it — and should resolve Open question #1 first, independently of
> whether the D3D12 decode path itself is ready, since a garbage/undecodable input bitstream
> would make any resulting hang or wrong-output symptom impossible to attribute correctly.

Planned shape once that consent exists (design only, not built this pass): **first**, confirm
whether `mediaway-encoder-windows`'s D3D12 AV1 encoder now produces a `libdav1d`-decodable stream
(re-run that crate's own encode-only test plus an external `dav1d`/`ffmpeg` oracle check outside
this crate); if still not decodable, source a real conformant `KEY_FRAME`-only Main-profile 8-bit
4:2:0 AV1 bitstream from `mediaway-sw::av1` (`rav1e`) instead (a new dev-dependency this crate
would need to add, with the usual `deps-policy.md` justification). Feed those exact bytes into
`D3d12VideoDecoderAv1`; soft-skip (not fail) on any `open`/`push_packet`/`poll_frame` error,
consistent with this workspace's "a real, not-yet-root-caused bug must soft-skip" convention.

## References

- [`mediaway-decoder-windows` ADR-0002](0002-d3d12-native-video-decode.md) — this module's
  founding ADR; **read the safety banner's citations in full before implementing anything here**.
- [`mediaway-decoder-windows` ADR-0004](0004-d3d12-hevc-single-forward-ref-p-slice-decode.md) —
  the immediate architectural precedent this ADR mirrors (parallel-implementation strategy,
  sans-io-only test plan, hand-defined DXVA-adjacent structs, additive-only file layout).
- `crates/mediaway-encoder/src/windows/d3d12_video_encode/{av1.rs,ops_av1.rs,bitstream_av1.rs}`
  — real, hardware-verified D3D12 AV1 **encode** source read directly this session; this ADR's
  primary cross-check for OBU/sequence-header/frame-header field values and D3D12 AV1
  profile-negotiation patterns.
- `crates/mediaway-decoder/src/windows/d3d12_video_decode/{dpb.rs,setup.rs,util.rs,ops.rs,
  hevc_decoder.rs,hevc_pic_params.rs,hevc_refs.rs}` and
  `crates/mediaway-decoder/src/windows/d3d12_video_decode.rs` — the existing H.264/HEVC
  implementation this ADR extends/mirrors, read directly this session.
- `crates/mediaway-sw/src/h264/bitreader.rs` — `BitReader::read_bit`/`read_bits` reused for AV1's
  own fixed-width bit reads (§ Reuse).
- `crates/mediaway-sw/src/av1.rs` — checked and found **not** a bitstream-parsing source (a
  `rav1e`-wrapping software encoder); flagged as a real candidate test-bitstream source instead
  (§ Open questions #1, § Test plan).
- Microsoft Learn (Windows Driver DDI reference, `dxva.h`, fetched this session — primary source
  for every DXVA-AV1 struct above):
  [`DXVA_PicParams_AV1`](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/dxva/ns-dxva-dxva_picparams_av1),
  [`DXVA_PicEntry_AV1`](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/dxva/ns-dxva-dxva_picentry_av1),
  [`DXVA_Tile_AV1`](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/dxva/ns-dxva-dxva_tile_av1).
- [`docs/standards/registry.toml`](../../../../docs/standards/registry.toml) — `av1-bitstream-spec`
  entry (cached at `local/standards/av1-bitstream-spec/av1-spec.pdf`), and its own real,
  load-bearing "not decodable by libdav1d" finding about this crate family's AV1 encoder output
  (§ Context, § Open questions #1).
- [`docs/spec/sans-io.md`](../../../../docs/spec/sans-io.md),
  [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md),
  [`docs/spec/gpu-interop.md`](../../../../docs/spec/gpu-interop.md).

ADRs are **English**. Numbering is local to this `adr/` folder.
