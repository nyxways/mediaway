# Vulkan Video decode (`mediaway-decoder::vulkan`)

Module: `mediaway-decoder::vulkan` (portable — not OS-suffixed, sibling of
`mediaway-encoder::vulkan`). ADR:
[`0001-vulkan-video-decode.md`](../../../../crates/mediaway-decoder/adr/vulkan/0001-vulkan-video-decode.md).

**Status (2026-08-05): H.264 general-GOP decode and HEVC IDR decode are both
real and hardware-verified.** H.264 was the first general-GOP (P/B + DPB)
decode backend in this workspace to pass a real hardware round trip; HEVC's
IDR-only GPU path is now hardware-verified too (root cause found — see
below). AV1 is a follow-up addendum (files not written yet).

## Scope — broader than the encoder-vulkan precedent, by project-owner decision

This ADR's design covers all three codecs (H.264/HEVC/AV1) and general
P/B-frame GOP support (DPB + reference-picture-list management, not
IDR-only/all-intra) from the start.

## Binding survey: `vulkanalia` decode structs

`vulkanalia` 0.35.0 (already workspace-pinned) has every per-codec decode
picture-info + DPB-slot struct — **zero new dependency**, unlike encode's
AV1 story (`ash` lacked bindings, forced a migration). `GpuBufferHandle::Vulkan`
already existed in `mediaway-common` — no common-crate change needed for
Zero-Copy output. HEVC/AV1 `ProfileInfoKHR` structs are inferred present
from the consistent naming pattern, not yet individually confirmed.

## Stage 0 probe: real, hardware-verified, positive

Both the reference RTX 4090 **and** the Intel UHD 770 advertise
`VK_VIDEO_CODEC_OPERATION_DECODE_H264_BIT_KHR`/`_H265_BIT_KHR`/`_AV1_BIT_KHR`
decode queue families — the ADR's #1 open risk, now answered for real. A
genuine positive result, unlike the equivalent encode probe finding no
Intel encode queue at all.

## H.264: implemented, unit-tested, hardware-verified

`src/{probe,session,dpb,h264_params,h264_slice,session_command,
session_command_h264,cpu_readback,zero_copy,decoder}.rs` implement real SPS/
PPS/slice-header parsing, sliding-window DPB, `RefPicList0` construction, and
a persistent multi-frame `VulkanVideoDecoder` session. 43 pure sans-io unit
tests pass (dpb, parsing, POC-MSB derivation); `cargo check`/`clippy` clean.

**Root cause of the initial "submits fine, output unchanged" bug — found and
fixed**, by diffing against FFmpeg's `vulkan_decode.c`/`vulkan_h264.c`
field-by-field: (1) the setup slot's `slotIndex` in
`vkCmdBeginVideoCodingKHR`'s `pReferenceSlots` must be `-1`, not its real
index; (2) the destination DPB layer needs `VIDEO_DECODE_DST_KHR` layout
during the decode command, not permanent `VIDEO_DECODE_DPB_KHR`; (3, the fix
that mattered) the uploaded bitstream needs a real 3-byte Annex-B start code
prepended — without one, decode ran error-free but found nothing.
`cargo test -p mediaway-decoder --test hardware_h264_decode` passes
with **hard** pixel-value assertions: real IDR decode, real `P_Skip`
motion-compensated DPB reference read, real new P-frame content.

Real gaps found only by implementing: `StdVideoDecodeH264PictureInfo` has no
ref-list field (hardware parses ref lists from raw bits itself); the DPB
image is one shared `2D_ARRAY` view, not per-slot views; slice-header
parsing was missing `num_ref_idx_active_override_flag` and deblocking-
filter-control fields until cross-checked against `ffmpeg`.

## HEVC (Stage 2): IDR decode hardware-verified; P/B still deferred

`hevc_params.rs`/`hevc_slice.rs`/`session_command_hevc.rs`/`decoder_hevc.rs`
add real VPS/SPS/PPS/slice-segment-header parsing (2-byte NAL header, new —
not reusable from H.264) + short-term RPS construction, 22 sans-io unit
tests (120 total for the crate). GPU path is IDR-only this round (P/B-slice
HEVC decode deferred — hand-constructing legal HEVC content needs a real
CABAC encoder, substantially riskier than H.264's `I_PCM`/CAVLC escape).
Hardware test chains this workspace's own verified `mediaway-encoder::vulkan`
HEVC encoder into the decoder (no hand-written CABAC needed).

Root cause of the all-zero symptom (two hardware hypotheses — level too low,
hardcoded PTL constraint bits — tested and ruled out first; see ADR-0001's
2026-08-05 addendum): `HevcPps::parse` stopped reading *before*
`pps_loop_filter_across_slices_enabled_flag`, which gates a conditional
`slice_loop_filter_across_slices_enabled_flag` slice-header bit (confirmed
against `FFmpeg`'s `hevcdec.c`) — desyncing the driver's CABAC parser one bit
before CTU data. `cargo test -p mediaway-decoder --test hardware_hevc_decode`
now passes with **hard** pixel-value assertions.

## Bitstream-parser reuse

H.264/HEVC both reuse `mediaway_sw::h264::split_annex_b` (codec-agnostic
framing) + `BitReader`, but each writes its own NAL-header/high-level-syntax
parser (1-byte vs. 2-byte header, different layout) — no shared `Sps`/`Pps`.
AV1 has zero reusable parsing code in this workspace (`mediaway_sw::av1` is
a `rav1e` **encoder**, not a parser) — needs a from-scratch OBU scanner.
HEVC/AV1 parsers stay local to this crate (no second consumer yet to justify
extracting into `mediaway-sw`). AV1 film-grain synthesis is explicitly
deferred past base decode (see ADR-0001).

## Related

- [vulkan-encode](vulkan-encode.md) — sibling encode crate, structural
  precedent this ADR mirrors throughout
- [linux-decode](linux-decode.md) — IDR-only VA-API decode precedent,
  bitstream-parser-reuse pattern this ADR extends to HEVC/AV1
- [decode/scaffold](../decode/scaffold.md)
