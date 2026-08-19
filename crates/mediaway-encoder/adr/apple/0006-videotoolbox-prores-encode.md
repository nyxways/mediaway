# ADR-0006: `VideoToolbox` ProRes encode; ProRes RAW permanent non-support

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (module `mediaway-encoder::apple`)

## ⚠️ Zero real-hardware / zero compile verification in this session

Same structural constraint as every other Apple ADR this session — see
[ADR-0002](0002-videotoolbox-hevc-encode.md) § header. New source read for this ADR:
`local/vendor-ref/objc2/generated/CoreMedia/CMFormatDescription.rs` (`kCMVideoCodecType_AppleProRes*`
constants), `local/vendor-ref/objc2/generated/VideoToolbox/VTCompressionProperties.rs` ("ProRes
hardware encoders currently prioritize speed over quality by default" — the only ProRes-specific
prose anywhere in the compression-properties file), `local/vendor-ref/objc2/generated/AVFoundation/
AVAssetWriterInput.rs` (pixel-format recommendation doc comment, flagged below as adjacent-API
evidence, not `VTCompressionSession`-specific).

## Context

`CodecKind` gained six new variants this session (`ProRes422Proxy`/`ProRes422Lt`/`ProRes422`/
`ProRes422Hq`/`ProRes4444`/`ProRes4444Xq`, discriminants 13-18, `mediaway-common`) rather than one
`CodecKind::ProRes` plus a separate profile field — see § Decision (profile) below for why.

### ProRes is, architecturally, simpler than H.264/HEVC for this backend

ProRes is **unconditionally all-intra** (every frame is independently decodable) and has no
in-band parameter-set concept (no SPS/PPS/VPS-equivalent) — the six codec-type constants
(`kCMVideoCodecType_AppleProRes422Proxy` … `kCMVideoCodecType_AppleProRes4444XQ`, confirmed,
`CMFormatDescription.rs`) are the *entire* format identity `VTCompressionSessionCreate` needs; no
`kVTProfileLevel_ProRes*` constant exists anywhere in the generated bindings (confirmed: grepped
every file under `VideoToolbox/` for "ProRes", only the one speed/quality-tradeoff doc comment in
`VTCompressionProperties.rs` matched — no profile-level, no GOP-shaping property). This backend's
existing `VTCompressionSession` infrastructure (session creation, `VTCompressionOutputCallback`,
`PixelBufferRef`/Zero-Copy input, `is_sync_sample` keyframe detection) is entirely codec-agnostic
already — ProRes reuses **all** of it unchanged; only `codec_type()`'s mapping and
`configure_properties()`'s per-codec property set needed new arms (see § Decision).

### Six `CodecKind` variants, not one `CodecKind::ProRes` + a profile field

Each ProRes flavor is a **distinct, permanent bitstream/container identity** (its own FourCC —
`apco`/`apcs`/`apcn`/`apch`/`ap4h`/`ap4x` — consumed by every ProRes-aware decoder/editing tool to
determine expected bit depth/chroma/quality tier), the same class of distinction that already
separates `CodecKind::H264` from `CodecKind::Hevc` rather than folding them into one "AVC family"
variant with a profile field. Confirmed low blast radius before choosing this: `CodecKind` is used
in 185 files across the workspace, but only two contain a **truly exhaustive** `match` over every
variant with no wildcard arm — `mediaway-ffi/src/common/types.rs` (the C-ABI `CodecKind ⇄
CommonCodecKind` conversion, 2 new arms × 2 directions) and `mediaway-container/src/convert.rs`
(ISOBMFF `Codec` mapping, added to the existing "not an ISOBMFF codec this crate writes" catch-all
group alongside `RawVideo`/`Mp3`/`Vorbis`/`Vp8`). Every other `match`/`matches!` site found via
`match (codec|self\.codec|config\.codec)` (31 files) already carries a wildcard or uses `matches!`
(never exhaustiveness-breaking). The alternative (`CodecKind::ProRes` + a new
`prores_profile: ProResProfile` field on the shared `VideoEncoderConfig`/`VideoDecoderConfig`
structs) was rejected specifically because those structs are **not** `#[non_exhaustive]` and are
constructed via full field-literal (no `..spread`) in dozens of test files across both crates — a
new mandatory field would have broken far more call sites than six new enum variants did.

