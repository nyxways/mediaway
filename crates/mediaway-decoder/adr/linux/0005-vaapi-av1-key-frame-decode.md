# ADR-0005: VA-API AV1 `KEY_FRAME`-only decode (bit-reader reuse from `mediaway_sw::h264`; no OBU-parser porting source exists)

- **Status**: Accepted — implemented (`src/linux/vaapi/av1.rs` +
  `src/linux/vaapi/av1/{obu,bits,sequence_header,frame_header,tile_info}.rs`), compile +
  clippy (`--all-targets -- -D warnings`) + test-verified on real WSL2 Linux
  (`cargo test -p mediaway-decoder --all-features --target x86_64-unknown-linux-gnu`, 2026-08-19).
  Every `VAProfileAV1*`/`VAAV1TransformationType`/`VA_INVALID_ID` assumption this ADR made
  compiled correctly against real bindgen output on the first pass — no ADR-vs-reality mismatch
  needed fixing beyond ordinary `clippy::pedantic`/`clippy::nursery` lint cleanup (documented
  `#[allow(...)]`s with reasons, same convention as this crate's H.264 siblings). **Zero
  real-hardware verification** — same standing caveat as ADR-0001/0002, unchanged by this pass.
  See `docs/ai/wiki/platform/linux-decode.md` for the current, honest status summary.
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (`src/linux/vaapi/`)

## Note on this ADR's design brief

The task that produced this ADR asserted this crate already has a landed "VA-API HEVC" ADR
(`adr/linux/0003-vaapi-hevc-p-slice-dpb.md`) and a Vulkan AV1 decode implementation
(`vulkan/av1_params.rs`, `av1_params/av1_frame_header.rs`, `av1_refs.rs`, own ADR
`adr/vulkan/0002-av1-decode-keyframe-first.md`) to reuse as porting sources. **Both are re-checked
against this repository this session and do not exist**:

- `crates/mediaway-decoder/adr/linux/` has exactly two ADRs: `0001` (H.264 CPU-output, IDR-only)
  and `0002` (H.264 single-forward-reference P-slice decode, dated the same day as this one, DPB
  ported from `vulkan/dpb.rs`). **There is no VA-API HEVC ADR anywhere in this workspace.** HEVC
  decode exists only on the Vulkan backend (`mediaway-decoder::vulkan::decoder_hevc`, real,
  hardware-verified per `docs/roadmap.md` §2's "Vulkan HEVC Decode: root cause found and fixed").
  This ADR is therefore `0003`, and its actual porting-methodology precedent is `0002` (H.264
  P-slice/DPB decode), not a nonexistent HEVC one.
- `mediaway-decoder::vulkan` has **no** `av1_params.rs`, `av1_refs.rs`, or any AV1 file at all —
  confirmed by listing the full directory this session (23 files, all H.264/HEVC-named:
  `h264_params.rs`, `h264_slice.rs`, `hevc_params.rs`, `hevc_slice.rs`, `decoder_hevc.rs`, `dpb.rs`,
  `session.rs`, `probe.rs`, `cpu_readback.rs`, `zero_copy.rs`, `session_command*.rs`, none named
  `av1*`). `mediaway-decoder::adr::vulkan` has exactly one ADR, `0001-vulkan-video-decode.md` (no
  `0002`). `docs/roadmap.md` §2 states, current and unambiguous: "**AV1 decode has not been
  started**" (workspace-wide, every backend). **This is the single most consequential correction
  in this ADR**: there is no hardware-verified (or even just-written) AV1 OBU/sequence-header/
  frame-header **parser** anywhere in this workspace to port from. This ADR must derive AV1
  bitstream parsing largely from the spec text itself — see § Why this ADR cannot be a pure port,
  unlike its `0002` sibling, for how this ADR still meaningfully reduces that risk using real,
  in-repo, spec-cited material that *does* exist (an AV1 OBU **writer**, the inverse direction).

This note exists so a reader comparing this ADR against the original task brief does not conclude
a citation was invented — every citation below was independently re-verified by reading the named
file this session.

## Context

`mediaway-decoder::linux::vaapi` (this crate) decodes H.264 only today (ADR-0001 IDR-only
baseline, ADR-0002 P-slice/DPB extension). `mediaway_common::CodecKind::Av1` already exists
workspace-wide — this crate's `linux/vaapi/codec.rs::h264_profile_candidates`/
`is_supported_video_codec` simply have no AV1 path (`codec.rs:16-38`, current file, H.264-only).

This ADR designs adding `CodecKind::Av1` decode support: **`KEY_FRAME`-only**, deliberately
mirroring this same crate family's H.264 ADR-0001 starting point (IDR-only, no DPB, no
reference-picture-list construction) and the reasoning the (nonexistent, per the note above)
design brief correctly anticipated even though its cited artifact does not exist — a
narrowest-real-useful-subset first increment is this workspace's own standing precedent for a new
codec's first decode pass on any backend (H.264 VA-API ADR-0001, HEVC Vulkan's own first landing,
per `docs/roadmap.md`).

### Why this ADR cannot be a pure port, unlike its `0002` sibling — and what *does* reduce its risk

ADR-0002 (H.264 P-slice/DPB) was a near-pure port: `vulkan::dpb.rs`/`h264_slice.rs` already had
real, hardware-verified DPB and reference-list-construction logic to copy field-for-field. **No
such AV1 decode-side precedent exists in this workspace** (see § Note above) — this ADR's
bitstream-parsing logic (an `uncompressed_header()` reader) has no sibling implementation to port.

