# Vulkan Video AV1 decode (`mediaway-decoder::vulkan::av1_params`)

Parent page: [vulkan-decode.md](vulkan-decode.md). ADR:
[`0002-av1-decode-keyframe-first.md`](../../../../crates/mediaway-decoder/adr/vulkan/0002-av1-decode-keyframe-first.md).

**Status (2026-08-19): implemented and hardware-verified on the RTX 4090,
first attempt, no bug-fix round needed.** `frame_type == KEY_FRAME` /
`show_frame == 1` / single-tile pictures only — anything else (`INTER_FRAME`,
`show_existing_frame == 1`, multi-tile, film grain, super-resolution, 10-bit,
monochrome) is rejected with `DecodeError::Unsupported`/`InvalidInput`, not
silently mis-decoded.

## Files

`av1_params.rs` (OBU scan + sequence header), `av1_params/av1_frame_header.rs`
(`KEY_FRAME` `uncompressed_header()` parse — segmentation, quantization, loop
filter, CDEF, loop restoration, tile info, all real spec-faithful parsers,
not stubs), `av1_params/av1_frame_header/av1_frame_std.rs`
(`StdVideoDecodeAV1PictureInfo`/`StdVideoAV1*` struct construction — split
into three files, not ADR-0002's originally-planned two, to stay under this
workspace's 1000-line-per-source-file rule). `av1_refs.rs` (`Av1RefSlots`:
Vulkan-level slot occupancy/outstanding-handle bookkeeping only — **not** a
port of `dpb.rs`, no `order_hint` tracking, since a `KEY_FRAME` never reads a
reference). `decoder_av1.rs`/`session_command_av1.rs` mirror
`decoder_hevc.rs`/`session_command_hevc.rs`'s shape.

## No Annex-B start code — AV1 has none

Unlike H.264/HEVC, `src_buffer` holds only the `OBU_FRAME`'s payload bytes
(after `obu_header()`/`leb128` size field) — no prepended framing. This is a
real design decision this crate made **without** a cross-checked reference
implementation (no working AV1 Vulkan decoder in another project was
available to diff against, unlike H.264/HEVC's FFmpeg-confirmed offset
conventions):

- `frameHeaderOffset` is always `0`.
- The single tile's `pTileOffsets`/`pTileSizes` entry is computed from
  `BitReader::bits_read()`'s position at the end of `uncompressed_header()`,
  rounded up to a byte boundary — `tile_group_obu()`'s own leading
  `tile_start_and_end_present_flag`/`byte_alignment()` are no-ops when
  `NumTiles == 1` (AV1 spec § 5.11.1), so no bits separate the two.

## `rav1e`'s real OBU shape (confirmed by byte inspection, not assumed)

A temporary instrumented test (reverted before finishing) dumped real
`mediaway_sw::av1::Av1Encoder` output: `OBU_TEMPORAL_DELIMITER` +
`OBU_SEQUENCE_HEADER` + one combined `OBU_FRAME` (frame header and tile
group together) — never split `OBU_FRAME_HEADER` + `OBU_TILE_GROUP`. The
real sequence header has `enable_cdef = 1`; the real frame header has
`segmentation_enabled = 1` — **not** the all-disabled shape this crate's own
AV1 Vulkan **encoder** (`mediaway-encoder::vulkan::av1_params`) produces.
This is why the frame-header parser had to be a real, spec-faithful
implementation of segmentation/CDEF/loop-filter/loop-restoration parsing —
a stub would not have decoded this test's own real bitstream at all.

## AV1 decode does not share AV1 encode's driver-maturity wall

This workspace's own AV1 Vulkan **encode** path is confirmed producing
invalid OBU output on this same RTX 4090 (a real driver-maturity limitation,
cross-checked against `ffmpeg`'s own `av1_vulkan` encoder failing the same
way). AV1 **decode** does not share this bug: `tests/vulkan/hardware_av1_decode.rs`
pushes a real `Av1Encoder`-produced `KEY_FRAME` (flat mid-gray 256×192 I420)
through `VulkanVideoDecoder` and passes **hard** content assertions (every
decoded luma byte nonzero, center sample exactly `128`) on the first attempt
— no protocol bug needed fixing, unlike every other codec/direction this
crate has hardware-verified (H.264 decode needed three real bugs fixed;
HEVC decode needed one).

## Struct field naming gotcha

`StdVideoDecodeAV1PictureInfo`/`StdVideoDecodeAV1ReferenceInfo` mix
`snake_case` and `PascalCase`/`camelCase` field names in the same struct
(bindgen preserved the C header's own inconsistent naming verbatim, e.g.
`frame_type` next to `OrderHint`, `refresh_frame_flags` next to
`SkipModeFrame`/`TxMode`) — every read/write site carries its own
item-scoped `#[allow(non_snake_case)]`, never a blanket crate-wide allow.

## Scope not attempted this round

`INTER_FRAME`/`INTRA_ONLY_FRAME`/`SWITCH_FRAME`, multi-tile, film grain
(architecturally excluded — forces `DPB_AND_OUTPUT_DISTINCT`, incompatible
with this crate's `COINCIDE`-only image design), super-resolution,
10/12-bit, monochrome, `frame_id_numbers_present_flag == 1`. A future
general-GOP increment needs a real `order_hint`/reference-name-slot-mapping
design in `av1_refs.rs` (deliberately not built this round) and a
`show_existing_frame` "no decode call, re-output an existing DPB slot" path
(structurally different from every NAL/OBU type this crate has handled).

## Related

- [vulkan-decode.md](vulkan-decode.md) — parent page (H.264/HEVC + shared
  probe/bindings context)
- [vulkan-encode](vulkan-encode.md) — AV1 Vulkan encode's own confirmed
  driver-maturity wall