## Decision

> `codec_type()` (`apple/videotoolbox/codec.rs`) maps each `CodecKind::ProRes*` variant to its
> `kCMVideoCodecType_AppleProRes*` constant 1:1. `configure_properties()` branches on a new
> `is_prores(codec)` helper: ProRes sessions get `RealTime`/`AllowFrameReordering`/
> `ExpectedFrameRate` (generic pacing hints, codec-independent) but **skip**
> `ProfileLevel`/`MaxKeyFrameInterval`/`AverageBitRate` entirely — the profile is baked into
> `codec_type`, not settable afterward; ProRes is unconditionally all-intra, so
> `VideoEncoderConfig::gop_size` has nothing to configure; profile determines quality/bitrate, not
> a settable property, so `VideoEncoderConfig::bitrate_bps` has nothing to configure. Both are
> **silently not honored** for ProRes — an explicit, documented backend-specific fallback (matches
> `gop_size`'s/`rate_control`'s own rustdoc contract: "a backend that cannot honor a value falls
> back... and must document that fallback"), not a silent bug.
>
> CPU input reuses `upload_cpu_nv12`/NV12 4:2:0 8-bit unchanged — grounded in
> `AVAssetWriterInput.appendSampleBuffer:`'s own doc comment: "If you are working with 8bit sources
> ProRes is also a good format to use due to its high image quality. Use either of the recommended
> [8-bit 4:2:0] pixel formats above... Pixel buffers not in a natively supported format will be
> converted internally prior to encoding when possible." This evidence is from `AVAssetWriterInput`
> (a higher-level AVFoundation wrapper over `VTCompressionSession`, not the session API itself) —
> flagged as adjacent-API evidence per this session's honesty discipline, not confirmed
> character-for-character for `VTCompressionSession` directly. `yuv420_size`/`validate`/
> `push_frame`/Zero-Copy input needed **zero** changes — already codec-agnostic.
>
> `handle_output`'s extradata-extraction dispatch gained an explicit `is_prores` branch that skips
> straight to marking `finalized_info` from `base_info` (already-empty `extra_data`) — ProRes has
> no parameter sets to extract, so calling `extract_h264` against a ProRes format description would
> just fail to find anything, uselessly. Fixing this dispatch surfaced a **real latent bug**: the
> prior code was `match shared.codec { Hevc => extract_hevc, _ => extract_h264 }` — any future
> non-H.264/HEVC codec (not just ProRes) would have silently fallen through to H.264 extraction.
> Now explicit: `H264 => extract_h264, Hevc => extract_hevc, _ => None`.

## Scope (this stage)

**In:** ProRes 422 Proxy / 422 LT / 422 (standard) / 422 HQ / 4444 / 4444 XQ encode, CPU NV12
upload and Zero-Copy `CVPixelBuffer` input (both reused unchanged from H.264/HEVC), real per-packet
`is_keyframe` (always true, via the existing `is_sync_sample` mechanism — untested against a real
ProRes bitstream, but the mechanism itself is codec-generic).

**Out (permanent, not deferred):**

- **ProRes RAW / RAW HQ** — zero references to either codec type exist anywhere in
  `VTCompressionSession`/`VTCompressionProperties`/`VTCompressionSession.rs` (confirmed: grepped
  every generated `VideoToolbox/*.rs` file). RAW is camera-capture-hardware-produced only
  (`AVCaptureDevice`/`AVCaptureMovieFileOutput`), never a general compression-session target — the
  same class of gap as VP9/AV1 encode (ADR-0002), not a deferred stage.
- **10/12-bit or 4:2:2/4:4:4-native input** — this stage's CPU-upload path stays NV12-only
  (`VideoEncoderConfig::pixel_format != PixelFormat::Nv12` rejects at `validate()`, unchanged from
  H.264/HEVC); Apple's documented high-bit-depth ProRes input formats
  (`kCVPixelFormatType_4444AYpCbCr16`/`_422YpCbCr16`/`_422YpCbCr10`) are not wired up — a real,
  disclosed quality ceiling for this stage (every ProRes encode through this backend is effectively
  an 8-bit-4:2:0-sourced one, regardless of the target profile's own higher native ceiling).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Single `CodecKind::ProRes` + a `prores_profile` field on `VideoEncoderConfig` | Rejected — breaks every full-literal `VideoEncoderConfig { .. }` construction site across the workspace (confirmed: dozens, via `grep -c "VideoEncoderConfig {"`), a much larger ripple than six new enum variants (2 truly-exhaustive match sites). See § Context. |
| A separate, ProRes-only `ProResEncoder`/`ProResEncoderConfig` type outside `VideoEncoder`/`AppleVideoEncoder` (mirroring `AacDecoderConfig`'s "no shared config" precedent for audio) | Rejected — unlike audio decode, this crate already has a working, fully codec-agnostic `VideoEncoderConfig`/`VideoToolboxVideoEncoder` that ProRes fits into with two small, additive changes; inventing a parallel type would duplicate `upload_cpu_nv12`/Zero-Copy/callback machinery for no benefit. |
| Set `MaxKeyFrameInterval`/`AverageBitRate` for ProRes anyway and let `VTSessionSetProperty` reject them | Rejected — silently swallowing (or worse, propagating as a confusing `EncodeError::Backend`) a property the codec fundamentally does not support is worse than never attempting it; explicit `is_prores` skip is the honest choice. |

## Consequences

### Positive

- Near-zero new code in the hot encode path — `push_frame`/`poll_packet`/`flush`/Zero-Copy input
  are shared, unmodified with H.264/HEVC.
- Fixed a real, pre-existing extradata-dispatch bug (silent H.264-extraction fallthrough for any
  non-HEVC codec) while adding ProRes, not just working around it.
- Six explicit `CodecKind` variants keep the C ABI / container-identity mapping honest — no
  profile ambiguity hidden behind a single opaque `ProRes` tag.

### Negative / Trade-offs

- **Zero compile verification as authored** — carries over unchanged.
- `gop_size`/`bitrate_bps` silently ignored for ProRes — a real behavior surprise for a caller who
  doesn't read this backend's docs (mitigated by the rustdoc-fallback-documentation contract these
  fields already carry generically, not a new pattern this ADR invents).
- 8-bit-4:2:0-only input this stage — ProRes's headline quality advantages (high bit depth, 4:4:4,
  lossless alpha) are not reachable through this backend yet.
- The `AVAssetWriterInput` pixel-format-tolerance evidence is one level removed from
  `VTCompressionSession` itself — flagged, not confirmed at the session-API level.

## References

- [ADR-0002](0002-videotoolbox-hevc-encode.md) — the VP9/AV1 "permanent platform gap" precedent
  this ADR's ProRes-RAW conclusion mirrors.
- [ADR-0003](0003-videotoolbox-metal-zero-copy-encode.md) — Zero-Copy input, reused unchanged.
- `mediaway-decoder` [ADR-apple/0006](../../../mediaway-decoder/adr/apple/0006-videotoolbox-prores-decode.md) —
  companion decode-direction ADR from the same session.
- Local grounding source (read directly): `local/vendor-ref/objc2/generated/CoreMedia/
  CMFormatDescription.rs` (`kCMVideoCodecType_AppleProRes*`), `local/vendor-ref/objc2/generated/
  VideoToolbox/VTCompressionProperties.rs` (full-file ProRes grep), `local/vendor-ref/objc2/
  generated/AVFoundation/AVAssetWriterInput.rs` (pixel-format recommendation doc comment).
- `README.md` § Codec support — new ProRes row, Apple column: `🛠️` → `🆗`.

ADRs are written in **English**.