However, two real, in-repo, already-cited-against-the-AV1-spec artifacts meaningfully reduce (not
eliminate) that risk, both confirmed by reading them directly this session:

1. **`mediaway-encoder::windows::d3d12_video_encode::bitstream_av1`** — a real, 315-line, AV1
   spec-section-cited (`§5.5` sequence header, `§5.5.2` color config, `§5.9.2`
   `uncompressed_header()`, `§5.9.5` frame/render size, `§5.9.12` quantization, `§5.9.15` tile
   info, `§5.9.11` loop filter, `§5.9.19` CDEF, `§5.9.20` loop restoration) **writer** for exactly
   this ADR's target profile (Main, 8-bit 4:2:0, `KEY_FRAME`-only, single tile, all optional
   coding tools disabled). A writer is the literal inverse of a parser for the same syntax
   elements — every `w.write_bit(0)`/`w.write_bits(...)` call in `write_sequence_header`/
   `write_frame_header` (`bitstream_av1.rs:132-282`) names, in an inline comment, exactly which
   AV1 syntax element it writes and exactly which conditional-inference rule (per §5.9.2's own
   "if X then read Y, else Y is inferred to Z" chain) determined whether that element is present
   at all. This ADR's own parser needs to walk the identical conditional chain in the read
   direction — the writer's own comments are a real, already-correct (D3D12-driver-verified byte
   layout for this identical scope) checklist for which fields this ADR's reader must expect,
   and in which order, even though no single line of Rust is reused verbatim (a reader and a
   writer are structurally different code, unlike `Dpb`/`GopState` which are literally the same
   logic regardless of read/write direction).
