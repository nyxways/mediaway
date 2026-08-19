# ADR-0006: `VideoToolbox` ProRes decode; ProRes RAW permanent non-support

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (module `mediaway-decoder::apple`)

## ⚠️ Zero real-hardware / zero compile verification in this session

Same constraint as every other Apple ADR this session; see the companion
[encoder ADR-0006](../../../mediaway-encoder/adr/apple/0006-videotoolbox-prores-encode.md) § header
for the full caveat and the six-`CodecKind`-variants-not-one-plus-a-profile-field rationale, shared
verbatim by this ADR.

## Context

This crate's Apple decoder already supports two structurally different construction paths:
H.264/HEVC (dedicated `CMVideoFormatDescriptionCreateFrom{H264,HEVC}ParameterSets` entry points,
parameter sets parsed from the bitstream) and VP9/AV1 (generic `CMVideoFormatDescriptionCreate`
plus a container-supplied `vpcC`/`av1C` config-record extension atom, ADR-0002). ProRes needs a
**third** shape: the generic `CMVideoFormatDescriptionCreate` path like VP9/AV1, but with **no**
extension atom at all — confirmed no ProRes-specific `CMFormatDescriptionExtension` key exists
anywhere in the local `CMFormatDescription.rs`/`CMFormatDescription` bindings; geometry alone
(`width`/`height`) plus the codec-type constant fully describes a ProRes stream to
`VTDecompressionSession`.

### `format_desc::create_raw_no_extension` — new, not a reuse of `create_raw`

`create_raw` (VP9/AV1) requires a non-empty `atom_payload` and always builds a
`SampleDescriptionExtensionAtoms` extensions dictionary. Rather than making `atom_payload`
`Option` and threading a "build extensions or not" branch through one function, this ADR adds a
separate, simpler `create_raw_no_extension(codec_type, width, height)` that calls
`CMVideoFormatDescription::new(None, codec_type, width, height, None, &mut format_desc_out)` —
`extensions: None`. Two small, single-purpose functions over one function with an internal branch
and an unused-`Option` parameter for the majority of its callers.

## Decision

> `codec::is_supported_video_codec` gains the six `CodecKind::ProRes*` variants; a new
> `codec::is_prores` helper (mirrors the companion encoder ADR's identical helper) marks them.
> `format_desc::raw_codec_type` gains the six `kCMVideoCodecType_AppleProRes*` mappings alongside
> VP9/AV1's existing two. `open()` gains a third branch, checked **before** the existing
> `!extra_data.is_empty()` branch and **not** gated by `requires_extra_data_at_open` (which stays
> VP9/AV1-only — ProRes needs no config record, so it is deliberately never added there): when
> `is_prores(config.codec)`, the session is built **eagerly and unconditionally** from
> `config.width`/`config.height` alone via `create_raw_no_extension`, regardless of whether
> `config.extra_data` happens to be empty or not (any caller-supplied `extra_data` for a ProRes
> config is simply never read — a config record ProRes has no concept of). `push_packet`'s
> per-codec framing `match` gains a `CodecKind::ProRes*` arm mirroring the VP9/AV1 arm exactly:
> raw byte-for-byte payload pass-through (`Bytes::copy_from_slice`), no NAL/parameter-set framing,
> session-already-exists assumed (built at `open()`).

### CPU readback still forces NV12 — a real, disclosed lossy step for ProRes specifically

`validate()`'s existing `config.pixel_format != PixelFormat::Nv12 → Unsupported` check is
unchanged and applies to ProRes too. Unlike H.264/HEVC (whose native decode output is already
8-bit 4:2:0), a ProRes source's native bit depth/chroma sampling (up to 12-bit, 4:2:2 or 4:4:4
depending on profile) is downsampled to NV12 8-bit 4:2:0 during `VTDecompressionSession`'s
internal CPU-readback conversion — a real quality loss specific to decoding *into* this backend's
CPU output path, not present for [`VideoOutputPreference::ZeroCopyGpu`] (which hands out whatever
native `CVPixelBuffer` format VideoToolbox itself decoded into, untouched). This is the same
existing `pixel_format` field/check every other codec already goes through — not a new gap this
ADR introduces, but the quality cost is more visible for ProRes than for H.264/HEVC given ProRes's
whole reason for existing is higher fidelity than 8-bit 4:2:0.

