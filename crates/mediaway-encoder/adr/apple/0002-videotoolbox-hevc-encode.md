# ADR-0002: `VideoToolbox` encode HEVC addition; VP9/AV1 permanent non-support

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (module `mediaway-encoder::apple`)

## ⚠️ Zero real-hardware / zero compile verification in this session

Same structural constraint as [ADR-0001](0001-videotoolbox-h264-cpu-upload.md): this repo's dev
environment (Windows host) cannot cross-compile Apple code at all. Every API name/signature/
constant cited below is a **direct read** of the locally cloned
[`objc2`](https://github.com/madsmtm/objc2) checkout
(`local/vendor-ref/objc2/framework-crates/objc2-video-toolbox/src/generated/`,
`local/vendor-ref/objc2/generated/VideoToolbox/mod.rs`), not a paraphrase from memory or web
search. This crate's Apple encode module is entirely `#[cfg(any(target_os = "macos", target_os =
"ios"))]`-gated at `apple/mod.rs`, including its own test files — unlike `mediaway-decoder::apple`
(whose pure `codec.rs` module is additionally `cfg(test)`-enabled for host testing), **nothing in
this crate's Apple module compiles or tests on this dev host at all**, before or after this ADR.

## Context

`mediaway-decoder::apple` (this session's companion ADR,
[decoder ADR-0002](../../../mediaway-decoder/adr/apple/0002-videotoolbox-hevc-vp9-av1-decode.md))
adds HEVC/VP9/AV1 **decode**. This ADR covers the encode side, and the two codecs are not
symmetric: `VideoToolbox`'s decode API (`VTDecompressionSession`) can decode HEVC, VP9, and AV1,
but its **encode** API (`VTCompressionSession`) only exposes a compression path for H.264 and
HEVC — confirmed by grepping every generated `objc2-video-toolbox` file for `kVTProfileLevel_*`
constants: `H264` and `HEVC` variants exist (`kVTProfileLevel_HEVC_Main_AutoLevel`,
`_Main10_AutoLevel`, `_Main42210_AutoLevel`, `_Monochrome_AutoLevel`, `_Monochrome10_AutoLevel` —
`local/vendor-ref/objc2/generated/VideoToolbox/mod.rs` lines 441-449), **no `VP9` or `AV1`
variant exists anywhere in the bindings**. This is a genuine, permanent platform API gap — Apple
has not shipped a public `VideoToolbox` VP9 or AV1 hardware/software encoder as of the SDK version
this `objc2` checkout was generated against — not a driver-maturity block like this workspace's
Vulkan AV1 encode backend (which has real API surface that a specific driver fails to use
correctly) and not a deferred stage like Android's csd extradata gap. There is no compression
property, no codec type constant accepted by `VTCompressionSessionCreate` for either codec, and
no code this crate could write today that would make it work.

## Decision

> Add HEVC encode (Main-class profile, CPU NV12 upload, matching H.264's existing scope shape).
> Do **not** implement VP9/AV1 encode — record the platform gap here instead of a "deferred"
> roadmap bullet, since there is no future stage where more of this crate's own code would close
> it; it can only be revisited if Apple ships a public compression API for either codec.

### HEVC encode — mirrors H.264's existing shape exactly

- `codec_type(CodecKind::Hevc) -> kCMVideoCodecType_HEVC` (confirmed real constant,
  `CMFormatDescription.rs`, alongside the already-used `kCMVideoCodecType_H264`).
- Profile: `kVTProfileLevel_HEVC_Main_AutoLevel` — the direct HEVC analog of the existing
  `kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel` choice (Main, not Main10/4:2:2:10, since
  this backend's CPU-upload path is 8-bit 4:2:0 NV12 only, matching H.264's own Constrained-
  Baseline choice's implicit 8-bit-only scope).
- Extradata: `CMVideoFormatDescriptionGetHEVCParameterSetAtIndex` (confirmed real, same file as
  the H.264 getter this backend already calls, same signature shape with one more parameter-set
  index available) extracts VPS(0)/SPS(1)/PPS(2) — three parameter sets instead of H.264's two.
  Built into an Annex-B blob (`[start-code, VPS, start-code, SPS, start-code, PPS]`) and converted
  via the new `iso_bmff::bitstream::hevc::to_hvcc` (added by the decoder ADR-0002 in the same
  session, reused here rather than writing a second `hvcC` builder).
- Everything else (session creation, callback bridging, NV12 upload, GOP/bitrate/frame-rate
  properties, keyframe detection via `kCMSampleAttachmentKey_NotSync`) is unchanged and
  codec-agnostic — `VTCompressionSession::create`/`configure_properties`'s non-profile properties
  apply identically to both codecs.

### Module layout

New file `src/apple/videotoolbox/extradata.rs` holds `extract_h264`/`extract_hevc` (moved out of
`video.rs`, which otherwise duplicates per-codec extraction logic inline) — the encoder-side
mirror of the decoder ADR-0002's `format_desc.rs` split, for the same reason (keep `video.rs`
about session/callback plumbing, not per-codec box-building). `SharedState` gains a `codec:
CodecKind` field so the output callback's `handle_output` (a free function, not a method) can
dispatch to the right extraction function.

## Scope (this stage)

**In:**

- HEVC encode: Main-class profile, CPU NV12 upload, hvcC extradata via VPS+SPS+PPS extraction —
  otherwise identical scope to ADR-0001's H.264 (best-effort `gop_size` via
  `kVTCompressionPropertyKey_MaxKeyFrameInterval`, real per-packet keyframe detection).

**Out:**

- **VP9/AV1 encode — permanently out of scope for this backend**, not deferred. No
  `VideoToolbox` compression API exists for either codec (see § Context). Revisiting this
  requires Apple shipping new public API, not more work in this crate.
- Zero-Copy `CVPixelBuffer`/`IOSurface` input — unchanged from ADR-0001, still deferred.
- HEVC Main10/4:2:2:10/Monochrome profiles, 10-bit input — this backend's CPU-upload path stays
  8-bit 4:2:0 NV12 only, same constraint H.264 already had.
- `mediaway-encoder`'s `auto`/`capability` wiring for HEVC specifically stays out of this ADR's
  scope; the workspace-level `mediaway::platform` wiring (covering H.264+HEVC at once) is a
  separate change (`crates/mediaway/src/platform.rs`).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Implement VP9/AV1 encode via a software fallback (e.g. `mediaway-sw`) when `VideoToolbox` can't | Rejected — out of scope for this ADR, which is specifically about what `VideoToolbox` itself can do; a cross-platform SW encoder fallback (if ever built) is a `mediaway-encoder` facade-level decision, not an Apple-backend one, and no such fallback exists in this workspace for any codec today. |
| Default HEVC to a higher profile (Main10) to future-proof for 10-bit input | Rejected — this backend's CPU-upload path is hardcoded 8-bit 4:2:0 NV12 (`upload_cpu_nv12`, `PixelFormat::Nv12` validation); requesting Main10 for content this backend can never actually produce would be a dishonest profile claim, not a real capability. |
| Keep VP9/AV1 out of `codec_type`'s match arms silently (fall into the existing `_ => Err(Unsupported)` without documentation) | Rejected — `docs/spec/caveats-and-clarity.md`'s "no silent slow defaults / honest names" principle applies to hard gaps too, not just costly paths; the existing wildcard already returns the right error, but leaving *why* undocumented would make a future contributor re-investigate a question this session already answered definitively by reading every generated binding. |

## Consequences

### Positive

- HEVC encode reaches the same maturity as H.264 (real hvcC extradata, real keyframe detection)
  at low incremental risk — almost entirely reuses ADR-0001's reviewed design.
- The VP9/AV1 gap is now a documented, source-grounded fact (not an assumption) that future
  sessions do not need to re-research from scratch.
- `iso_bmff::bitstream::hevc::to_hvcc` (added by the decoder ADR-0002) is reused here rather than
  duplicated — one `hvcC` builder for the whole workspace.

### Negative / Trade-offs

- **Zero compile verification as authored** — carries over from ADR-0001 unchanged, and this
  crate's Apple module has no host-testable pure-logic subset at all (unlike the decoder crate),
  so there is no compiler feedback loop of any kind for this change until real Apple CI/hardware
  access exists.
- VP9/AV1 encode capability gap is real and permanent for this backend specifically — any product
  requirement for VP9/AV1 encode on Apple platforms needs a different approach entirely (not a
  `mediaway-encoder::apple` code change).

## References

- [ADR-0001](0001-videotoolbox-h264-cpu-upload.md) — original H.264 scope, session lifecycle,
  callback design, all reused unchanged by this ADR
- `mediaway-decoder` [ADR-apple/0002](../../../mediaway-decoder/adr/apple/0002-videotoolbox-hevc-vp9-av1-decode.md) —
  companion decode-side ADR from the same session; source of `iso_bmff::bitstream::hevc::to_hvcc`,
  reused here
- Local grounding source (read directly): `local/vendor-ref/objc2/generated/VideoToolbox/mod.rs`
  (`kVTProfileLevel_HEVC_*` constants, absence of any `VP9`/`AV1` compression constant),
  `local/vendor-ref/objc2/generated/CoreMedia/CMFormatDescription.rs`
  (`kCMVideoCodecType_HEVC`, `CMVideoFormatDescriptionGetHEVCParameterSetAtIndex`)
- `README.md` § Codec support — Apple encode HEVC cell: `👻` → `🆗` once implemented
  (implemented/compiles, not hardware-verified, matching this backend's existing H.264 mark); VP9/
  AV1 cells stay `🚫`/N/A (platform gap, not a roadmap item)

ADRs are written in **English**.