2. **`mediaway_sw::h264::BitReader`** (`crates/mediaway-sw/src/h264/bitreader.rs`) — a plain,
   codec-agnostic MSB-first raw-bit reader (`read_bit() -> Result<u32, H264Error>`,
   `read_bits(count: u32) -> Result<u32, H264Error>`, `bitreader.rs:10,34,47`). This is the
   **exact same reuse case** `mediaway-decoder`'s own ADR-0001 already made for H.264
   ("Reuse `mediaway_sw::h264`'s `BitReader`... instead of re-implementing bit-level framing" —
   ADR-0001's Decision) — the reader's raw-bit mechanics have nothing H.264-specific about them
   (no Exp-Golomb, no NAL awareness, just MSB-first bit extraction from a byte slice). AV1 uses
   different variable-length codes at the syntax layer (`uvlc()`, `leb128()`, `su(n)`, `ns(n)`,
   never H.264/HEVC's `ue(v)`/`se(v)`), but the same raw `read_bit`/`read_bits` primitive
   underlies all of them — this ADR reuses `BitReader` directly for `f(n)` reads and implements
   AV1's own small set of variable-length decoders (`uvlc()`/`leb128()`/`su(n)`, AV1 spec
   §4.10.3/§4.10.5/§4.10.6) as new, local, AV1-specific free functions built on top of it,
   the same "reuse the primitive, not the syntax layer" split ADR-0001 already used for
   H.264/PPS/SPS/slice-header parsing (its own Alternatives table: "avoids duplicating the
   trickiest, least-reusable part (bit framing / emulation prevention) while keeping the
   VA-API-specific field set honest and local").

### Why AV1 decode needs no packed-header submission (unlike this ADR's encoder sibling)

This ADR's `mediaway-encoder` sibling
(`crates/mediaway-encoder/adr/linux/0005-vaapi-av1-key-frame-and-inter-gop.md`) found a real,
blocking `cros-libva` gap: AV1 **encode** needs the application to construct and submit real
`frame_header_obu()` bytes via a packed-header buffer type `cros-libva` 0.0.13 does not wrap.
**Decode has the opposite shape and no such gap.** VA-API decode (any codec) never asks the
application to *write* bitstream bytes — only to *parse* the driver-opaque syntax elements out of
an incoming bitstream and place them into a plain C struct
(`VADecPictureParameterBufferAV1`/`VASliceParameterBufferAV1`) the driver then uses to drive its
own internal entropy decode + reconstruction, exactly the same shape H.264/HEVC VA-API decode
already uses in this crate. Confirmed directly: `cros-libva` 0.0.13's `src/buffer/av1.rs` already
provides real, complete safe wrappers for both decode buffer types
(`PictureParameterBufferAV1::new`, `av1.rs:388-512`; `SliceParameterBufferAV1`,
`av1.rs:519-562`) — **no `cros-libva` extension is needed for this ADR**, unlike its encoder
sibling.

## Decision

> Add `CodecKind::Av1` `KEY_FRAME`-only decode to `mediaway-decoder::linux::vaapi`: single tile,
> Main profile (8-bit 4:2:0), all optional coding tools (segmentation, film grain, CDEF, loop
> restoration, superres, warped motion, delta-Q, delta-LF) rejected as `Unsupported` if signaled
> — this crate accepts exactly the all-disabled-tool AV1 subset this workspace's own AV1 encoders
> (D3D12, and — once its own blocking gap is resolved — VA-API's `0003` sibling) already produce,
> giving this ADR a real, in-repo round-trip correctness target beyond system `ffmpeg`/`ffprobe`
> alone. No reference-picture management, no DPB, no `INTER_FRAME` — a genuinely narrower scope
> than this crate's own H.264 ADR-0002 sibling (which reached P-slice/DPB in the same pass), a
> deliberate choice explained in § Scope below.

### Scope

**In (this ADR's design):**

- `OBU_TEMPORAL_DELIMITER`/`OBU_SEQUENCE_HEADER`/`OBU_FRAME` (or `OBU_FRAME_HEADER` +
  `OBU_TILE_GROUP` split — both legal AV1 framings; this ADR accepts either, see § Bitstream
  parsing below) OBU scanning: `leb128()`-length-prefixed OBU splitting (AV1's own framing,
  no start codes/emulation prevention, structurally simpler than H.264/HEVC Annex-B).
- `sequence_header_obu()` parsing: Main profile only (`seq_profile == 0`), 8-bit only
  (`!high_bitdepth`), 4:2:0 only (matches this crate's NV12-only output convention, same as
  H.264/HEVC), `reduced_still_picture_header == 0` (a real multi-frame stream, matching this
  workspace's own encoders' choice — `av1_params.rs`'s own doc: "`reduced_still_picture_header`...
  produced a real-hardware-verified **invalid** bitstream"), every optional coding tool
  (`enable_filter_intra`/`enable_intra_edge_filter`/`enable_interintra_compound`/
  `enable_masked_compound`/`enable_warped_motion`/`enable_dual_filter`/`enable_jnt_comp`/
  `enable_ref_frame_mvs`/`enable_superres`/`enable_cdef`/`enable_restoration`/
  `film_grain_params_present`) rejected as `Unsupported` if set to `1` — this ADR's parser reads
  each flag (correct bit consumption either way) but returns `DecodeError::Unsupported` rather
  than attempting to build the corresponding (non-trivial) parameter-buffer sub-structs.
  `enable_order_hint` may be `0` or `1` (accepted either way — a `KEY_FRAME`'s own `order_hint`
  is always `0` regardless, so this costs nothing to accept and matches both this workspace's
  D3D12 encoder, which sets `0`, and its Vulkan encoder, which sets `1`).
- `uncompressed_header()` parsing (AV1 spec §5.9.2), `KEY_FRAME`-only branch: `frame_type ==
  KEY_FRAME(0)`, `show_frame == 1` required (a non-shown/`show_existing_frame` keyframe is
  rejected as `Unsupported` — no output-reordering/decoder-DPB-replay logic this scope needs);
  `tile_info()` accepted only when it resolves to exactly one tile (`TileCols == TileRows == 1`,
  matching `windows::bitstream_av1::write_tile_info`'s own always-one-tile scope for this
  crate's validated resolution range); `quantization_params()`/`segmentation_params()`/
  `delta_q_params()`/`delta_lf_params()`/`loop_filter_params()`/`cdef_params()`/`lr_params()`
  parsed far enough to confirm every optional tool is off (mirrors this crate's own H.264
  ADR-0001 precedent: "PPS extension fields... are read far enough to confirm absence; a stream
  that sets them returns `Unsupported`").
- `VADecPictureParameterBufferAV1`/`VASliceParameterBufferAV1` construction with every reference
  slot `VA_INVALID_ID`/unused (a `KEY_FRAME` references nothing), matching this crate's own
  H.264 ADR-0001 IDR-only convention exactly.
- Lazy pipeline creation (`Config`/`Surface`/`Context` created on first parsed sequence header),
  matching ADR-0001's own H.264 precedent.
- Profile: `VAProfileAV1Profile0` (Main), entrypoint `VAEntrypointVLD` (VA-API's decode
  entrypoint is codec-uniform — the same one this crate's H.264 path already uses,
  `codec.rs`/`h264.rs`'s existing `VAEntrypoint::VAEntrypointVLD` usage — unlike encode, decode
  has no "low power" entrypoint split to probe).

**Out (deferred, tracked in `docs/roadmap.md`):**

- `INTER_FRAME` decode (single-forward-reference or otherwise) — no DPB, no reference-frame-slot
  management, no motion-vector prediction, no CDF forward-adaptation. **Deliberately not
  attempted this pass** — see § Why decode-side GOP support is a substantially larger lift than
  encode-side GOP support below, the reasoning this ADR uses instead of blindly mirroring
  ADR-0002's H.264 same-pass DPB extension.
- CDEF, loop restoration, segmentation, film grain, superres, screen-content tools, warped
  motion, multi-tile, non-Main profile, > 8-bit, monochrome, film grain — all rejected as
  `Unsupported`, not silently mishandled.
- Zero-Copy DMA-BUF surface export — unrelated axis, ADR-0001's own deferral, unchanged.
- `AVCC`-style length-prefixed OBU framing from a demuxer — this ADR assumes the low-overhead
  bitstream ("Annex B"-equivalent, length-prefixed-OBU-stream) framing AV1 streams commonly use
  raw from an encoder/muxer's elementary stream; a demuxer-specific framing adapter is out of
  scope here, same disposition ADR-0001 already gives H.264 Annex-B vs. AVCC.

### Why decode-side `INTER_FRAME`/GOP support is a substantially larger lift than encode-side GOP support

This ADR's `mediaway-encoder` sibling adds single-forward-reference `INTER_FRAME` GOP encode in
the *same* ADR as its `KEY_FRAME` baseline, because a real, ready-to-port precedent
(`vulkan::av1_gop::GopState`) already exists for it. **Decode has no equivalent precedent**, and —
independent of that gap — AV1 *decoding* an `INTER_FRAME` is intrinsically a larger step up from
`KEY_FRAME`-only than *encoding* one, for reasons specific to AV1 (not shared with H.264/HEVC,
where ADR-0002's encoder/decoder pair *did* reach P-frame support together):

- An **encoder** choosing single-forward-reference `INTER_FRAME` output can freely set
  `primary_ref_frame = PRIMARY_REF_NONE` (as `vulkan::av1_params` already does) to sidestep AV1's
  CDF-forward-adaptation entirely — the encoder controls every bit it emits, so it can always pick
  the simplest legal encoding. A **decoder** has no such freedom: it must correctly implement
  whatever `primary_ref_frame`/CDF-context-loading behavior a real-world encoder's stream actually
  signals, which for genuine `INTER_FRAME` content is `primary_ref_frame != PRIMARY_REF_NONE` far
  more often than not (most real encoders carry CDF state forward for compression efficiency)
  — meaning a decoder-side `INTER_FRAME` path realistically needs real CDF-context bookkeeping
  from day one to be useful against real-world streams, not just this workspace's own
  all-`PRIMARY_REF_NONE` encoder output.
- AV1's reference model is a full **9-way indirection** (`ref_frame_idx[7]` naming which of 8
  physical `RefFrameId` slots each of AV1's 7 named reference types (`LAST`/`LAST2`/`LAST3`/
  `GOLDEN`/`BWDREF`/`ALTREF2`/`ALTREF`) currently points at) plus per-slot `RefOrderHint`/
  `RefFrameType`/`RefUpscaledWidth`/`RefUpscaledHeight`/`RefRenderWidth`/`RefRenderHeight`/
  `RefMiCols`/`RefMiRows`/`RefFrameId`/`RefSubsamplingX`/`RefSubsamplingY`/`RefBitDepth` state
  (AV1 spec §7.20 `reference_frame_update_process`) that decode alone must maintain for
  `frame_size_with_refs()`/`motion_field_estimation()`/segmentation-ID prediction to work at all
  even for a *single* forward reference — a meaningfully larger, AV1-specific state surface than
  H.264's `frame_num`/POC/sliding-window DPB this crate's own ADR-0002 already ported.
- No workspace precedent (Vulkan, D3D12, or otherwise) exists anywhere for AV1 **decode**-side
  reference management to port from, unlike H.264/HEVC's Vulkan decoders. This ADR would be
  entirely spec-derived for that piece, the single highest-risk kind of work this workspace's own
  established practice (ADR-0002's own repeated "port, don't re-derive" reasoning) explicitly
  avoids where a real alternative exists.

This is a scope decision, not an oversight — flagged explicitly, matching this ADR's own honesty
requirement, as a real follow-up a future ADR should pick up once a decode-side reference-tracking
precedent exists (either from real-hardware AV1 decode experience on this backend, or from a
future Vulkan/D3D12 AV1 decode port this workspace has not started).

### Bitstream parsing — new modules, ported bit-level primitive, new AV1-specific syntax layer

New files under `crates/mediaway-decoder/src/linux/vaapi/`, sans-io
(`#![forbid(unsafe_code)]`, matching this crate's existing `mod.rs`-level convention — confirmed:
`mediaway-decoder-linux`'s own crate root already uses `#![forbid(unsafe_code)]`, ADR-0001's
Consequences: "this crate uses `#![forbid(unsafe_code)]` at the crate root"), unit-testable
without any VA-API device — mirrors this crate's existing `sps.rs`/`pps.rs`/`slice.rs` shape:

| New module | Reused/ported from (cited source) | New (this ADR, no porting source) |
|---|---|---|
| `av1/obu.rs` — `leb128()` reader, OBU header (`obu_type`/`obu_extension_flag`/`obu_has_size_field`) split | Inverse of `windows::bitstream_av1::write_leb128`/`obu_header_byte` (`bitstream_av1.rs:40-56`) — same spec section (§4.10.5, §5.3.2), read direction is new code but the byte-layout knowledge is already validated by the writer's own D3D12-driver-accepted output | `leb128()` **decoding** loop itself (continuation-bit accumulation, the writer only ever encodes) |
| `av1/bits.rs` — `f(n)` via `mediaway_sw::h264::BitReader::read_bits` directly; new `uvlc()`, `su(n)`, `ns(n)` decoders | `BitReader::read_bit`/`read_bits` (`bitreader.rs:34,47`) reused directly, same primitive-reuse precedent as ADR-0001's H.264 `BitReader` reuse | `uvlc()`/`su(n)`/`ns(n)` (AV1 spec §4.10.3/§4.10.6/§4.10.7) — no H.264/HEVC equivalent exists anywhere in this workspace to port; implemented directly from spec text, each function doc-cited to its exact spec subsection |
| `av1/sequence_header.rs` — `SequenceHeader` struct + `parse()` | Field-presence/order cross-checked against `windows::bitstream_av1::write_sequence_header` (`bitstream_av1.rs:132-192`) inline comments, which already enumerate every conditional-inference rule for this exact profile | The parse function itself — new code, spec-cited per field (§5.5.1/§5.5.2), cross-checked (not copied) against the writer |
| `av1/frame_header.rs` — `FrameHeader` struct + `parse()` (`KEY_FRAME` branch only) | Field-presence/order cross-checked against `windows::bitstream_av1::write_frame_header` (`bitstream_av1.rs:204-282`) inline comments (which already document, per field, which §5.9.2 inference rule applies for `frame_type == KEY_FRAME`) | The parse function itself — new code, spec-cited (§5.9.2, §5.9.5, §5.9.11, §5.9.12, §5.9.15), cross-checked against the writer |
| `av1/tile_info.rs` — `tile_info()` parse, single-tile-only acceptance | Inverse of `windows::bitstream_av1::write_tile_info`'s `tile_log2`/min/max tile-count math (`bitstream_av1.rs:71-128`) — same spec section (§5.9.15), same arithmetic, read direction is new but the formula is already validated | The parse-and-reject-if-multi-tile logic itself |

### VA-API-specific plumbing (distinct from the bitstream parser above)

**Confirmed by reading `cros-libva` 0.0.13's real vendored source directly**
(`.../cros-libva-0.0.13/src/buffer/av1.rs`, line numbers refer to this file):

- `PictureParameterBufferAV1::new(profile, order_hint_bits_minus_1, bit_depth_idx,
  matrix_coefficients, seq_info_fields: &AV1SeqFields, current_frame: VASurfaceID,
  current_display_picture: VASurfaceID, anchor_frames_list: Vec<VASurfaceID>,
  frame_width_minus1, frame_height_minus1, ..., ref_frame_map: [VASurfaceID; 8],
  ref_frame_idx: [u8; 7], primary_ref_frame, order_hint, seg_info: &AV1Segmentation,
  film_grain_info: &AV1FilmGrain, ..., pic_info_fields: &AV1PicInfoFields, ...,
  loop_filter_info_fields: &AV1LoopFilterFields, ..., qmatrix_fields: &AV1QMatrixFields,
  mode_control_fields: &AV1ModeControlFields, ..., loop_restoration_fields:
  &AV1LoopRestorationFields, wm: &[AV1WarpedMotionParams; 7])` (`av1.rs:388-512`, the full
  40-parameter constructor) — **every one of these sub-structs is a mandatory, non-`Option`
  parameter**, including `seg_info`/`film_grain_info`/`loop_restoration_fields`/`wm` — confirming
  this ADR's own "reject if the stream signals a disabled tool as enabled" scope must still
  **construct real, correctly-all-disabled** versions of every one of these structs for every
  `KEY_FRAME` this crate does decode (the driver needs a real, spec-legal all-zero struct passed
  even when the corresponding sequence-header flag is off), exactly the same "never null/never
  omitted, even when disabled" discipline `vulkan::av1_params`'s own doc history already
  establishes for the *encode* side of this same codec (`build_segmentation`/`build_cdef`/
  `build_loop_restoration`/`build_global_motion`, all-disabled but real structs, never
  `Option::None`/null pointers).
- **No `PictureAV1`-with-flags wrapper type exists for AV1** — unlike H.264's `PictureH264`
  (`flags: u32` bitmask, `VA_PICTURE_H264_SHORT_TERM_REFERENCE` etc., this crate's own ADR-0002
  already confirmed `= 8u32`) or HEVC's presumed equivalent, `ref_frame_map`/`ref_frame_idx` here
  are **raw `VASurfaceID`/`u8` arrays with no flags field at all** — real libva's AV1 decode
  buffer addresses every reference purely by DPB-slot-index + surface ID, confirmed absent from
  this file. For this ADR's `KEY_FRAME`-only scope: `ref_frame_map = [VA_INVALID_ID; 8]`
  (`cros_libva::VA_INVALID_ID`, already used by this crate's own H.264 `invalid_picture_h264`-style
  convention), `ref_frame_idx = [0; 7]` (harmless — never dereferenced when every `ref_frame_map`
  entry is invalid), `current_display_picture` set equal to `current_frame` (no film-grain-driven
  dual-surface-output case in this scope — see FFmpeg's own `vaapi_av1.c` film-grain comment,
  fetched this session, confirming that dual-output case is specifically a film-grain artifact
  this ADR's scope already excludes).
- `SliceParameterBufferAV1` (`av1.rs:519-562`) — **an array-of-tiles wrapper** (`Vec<VASliceParameterBufferAV1>`,
  submitted as a single multi-element buffer, same convention this crate's own H.264 path already
  uses for its own `SliceParameter::H264` wrapper via `Buffer::new`'s `nb_elements` special-case,
  `cros-libva`'s `buffer.rs:44-51`). This ADR's single-tile scope calls
  `add_slice_parameter` exactly once per frame: `slice_data_size`/`slice_data_offset` from the
  parsed tile's byte range within the OBU stream, `tile_row = tile_column = 0`, `tg_start =
  tg_end = 0`, `anchor_frame_idx` unused (`0` — anchor frames are a large-scale-tile / scalable
  feature this scope does not enable).
- Confirmed by FFmpeg's real `libavcodec/vaapi_av1.c` (fetched this session): the decoder
  populates `pic_param.ref_frame_map[i] = VA_INVALID_ID` explicitly for every slot on a shown
  `KEY_FRAME`, matching this ADR's own design above field-for-field; and its tile submission
  builds one `VASliceParameterBufferAV1` per tile within a tile group (this ADR's own single-tile
  scope needs only the trivial one-element case of that same loop).
- Profile: `VAProfileAV1Profile0` — same unconfirmed-bindgen-value disposition as this ADR's
  encoder sibling (§ Open questions). Entrypoint: `VAEntrypointVLD` — this one is **not** newly
  uncertain; it is the same entrypoint constant this crate's existing H.264 decode path already
  uses successfully in WSL2 compile verification (`h264.rs:140,153`), and VA-API decode has no
  "low power" entrypoint split the way encode does (confirmed absent from every codec's decode
  entrypoint set in real libva).

### `VaapiAv1Decoder` struct shape (ZCA sketch — ownership, no `Box`/`dyn`)

```rust
// linux/vaapi/av1/{obu,bits,sequence_header,frame_header,tile_info}.rs — new files, sans-io,
// #![forbid(unsafe_code)], no cros_libva types.
pub(super) struct SequenceHeader { seq_profile: u8, /* ...only the fields this scope needs... */ }
pub(super) struct FrameHeader { frame_width_minus1: u16, frame_height_minus1: u16, base_q_idx: u8, /* ... */ }
pub(super) fn parse_sequence_header(r: &mut BitReader) -> Result<SequenceHeader, DecodeError> { .. }
pub(super) fn parse_frame_header(r: &mut BitReader, seq: &SequenceHeader) -> Result<FrameHeader, DecodeError> { .. }

// linux/vaapi/av1.rs — new file, sibling of h264.rs, same shape.
struct Av1Pipeline {
    _config: Config,
    context: Rc<Context>,
    surface: Option<Surface<()>>, // single surface: no DPB, one KEY_FRAME decoded at a time
    coded_width: u32,
    coded_height: u32,
    nv12_format: VAImageFormat,
}

pub(crate) struct VaapiAv1Decoder {
    display: Rc<Display>,
    pipeline: Option<Av1Pipeline>,
    seq: Option<SequenceHeader>,
    info: StreamInfo,
    declared_width: u32,
    declared_height: u32,
    pending: VecDeque<VideoFrame>,
    flushed: bool,
}
```

Unlike this crate's own H.264 `Pipeline` (ADR-0002's `surfaces: Vec<Option<Surface<()>>>` sized
`max_num_ref_frames + 1`), `Av1Pipeline` needs **exactly one** surface — a `KEY_FRAME`-only
decoder references nothing, so there is no DPB ring to size; each `push_packet` call decodes into
the single surface, reads it back via the same `Image::create_from`/`vaGetImage` CPU-readback
path ADR-0001 already established for H.264, and the surface is immediately free for the next
frame. No `Box<dyn _>`/`dyn Trait` anywhere — matches every other decode backend in this
workspace.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Wait for a hypothetical future Vulkan or D3D12 AV1 decode port instead of deriving `uncompressed_header()` parsing largely from spec text | Rejected — no such port exists anywhere in this workspace today (see § Note above), and waiting indefinitely blocks this crate from gaining any AV1 decode capability. The in-repo D3D12 AV1 *encoder* (a real, spec-cited, inverse-direction artifact) already meaningfully de-risks the highest-risk part (conditional field presence per §5.9.2) without waiting for a decode-side precedent that does not exist. |
| Support `INTER_FRAME` decode in this same ADR, mirroring ADR-0002's H.264 same-pass DPB extension | Rejected — see § Why decode-side GOP support is a substantially larger lift than encode-side GOP support: no porting precedent exists for AV1 decode-side reference management (unlike H.264, where `vulkan::dpb.rs` was ready to port), and AV1's own CDF-forward-adaptation/9-way reference-slot model is intrinsically larger than what a `KEY_FRAME`-only encoder-side GOP extension needed to solve. A `KEY_FRAME`-only first increment is independently useful (keyframe/thumbnail extraction, same value H.264 ADR-0001's own IDR-only scope already provided) and independently verifiable. |
| Reuse `mediaway_sw::h264::{Sps, Pps}`-style types, or invent a new shared `mediaway_sw::av1` bitstream module, instead of a crate-local parser | Considered the shared-module option seriously (this ADR's parser is genuinely more spec-derived than usual, a stronger case for shared, more-reviewed code than H.264's local-parser precedent) but rejected for this pass: `mediaway_sw` is a cross-platform software-fallback crate whose own AV1 involvement today is `rav1e`-backed software *encode* (`mediaway-sw/adr/0002-rav1e-av1-encode.md`), not bitstream parsing; introducing a new `mediaway_sw::av1` module as this ADR's byproduct would fold a decode-specific, VA-API-parameter-buffer-shaped parser into a crate meant for broader reuse, before any second consumer exists to justify the shared-module design cost. Revisit if/when a second AV1 bitstream-parsing consumer appears (e.g. a future software AV1 decode path, or a Vulkan AV1 decode port). |
| Accept multi-tile streams in this pass (reject only tool-flag-signaled features, not tile count) | Rejected — multi-tile AV1 decode needs per-tile `VASliceParameterBufferAV1` entries plus tile-boundary-aware bitstream offset tracking (`tile_size_bytes_minus_1`-driven length parsing for all but the last tile, AV1 spec §5.11.1) that this scope's single-tile assumption entirely sidesteps; deferring it keeps this ADR's own parser simpler without narrowing real-world usefulness much (this crate's own encoders, and most compatible small-resolution content, already produce single-tile streams). |

## Consequences

### Positive

- A real, useful `KEY_FRAME`-only AV1 decode capability lands on this backend for the first time
  in this workspace on **any** backend for **decode** specifically (Vulkan/D3D12 have none at
  all) — a genuine capability gap this ADR closes, not just a VA-API-specific extension.
- The D3D12 AV1 encoder's inline per-field spec citations meaningfully cross-check this ADR's own
  parser design before a single line of Rust exists, even without a literal ported function to
  copy — a real, if unusual, risk-reduction path this ADR names honestly rather than overstating
  as a "port."
- This ADR's scope gives this workspace's own AV1 encoders (D3D12 today; VA-API once its sibling
  ADR's blocker resolves) a real round-trip decode target, closing a testing gap none of them
  currently have (today, AV1 encoder output can only be checked by system `ffprobe`/`ffmpeg`,
  never by this workspace's own code).
- Correctly, explicitly identifies (rather than silently deferring) why `INTER_FRAME` support is
  a meaningfully larger lift here than it was for H.264 in the same-crate-family ADR-0002 — a
  finding useful to whoever scopes that future follow-up.

### Negative / Trade-offs

- **No decode-side porting precedent exists for the hardest part of this ADR** (bitstream
  parsing) — meaningfully higher spec-derivation risk than every other ADR in this crate's
  `linux/vaapi/adr/` folder, which could all cite a real, hardware-verified sibling to port from.
  This ADR's cross-check against the D3D12 *encoder*'s inline comments reduces, but does not
  eliminate, that risk the way a true decode-side port would have.
- Zero real-hardware verification for this crate's AV1 path (same standing caveat as
  ADR-0001/0002), now covering a codec this workspace has broadly found AV1 hardware/driver
  support to be less mature for in general (`docs/roadmap.md`'s Vulkan AV1 encode entry).
- `KEY_FRAME`-only decode cannot play back any real-world AV1 GOP structure — only useful for
  all-intra streams, keyframe/thumbnail extraction, or this workspace's own `KEY_FRAME`-only
  encoder output, until a real follow-up (with its own, currently nonexistent, porting precedent)
  lands reference-frame support.
- `VAProfileAV1Profile0`'s real bindgen name/value is unconfirmed this session (same disposition
  as every prior ADR in this folder for a build-time-generated constant).

## Test plan (for the implementation pass that follows this ADR)

- **Sans-io, hardware-independent (highest-value, run first)**: new `av1/*_tests.rs` siblings —
  `leb128()` round-trip against hand-computed byte sequences (including multi-byte continuation
  cases); `uvlc()`/`su(n)`/`ns(n)` against hand-computed AV1-spec-example values;
  `parse_sequence_header`/`parse_frame_header` against a byte sequence **generated by this
  workspace's own `windows::bitstream_av1::build_av1_session_prefix`/
  `build_av1_frame_header_bytes`** for a small fixed resolution — the single most valuable
  regression test this ADR can specify, since it exercises this ADR's new parser against a
  real, independently-authored (if same-workspace) writer's output, not just hand-rolled bytes;
  rejection cases for every disabled-tool-signaled-as-enabled branch (segmentation, film grain,
  CDEF, restoration, superres, multi-tile).
- **`av1.rs` integration** (hardware-gated, `_or_skip_without_hw`-style, expected to skip in this
  session/CI): decode a `windows::bitstream_av1`-generated (or hand-constructed, if a real driver
  needs genuine compressed tile payload bytes this crate cannot itself produce without a real
  encoder) `KEY_FRAME` AV1 stream, assert NV12 output geometry matches the parsed sequence header.
- **Oracle validation**: pipe a real AV1 `KEY_FRAME` stream (ideally this workspace's own D3D12
  encoder output, or a small system-`ffmpeg`-generated fixture) through this new decoder and
  compare geometry/keyframe-count against `ffprobe` (this workspace's standing oracle,
  [ADR-0002](../../../../docs/adr/0002-system-oracle.md)).
- **WSL2 real-Linux compile verification**: confirms `VAProfileAV1Profile0`'s real name/value —
  this ADR's only unconfirmed VA-API-level constant (entrypoint `VAEntrypointVLD` is already
  proven-in-use by this crate's existing H.264 path, no new risk there).
- Default `cargo test --workspace` (no system FFmpeg, no VA-API hardware) must keep passing —
  every sans-io test above requires neither.

## Addendum (2026-08-19, confirmed via real WSL2 bindgen output)

Open question #2 is now closed. Real `cros-libva` bindgen output
(`target/x86_64-unknown-linux-gnu/debug/build/cros-libva-*/out/bindings.rs`):

```
pub const VAProfileAV1Profile0: Type = 32;
pub const VAProfileAV1Profile1: Type = 33;
pub const VAEntrypointVLD: Type = 1;
```

`cros_libva::VAProfile::VAProfileAV1Profile0` is the correct reference path (module-const shape,
same as every other profile this crate's H.264/HEVC `codec.rs` already reference).

Also independently confirmed while fact-checking the encode-side sibling's blocker (see that
ADR's own addendum): `cros_libva::BufferType` (`src/buffer.rs:299-322`) has exactly 10 variants —
`PictureParameter`, `SliceParameter`, `IQMatrix`, `Probability`, `SliceData`,
`EncSequenceParameter`, `EncPictureParameter`, `EncSliceParameter`, `EncMacroblockParameterBuffer`,
`EncCodedBuffer`, `EncMiscParameter` — no `PackedHeader` variant anywhere, confirming this ADR's
own § "Why AV1 decode needs no packed-header submission" finding is correct: decode has no
analogous gap since `PictureParameterBufferAV1`/`SliceParameterBufferAV1` are plain
`PictureParameter`/`SliceParameter` variants, unlike encode's packed-`frame_header_obu()`
requirement.

Open questions #1, #3, #4, #5 remain open — none are resolvable from bindgen output alone; they
need either a real driver, real AV1 test material, or the actual bit-level parser implementation.

## Open questions / risks (explicit, for whoever picks up the implementation pass)

1. **The entire bitstream parser is spec-derived, not ported** — the single highest-priority
   residual risk this ADR carries relative to every other ADR in this folder. Mitigated (not
   eliminated) by the D3D12 encoder cross-check (§ Why this ADR cannot be a pure port) and by
   this ADR's own test plan's self-generated-fixture round-trip.
2. **`VAProfileAV1Profile0`'s real bindgen name/value** — same unconfirmed-build-time-constant
   disposition as every prior ADR in this folder, and as this ADR's `mediaway-encoder` sibling.
3. **Whether real AV1 decode drivers actually reject (rather than silently misbehave on) a
   `KEY_FRAME` picture-parameter buffer whose optional-tool sub-structs are all-disabled-but-real
   while the *sequence header* itself also signals those tools off** — this ADR assumes drivers
   validate consistency between the sequence header's `enable_*` flags and the per-picture
   struct's own flags/values, matching every other codec's VA-API decode convention this crate
   already relies on, but not independently confirmed for AV1 specifically this session.
4. **`current_display_picture` vs `current_frame` for a `KEY_FRAME`** — this ADR sets them equal
   (no film grain, no scalability in scope), inferred from FFmpeg's `vaapi_av1.c` film-grain-only
   dual-output comment (fetched this session) rather than an explicit "these are always equal
   when film grain is off" statement quoted directly from that source.
5. **Whether `INTER_FRAME`/GOP decode support should eventually be designed against a *new*
   from-scratch spec derivation, or wait for a Vulkan/D3D12 AV1 decode port this workspace has not
   started** — explicitly left open per § Why decode-side GOP support is a substantially larger
   lift, not resolved by this ADR.

## References

- [ADR-0001](0001-vaapi-h264-cpu-out.md) · [ADR-0002](0002-vaapi-h264-p-slice-dpb.md) — this
  crate's H.264 IDR-only/P-slice precedent, porting-methodology template, and the
  `#![forbid(unsafe_code)]` crate-root convention this ADR's new modules also follow
- `crates/mediaway-decoder/src/linux/vaapi/{codec,h264,sps,pps,slice,dpb,nv12}.rs` — current
  H.264-only implementation this ADR adds an AV1 sibling to
- `crates/mediaway-sw/src/h264/bitreader.rs` — `BitReader` reuse source (`read_bit`/`read_bits`,
  lines 34, 47), same reuse precedent ADR-0001 already established
- `crates/mediaway-encoder/src/windows/d3d12_video_encode/bitstream_av1.rs` — AV1 OBU writer,
  this ADR's primary (inverse-direction) cross-check source for field presence/order
  (`write_sequence_header`: lines 132-192; `write_frame_header`: lines 204-282; `write_tile_info`:
  lines 87-128)
- `crates/mediaway-encoder/adr/linux/0005-vaapi-av1-key-frame-and-inter-gop.md` — this ADR's
  same-session encode-side sibling; source of the "no packed-header gap on the decode side"
  finding (§ Why AV1 decode needs no packed-header submission) and the shared § Note on this
  ADR's design brief correction
- `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cros-libva-0.0.13\src\buffer\av1.rs`
  — real vendored `cros-libva` 0.0.13 source read directly for `PictureParameterBufferAV1`/
  `SliceParameterBufferAV1`/`AV1SeqFields`/`AV1Segmentation`/`AV1FilmGrain`/`AV1PicInfoFields`/
  `AV1LoopFilterFields`/`AV1WarpedMotionParams`/`AV1LoopRestorationFields`/`AV1ModeControlFields`/
  `AV1QMatrixFields` signatures (lines 1-562 read in full this session);
  `src/buffer.rs:44-51` (`nb_elements` multi-element slice-parameter convention, already shared
  with this crate's own H.264 usage)
- FFmpeg `libavcodec/vaapi_av1.c` — fetched this session (`raw.githubusercontent.com`), confirmed:
  `ref_frame_map[i] = VA_INVALID_ID` convention for a shown `KEY_FRAME`, per-tile
  `VASliceParameterBufferAV1` submission shape, film-grain dual-surface-output caveat (out of
  this ADR's scope)
- `docs/roadmap.md` §2 — "AV1 decode has not been started" (workspace-wide), the finding this
  ADR's § Note on this ADR's design brief is built around
- [`docs/spec/sans-io.md`](../../../../docs/spec/sans-io.md) ·
  [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md) ·
  [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) ·
  [`docs/conventions/error-handling.md`](../../../../docs/conventions/error-handling.md) ·
  [`docs/adr/0002-system-oracle.md`](../../../../docs/adr/0002-system-oracle.md)

ADRs are **English**. Numbering is local to this `adr/` folder.
