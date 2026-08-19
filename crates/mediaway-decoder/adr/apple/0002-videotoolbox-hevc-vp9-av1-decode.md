# ADR-0002: `VideoToolbox` decode multicodec expansion — HEVC, VP9, AV1

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (module `mediaway-decoder::apple`)

## ⚠️ Zero real-hardware / zero compile verification in this session

Same structural constraint as [ADR-0001](0001-videotoolbox-h264-cpu-out.md): this repo's dev
environment (Windows host) cannot cross-compile Apple code at all. Every API name/signature/
constant cited below is a **direct read** of the locally cloned
[`objc2`](https://github.com/madsmtm/objc2) checkout
(`local/vendor-ref/objc2/framework-crates/objc2-video-toolbox/src/generated/`,
`local/vendor-ref/objc2/generated/CoreMedia/CMFormatDescription.rs`), not a paraphrase from
memory or web search.

## Context

This crate's `apple` decode module supported H.264 only (ADR-0001). This ADR extends it to the
other three video codecs this workspace already has multicodec decode support for on other
platforms (VA-API, Vulkan, D3D12): HEVC, VP9, AV1. Unlike those backends — which are low-level
session APIs requiring this crate to parse SPS/PPS/slice headers and build picture-parameter
buffers by hand — `VTDecompressionSession` stays a black box for every codec here exactly as
ADR-0001 already established for H.264: this crate never builds a DPB or reference-picture list
for HEVC/VP9/AV1 either.

### Two different construction shapes, confirmed via direct source reads

`local/vendor-ref/objc2/generated/CoreMedia/CMFormatDescription.rs` exposes two distinct format-
description construction paths:

- **`CMVideoFormatDescriptionCreateFromHEVCParameterSets`** — the HEVC analog of H.264's own
  `...FromH264ParameterSets` (confirmed real, same file): "parameter sets' data can come from raw
  NAL units and must have any emulation prevention bytes needed... at least one of each parameter
  set must be provided", accepting VPS (32) + SPS (33) + PPS (34). HEVC therefore reuses
  ADR-0001's exact shape — in-band or `extra_data`-supplied parameter sets, lazy session creation.
- **No such per-frame-parameter-set entry point exists for VP9 or AV1** anywhere in the generated
  bindings (confirmed by grepping the whole `objc2-video-toolbox`/`objc2-core-media` generated
  tree for `VP9`/`AV1` construction functions — none exist beyond the plain
  `kCMVideoCodecType_VP9`/`kCMVideoCodecType_AV1` codec-type constants in
  `CMFormatDescription.rs`). The only construction path for these two codecs is the **generic**
  `CMVideoFormatDescriptionCreate(allocator, codecType, width, height, extensions,
  &mut out)`, with the codec-specific config record (`vpcC` for VP9 per the VP Codec ISO Media
  File Format Binding, `av1C` for AV1 per the AV1 Codec ISO Media File Format Binding) supplied
  via the `extensions` dictionary's `kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms`
  key (confirmed real constant, same file) — a `CFDictionary` mapping the atom's four-character
  code to its raw `CFData` payload. This mirrors the real-world technique other VideoToolbox
  integrations (e.g. browser engines' VP9/AV1 hardware-decode paths) use for these two codecs.

This crate has **no VP9/AV1 bitstream parser of its own** (unlike `linux::vaapi`'s
`vp9::{header,color_config,...}` and `linux::vaapi::av1::{obu,frame_header,...}` modules, which
exist because VA-API's session API genuinely needs full picture parameters). Rather than write a
third from-scratch VP9/AV1 parser purely to synthesize a `vpcC`/`av1C` record VideoToolbox itself
never asks this crate to derive semantically, this backend requires the **container** to already
supply one in [`VideoDecoderConfig::extra_data`] — see § Decision.

## Decision

> HEVC reuses ADR-0001's shape exactly (dedicated parameter-set entry point, in-band or
> `extra_data`-supplied VPS/SPS/PPS, lazy session creation). VP9/AV1 use the generic
> `CMVideoFormatDescriptionCreate` + `SampleDescriptionExtensionAtoms` extension, **requiring**
> `VideoDecoderConfig::extra_data` to already hold a valid `vpcC`/`av1C` record **at `open()`**
> (`DecodeError::Unsupported` if empty) — no in-band lazy discovery, no bitstream parsing to
> synthesize a missing record.

### Module layout

New file `src/apple/videotoolbox/format_desc.rs` holds all per-codec `CMFormatDescription`
construction (`create_h264`, `create_hevc`, `create_raw`) — factored out of `video.rs` (which
would otherwise grow past this workspace's 1000-line source-file limit) so `video.rs` stays
session/callback plumbing shared unchanged across all four codecs (`ensure_session`, the output
callback, block/sample buffer creation, NV12 readback — none of that differs by codec).
`codec.rs` (the pure, no-`objc2-*`-dependency helper module ADR-0001 established) gains
`is_supported_video_codec` (now H264/Hevc/Vp9/Av1), `requires_extra_data_at_open`, `raw_atom_key`,
and `validate_hevc_parameter_sets` (the HEVC analog of ADR-0001's `validate_parameter_sets`).

### HEVC byte framing — new `iso_bmff::bitstream::hevc`, mirroring `avc` exactly

`iso_bmff::bitstream::avc` (H.264 Annex-B ↔ AVCC, ADR-0001 reused it unchanged) has no HEVC
counterpart anywhere in this workspace — every other HEVC backend (`linux::vaapi::hevc_nal`,
`windows::d3d12_video_decode::hevc_vps_sps_pps`) writes its own **semantic** SPS/PPS field parser
for its own picture-parameter needs, not a reusable NAL-framing/`hvcC`-box helper. This ADR adds
`iso_bmff::bitstream::hevc::{to_hvcc, parse_hevc_decoder_config, annex_b_sequence_header,
hvcc_payload_to_annex_b}`, structurally identical to `avc.rs` generalized from one NAL-header byte
to HEVC's two and from 2 parameter-set types to 3 (VPS/SPS/PPS) — a genuinely reusable crate
addition (any future HEVC container/mux work needs the same `hvcC` box shape), not code private to
this decode backend. `hvcC`'s `general_profile_space`/`tier`/`profile_idc`/
`profile_compatibility_flags`/`constraint_indicator_flags`/`level_idc` fields are copied verbatim
from the SPS's `profile_tier_level()` general fields — byte-aligned at a fixed offset (ITU-T
H.265 § 7.3.3: 2-byte NAL header + 1-byte `sps_video_parameter_set_id`/
`sps_max_sub_layers_minus1`/`sps_temporal_id_nesting_flag`, then the 12-byte general
`profile_tier_level` fields land byte-aligned with no exp-golomb parsing needed) — the same "copy
the known fixed-position bytes" technique `avc.rs`'s `build_avcc` already uses for H.264's
`profile_idc`/`constraint_flags`/`level_idc`. Fields that sit past exp-golomb-coded SPS syntax
(`chroma_format_idc`, `bit_depth_*`, `min_spatial_segmentation_idc`, `avgFrameRate`,
`numTemporalLayers`) are left at documented safe defaults (4:2:0, 8-bit, one temporal layer) —
mirroring `av1.rs`'s `to_av1c` identical "informational fields default until verified against a
real encoder" precedent, not a new pattern.

### VP9/AV1 — no bitstream parsing, container-supplied config record only

`format_desc::create_raw(codec_type, width, height, atom_key, atom_payload)` wraps
`atom_payload` (`config.extra_data`, verbatim, unparsed) as a `CFData` under a one-entry
`CFDictionary<CFString, CFType>` (`{"vpcC" | "av1C": data}`), then that dictionary under
`kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms` in the outer extensions
dictionary passed to `CMVideoFormatDescriptionCreate`. The outer dictionary is built as
`CFDictionary<CFString, CFType>` and cast to `CFDictionary<CFString, CFPropertyList>` via
`CFRetained::cast_unchecked` — confirmed real toll-free bridging (`CFType`/`CFPropertyList` are
both phantom marker types any CF property-list-compatible value, including `CFDictionary` itself,
conforms to; `CMVideoFormatDescription::from_hevc_parameter_sets`'s own `extensions` parameter
already makes this exact assumption for its typed signature). `av1C` reuses this crate's existing
`iso_bmff::bitstream::av1::to_av1c` output shape (same box layout, ISO Media File Format Binding
§ 2.3.3) when a caller builds one; this backend does not itself construct a `vpcC`/`av1C` — see
§ Alternatives Considered for why not.

### Per-packet handling

- H.264/HEVC: `to_avcc`/`to_hvcc` Annex-B → 4-byte length-prefixed conversion per packet
  (unchanged from ADR-0001 for H.264; HEVC identical shape).
- VP9/AV1: **no NAL framing at all** — VP9 has no NAL structure, and AV1's OBU stream is passed
  through byte-for-byte; the payload goes directly into the `CMBlockBuffer` (the same
  `create_block_buffer` helper ADR-0001 already built, codec-agnostic — it only ever wraps
  arbitrary bytes). The session must already exist from `open()` (`requires_extra_data_at_open`);
  `push_packet` returns `DecodeError::Backend` in the unreachable case where it does not (this
  would indicate an `open()` invariant violation, not a normal runtime condition).

## Scope (this stage)

**In:**

- HEVC decode: general GOP (VideoToolbox-managed DPB, same as H.264), in-band or `extra_data`
  VPS/SPS/PPS, exactly one of each, 4-byte `hvcC` length-prefix size only (mirrors ADR-0001's
  H.264 constraints via `validate_hevc_parameter_sets`).
- VP9/AV1 decode: `extra_data` (`vpcC`/`av1C`) **required** at `open()`; no in-band lazy
  discovery, no bitstream parsing.
- `iso_bmff::bitstream::hevc` — new, reusable, workspace-level HEVC Annex-B/`hvcC` helper.

**Out (deferred, same as ADR-0001 unless noted):**

- Zero-Copy `CVPixelBuffer`/`IOSurface` output (`GpuBufferHandle::Metal`) for any codec.
- VUI-based `ColorRange`/`Full` detection.
- Multiple VPS/SPS/PPS, non-4-byte HEVC length sizes, HEVC RExt/SCC/multiview profiles.
- Deriving a `vpcC`/`av1C` from the raw VP9/AV1 bitstream when the container did not supply one —
  a real capability gap (streams whose container omits it cannot be opened by this backend), not
  silently worked around.
- `mediaway-decoder`'s `auto`/`capability` wiring for HEVC/VP9/AV1 specifically stays out of this
  ADR's scope; the workspace-level `mediaway::platform` wiring (covering all four codecs at once)
  is a separate change (`crates/mediaway/src/platform.rs`).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Write a VP9/AV1 uncompressed-header parser in this module to synthesize `vpcC`/`av1C` from the first frame | Rejected — this crate already has zero VP9/AV1 bitstream code (unlike VA-API/Vulkan/D3D12, whose session APIs genuinely require full picture parameters this crate must parse). Writing one purely to backfill a config record VideoToolbox never asks this crate to derive semantically is real, unverified, zero-compile-checked complexity for a container-completeness gap real-world containers rarely leave open (MP4/WebM VP9/AV1 tracks normally carry `vpcC`/`av1C`). |
| Require VP9/AV1 `extra_data` shaped some other way (raw profile/bit-depth struct instead of the real `vpcC`/`av1C` box bytes) | Rejected — `vpcC`/`av1C` are exactly what `SampleDescriptionExtensionAtoms` expects verbatim; inventing a different shape would need a translation layer with no benefit, and would diverge from what containers actually hand this crate as `extra_data` elsewhere in the workspace. |
| Fold HEVC's Annex-B/`hvcC` helper into this crate privately instead of adding it to `iso_bmff` | Rejected — `iso_bmff` is the unprefixed freestanding core `avc.rs`/`av1.rs` already live in (per `docs/spec/crate-packaging.md`); a HEVC container/mux consumer elsewhere in the workspace will need the identical `hvcC` box shape, and duplicating it privately here would diverge from that precedent for no reason. |
| One `CMFormatDescription` construction function handling all four codecs via a big match | Rejected — H.264/HEVC's dedicated entry points and VP9/AV1's generic-plus-extension-atom path have different parameter shapes (parameter-set pointer arrays vs. width/height/extensions); a single function would need an unreadable union of both shapes. Splitting into `format_desc::create_{h264,hevc,raw}` (this ADR's choice) keeps each function's `unsafe` FFI call auditable in isolation, matching this crate's existing per-concern file split. |

## Consequences

### Positive

- HEVC decode reaches full feature parity with H.264 (general GOP, same session/callback
  plumbing) at low incremental risk — it is a straightforward generalization of already-reviewed
  ADR-0001 code, not new design.
- VP9/AV1 decode is honest about its real constraint (container must supply the config record)
  rather than presenting a best-effort guess as full support — consistent with this workspace's
  "no silent guessing" convention (`linux::vaapi`'s AV1 backend made the same call for its own
  missing-sequence-header case).
- `iso_bmff::bitstream::hevc` is a genuine, reusable workspace addition, not backend-private code.
- `video.rs`'s shared plumbing (session creation, callback, block/sample buffer, NV12 readback)
  needed zero codec-specific changes — confirms ADR-0001's original design (VideoToolbox as an
  opaque black-box decoder) generalizes cleanly to more codecs.

### Negative / Trade-offs

- **Zero compile verification as authored** — carries over from ADR-0001 unchanged.
- VP9/AV1 cannot open a stream whose container omits `vpcC`/`av1C`, even though the raw bitstream
  itself may carry everything VideoToolbox would need — a real, documented capability gap versus
  a hypothetical from-scratch-parser backend.
- `hvcC`'s informational fields (`chroma_format_idc`, `bit_depth_*`, `avgFrameRate`,
  `min_spatial_segmentation_idc`) are safe defaults, not derived from the actual stream — same
  trade-off class ADR-0001's `av1.rs` precedent already accepted, now applied to a second box
  builder.
- `format_desc.rs`'s `CFDictionary<CFString, CFType>` → `CFDictionary<CFString, CFPropertyList>`
  cast relies on toll-free-bridging behavior confirmed by reading the generated bindings' own
  type signatures, not by a real compile/run — flagged, not proven, same posture as every other
  `unsafe` block in this crate.

## References

- [ADR-0001](0001-videotoolbox-h264-cpu-out.md) — original H.264 scope, session lifecycle,
  callback design, byte framing, all reused unchanged by this ADR
- `crates/iso-bmff/src/bitstream/avc.rs` — the structural template `bitstream/hevc.rs` mirrors
- `crates/iso-bmff/src/bitstream/av1.rs` — `to_av1c`'s "defer informational fields" precedent,
  reused for `hvcC`'s equivalent fields
- `mediaway-decoder` [ADR-linux/0004](../linux/0004-vaapi-vp9-key-frame-and-inter-decode.md),
  [ADR-linux/0005](../linux/0005-vaapi-av1-key-frame-decode.md) — this workspace's from-scratch
  VP9/AV1 bitstream parsers, the contrast baseline for why this backend does not write a third one
- Local grounding source (read directly): `local/vendor-ref/objc2/generated/CoreMedia/
  CMFormatDescription.rs` (`CMVideoFormatDescriptionCreate`,
  `CMVideoFormatDescriptionCreateFromHEVCParameterSets`,
  `kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms`,
  `kCMVideoCodecType_{HEVC,VP9,AV1}`), `local/vendor-ref/objc2/framework-crates/
  objc2-core-foundation/src/{data.rs,string.rs,dictionary.rs}`
- ISO/IEC 14496-15 § 8.3.3.1.2 (`HEVCDecoderConfigurationRecord`) — `hvcC` box layout
- `README.md` § Codec support — Apple decode HEVC/VP9/AV1 cells: `👻` → `🆗` once implemented
  (implemented/compiles, not hardware-verified)

ADRs are written in **English**.