## Scope (this stage)

**In:** ProRes 422 Proxy / 422 LT / 422 (standard) / 422 HQ / 4444 / 4444 XQ decode to CPU NV12
(lossy readback, see above) or Zero-Copy `GpuBufferHandle::Metal` (native format, no conversion —
reused unchanged from ADR-0003), general "black-box DPB" posture inherited from
`VTDecompressionSession` (trivially true for ProRes since every frame is independently decodable —
no actual DPB/reference-picture management ever happens).

**Out (permanent, not deferred):** ProRes RAW / RAW HQ decode — see the companion encoder ADR's
identical conclusion; RAW decode output is fundamentally different data (pre-demosaic sensor
values via a separate `VTRAWProcessingSession` API this backend does not implement), not a
`CVPixelBuffer` this crate's existing `VideoFrame`/`PixelFormat` model can represent without a much
larger, separate piece of work.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Make `format_desc::create_raw`'s `atom_payload` an `Option<&[u8]>` and skip the extensions dictionary when `None`, reusing one function for VP9/AV1/ProRes | Rejected — `create_raw`'s callers (VP9/AV1) always have a real payload; threading an `Option` through only for ProRes's benefit adds a branch and an always-true-for-existing-callers invariant to check, for no real code reuse (the extensions-dictionary-building block is the only part skipped, a handful of lines). A second small function is clearer. |
| Gate ProRes through `requires_extra_data_at_open` too, treating an empty `extra_data` as "use defaults" | Rejected — `requires_extra_data_at_open` exists specifically to *require* a container-supplied record before `open()` succeeds; ProRes's contract is the opposite (never needs one), so reusing that flag would be a confusing double meaning for the same function name. |

## Consequences

### Positive

- Reuses the entire session-creation/callback/Zero-Copy/CPU-readback machinery this crate already
  has for H.264/HEVC/VP9/AV1 — only `open()`'s dispatch and `push_packet`'s framing `match` needed
  new arms.
- `create_raw_no_extension` is small, single-purpose, and independently testable in shape (though
  untestable for real behavior without Apple hardware, like everything else this stage).

### Negative / Trade-offs

- **Zero compile verification as authored** — carries over unchanged.
- CPU-readback ProRes decode is lossy relative to the source's native bit depth/chroma sampling —
  disclosed above, not hidden.
- ProRes RAW decode remains permanently out of reach through this backend's existing `VideoFrame`
  model.

## References

- `mediaway-encoder` [ADR-apple/0006](../../../mediaway-encoder/adr/apple/0006-videotoolbox-prores-encode.md) —
  companion encode-direction ADR from the same session; full `CodecKind`-variants-vs-profile-field
  rationale lives there, shared verbatim.
- `mediaway-decoder` [ADR-apple/0002](0002-videotoolbox-hevc-vp9-av1-decode.md) — the VP9/AV1
  "generic `CMVideoFormatDescriptionCreate` + extension atom" precedent this ADR's `create_raw_no_extension`
  is a sibling of.
- `mediaway-decoder` [ADR-apple/0003](0003-videotoolbox-metal-zero-copy-decode.md) — Zero-Copy
  output, reused unchanged (native-format handle, no NV12 downsampling).
- Local grounding source (read directly): `local/vendor-ref/objc2/generated/CoreMedia/
  CMFormatDescription.rs` (`kCMVideoCodecType_AppleProRes*`, full-file grep for any ProRes-specific
  extension key — none found).
- `README.md` § Codec support — new ProRes row, Apple column: `🛠️` → `🆗`.

ADRs are written in **English**.
