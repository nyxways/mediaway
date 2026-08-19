# Apple ProRes (`VideoToolbox` `VTCompressionSession`/`VTDecompressionSession`)

ADRs: [`mediaway-encoder` ADR-apple/0006](../../../../crates/mediaway-encoder/adr/apple/0006-videotoolbox-prores-encode.md),
[`mediaway-decoder` ADR-apple/0006](../../../../crates/mediaway-decoder/adr/apple/0006-videotoolbox-prores-decode.md)
— both **Accepted, zero compile verification** (same posture as every other Apple backend this
session). 6 profiles: 422 Proxy / 422 LT / 422 (standard) / 422 HQ / 4444 / 4444 XQ. **ProRes RAW
is permanently unsupported** — see below.

## Six `CodecKind` variants, not one `ProRes` + a profile field

`ProRes422Proxy`/`ProRes422Lt`/`ProRes422`/`ProRes422Hq`/`ProRes4444`/`ProRes4444Xq` (discriminants
13-18, `mediaway-common`) — each is a distinct, permanent FourCC identity (`apco`/`apcs`/`apcn`/
`apch`/`ap4h`/`ap4x`), the same class of distinction as `H264` vs `Hevc`. A single `CodecKind::
ProRes` + a `prores_profile` field on `VideoEncoderConfig`/`VideoDecoderConfig` was rejected —
those structs are **not** `#[non_exhaustive]` and are built via full field-literal in dozens of
test files across both crates; a new field would break far more call sites than six enum variants
did (confirmed: only 2 files in the whole workspace have a truly exhaustive `CodecKind` match with
no wildcard — `mediaway-ffi/src/common/types.rs`, `mediaway-container/src/convert.rs`).

## ProRes is architecturally *simpler* than H.264/HEVC for this backend

Unconditionally all-intra, no in-band parameter sets, no `kVTProfileLevel_ProRes*` property (the
six `kCMVideoCodecType_AppleProRes*` codec-type constants fully encode the profile). Both crates'
existing session/callback/Zero-Copy/CPU-upload-or-readback machinery is entirely codec-agnostic
already — ProRes reuses **all of it**, only per-codec dispatch tables needed new arms.

## Encode (`videotoolbox::codec::{codec_type, is_prores}`)

- CPU input reuses `upload_cpu_nv12` unchanged (NV12 4:2:0 8-bit) — grounded in
  `AVAssetWriterInput.appendSampleBuffer:`'s doc comment ("If you are working with 8bit sources
  ProRes is also a good format... pixel buffers not in a natively supported format will be
  converted internally"). Flagged: this evidence is from `AVAssetWriterInput` (AVFoundation), not
  `VTCompressionSession` itself — adjacent-API evidence, not locally confirmed at the session-API
  level. Zero-Copy input reused unchanged too (ADR-0003).
- `configure_properties` skips `ProfileLevel`/`MaxKeyFrameInterval`/`AverageBitRate` entirely for
  ProRes — none apply (profile is baked into `codec_type`; all-intra; profile-fixed quality).
  `VideoEncoderConfig::gop_size`/`bitrate_bps` are **silently not honored** for ProRes, documented
  per those fields' own "backend must document its fallback" rustdoc contract.
- Fixed a real pre-existing bug while wiring extradata dispatch: `handle_output`'s
  `match shared.codec { Hevc => extract_hevc, _ => extract_h264 }` would have silently misapplied
  H.264 extraction to *any* future non-H.264/HEVC codec, not just ProRes. Now explicit
  (`H264 => extract_h264, Hevc => extract_hevc, _ => None`) — ProRes takes its own branch that
  marks `finalized_info` immediately (no parameter sets to extract, `extra_data` stays empty).

## Decode (`videotoolbox::format_desc::create_raw_no_extension`)

A **third** `CMFormatDescription` construction shape, alongside H.264/HEVC's parameter-set entry
points and VP9/AV1's extension-atom path (ADR-0002): `CMVideoFormatDescription::new(codec_type,
width, height, extensions: None)` — no config record needed at all (confirmed: no ProRes-specific
`CMFormatDescriptionExtension` key exists anywhere in the local bindings). `open()` builds the
session eagerly and unconditionally from geometry alone, checked *before* the existing
`!extra_data.is_empty()` branch; `push_packet` passes raw payload byte-for-byte, same as VP9/AV1.
CPU readback still forces NV12 output (`validate()`'s existing `pixel_format` check, unchanged) —
a real, disclosed lossy downsample from ProRes's native higher bit depth/chroma sampling, not
present for Zero-Copy output (native format, no conversion).

## ProRes RAW / RAW HQ — permanent, not deferred, and different from the VP9/AV1 gap

Two separate findings, not one: **encode** has zero references to either RAW codec type anywhere
in `VTCompressionSession`/`VTCompressionProperties` — RAW is camera-capture-hardware-produced only
(`AVCaptureDevice`), never a general compression-session target. **Decode** genuinely has a real
API (`kVTDecompressionPropertyKey_DecoderProducesRAWOutput`/`RequestRAWOutput`), but turning
decoded RAW into a viewable image needs a **separate** `VTRAWProcessingSession` API (demosaic/
white-balance/color-science parameter system) this backend does not implement — RAW decode output
is pre-demosaic sensor data, not a `CVPixelBuffer` this crate's `PixelFormat`/`VideoFrame` model
can represent without a much larger, separate piece of work.
