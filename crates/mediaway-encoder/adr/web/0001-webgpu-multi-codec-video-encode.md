# ADR-0001: Multi-codec WebGPU-surface video encode (HEVC / AV1 / VP9)

- **Status**: Proposed
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`src/web/wasm.rs`)

## Note on this ADR's premise

The task that produced this ADR assumed a prior "Web Opus" session had already
landed `crates/mediaway-encoder/adr/web/0001-webcodecs-opus-audio-encode.md`
on a branch `feat/web-opus`, making this ADR `0002`. Neither the branch nor
any `adr/web/` directory exist in this repository (checked local + remote
refs, and `crates/mediaway-encoder/src/web/wasm.rs` has no Opus code on any
reachable branch). This is therefore the **first** ADR in `mediaway-encoder`'s
`adr/web/` folder — numbering restarts at `0001`, matching the per-platform
numbering convention already used by `adr/windows/`, `adr/linux/`,
`adr/vulkan/`, etc.

## Context

`mediaway-encoder::web::wasm.rs` has two video-encode call paths today:

1. **CPU path** — already codec-parameterized: `video_codec_supported(codec:
   &str)`, `is_webcodecs_video_codec_supported(codec: String)`, and
   `encode_video_frames(codec: String, width, height, bitrate_bps, lumas,
   timestamps_us)` all accept an arbitrary WebCodecs codec string
   (`avc1…`/`hev1…`/`av01…`/`vp09…`) and share one generic
   `encode_frame_via(frame, codec)` helper.
2. **GPU-surface path** — hardcoded to H.264 end to end:
   `is_webgpu_video_frame_supported()` (calls `video_supported()` →
   `video_codec_supported("avc1.42E01E")`), `webgpu_canvas_frame()` (builds a
   `VideoFrame` from a WebGPU-backed `OffscreenCanvas`, codec-agnostic
   already), `encode_one_h264_frame_from_webgpu_canvas()` (calls
   `encode_frame_via(&frame, "avc1.42E01E")`), `webcodecs_gpu_video_fmp4_smoke()`,
   and `mux_video_chunk()` (hardcodes `iso_bmff::Codec::H264` for the sample
   entry).

Goal: extend the GPU-surface path to HEVC/AV1/VP9, mirroring the CPU path's
existing codec-parameterized shape.

## Investigation: does codec choice affect the WebGPU/canvas side at all?

**No.** `webgpu_canvas_frame()` renders into `ctx.getCurrentTexture()` at the
canvas's `getPreferredCanvasFormat()` (a *pixel* format — `bgra8unorm` /
`rgba8unorm` — unrelated to the video *codec*) and builds a `VideoFrame` from
the canvas. `VideoEncoder.configure()`/`encode()` then convert that
`VideoFrame` to whatever YUV subsampling the target codec needs, entirely
inside the browser — the exact same conversion step the CPU NV12 path already
goes through. `encode_frame_via(frame, codec)` is already generic over both
`frame` source and `codec` string and is shared by all three CPU-path,
GPU-path, and `video_codec_supported`'s real-encode probe call sites. So the
GPU-canvas-to-`VideoEncoder` leg itself needs **no** codec-specific branching
— confirms the task's working hypothesis. All real complications below are
**downstream of the encoder** (muxing, and the `web-sys` binding surface),
not in the WebGPU render pass.

## Real complications found

### 1. `mux_video_chunk` hardcodes `Codec::H264` — needs a codec-string → `iso_bmff::Codec` mapper

No such mapper exists yet anywhere in-tree for WebCodecs strings specifically
(`mediaway-container::convert::to_codec_kind` maps a *different* type, the
facade's generic `CodecKind`, not `"avc1.…"`/`"hev1.…"` strings). A new
`iso_codec_for(codec: &str) -> Result<Codec, JsValue>` prefix matcher
(`avc1`→`H264`, `hvc1`/`hev1`→`Hevc`, `av01`→`Av1`, `vp09`/`vp08`→`Vp9`) is
needed and should be shared by a generalized `mux_video_chunk(codec, video)`.
`iso-bmff` already writes real, correctly-labeled sample entries for all four
video codecs ([`iso-bmff` ADR-0002](../../../iso-bmff/adr/0002-vp9-sample-entry.md),
[ADR-0003](../../../iso-bmff/adr/0003-hevc-av1-sample-entry.md)) — no
container-side gap.

### 2. HEVC bitstream framing is an open question, and `web-sys` 0.3.104 gives no lever to control it

Per `docs/ai/wiki/container/mp4-sample-entries.md`, `iso-bmff`'s `hvc1` sample
entry expects **length-prefixed** HEVC NALs and does *no* conversion (unlike
`Codec::H264`, which auto-converts Annex-B → AVCC on mux). Whether a
WebCodecs `VideoEncoder` emits length-prefixed or Annex-B `EncodedVideoChunk`
bytes for HEVC is governed by the codec string's storage-mode prefix
(`hvc1.…` vs `hev1.…`) and/or a dedicated `hevc: { format }` extension on
`VideoEncoderConfig` per the WebCodecs Codec Registration spec. **Checked
this workspace's pinned `web-sys` 0.3.104 source directly
(`web-sys-0.3.104/src/features/gen_VideoEncoderConfig.rs`): there is no
`avc`/`hevc` field, and no `AvcEncoderConfig`/`HevcEncoderConfig` type exists
in this crate version at all** — the only lever available through this
binding is the codec string prefix itself. This is a genuine, codec-specific
`web-sys` gap (structurally the same shape as this session's separately-noted
missing `OpusEncoderConfig`), not a Mediaway bug. **Open question, needs
empirical real-browser verification** (this crate's established method: real
Chrome/Edge over CDP, not Playwright's bundled Chromium — see
`docs/ai/wiki/encode/web-real-chrome-bugs.md`): does `hvc1.` reliably yield
length-prefixed output matching `iso-bmff`'s unconverted expectation? If only
`hev1.`/Annex-B is available in practice, HEVC fMP4 muxing would additionally
need an Annex-B → HVCC bitstream conversion step (mirroring the existing
H.264 one) before this ADR's HEVC path can honestly claim working fMP4
output — that conversion is explicitly **not** proposed here until the real
framing behavior is confirmed.

### 3. AV1 / VP9 need no such conversion

WebCodecs emits a raw OBU stream for AV1 and a raw frame for VP9 — both
already match `iso-bmff`'s unconverted `av01`/`vp09` sample-entry expectations
(confirmed working end to end for VP9 already, per
`docs/ai/wiki/decode/web-video-decode.md` § "fMP4 mux/demux in the browser
E2E"). Lowest-risk part of this ADR's scope.

### 4. HEVC hardware/license gating — already handled by existing plumbing

`video_codec_supported`'s real-encode probe (added earlier this project to
fix `isConfigSupported`'s H.264 over-report — see
`docs/ai/wiki/decode/web-video-decode.md` § "Second real bug") is already
generic over the codec string, so it will honestly report `false` for HEVC
wherever no real encoder is reachable (common in Chrome due to licensing —
often hardware-only with no software fallback). No new work needed here;
worth documenting as an *expected* honest-skip outcome in most CI/headless
environments.

### 5. Minimum coded-size constraints — unverified, same caveat as above

Whether HW HEVC/AV1 encoders accept the existing 64×64 smoke size is
unverifiable without a real browser + real hardware encoder; flag as an open
question for the same manual-CDP verification pass.

## Decision

> Generalize the GPU-surface encode functions to accept a `codec: String`
> parameter, mirroring `encode_video_frames`'s existing shape, reusing
> `encode_frame_via` and `video_codec_supported` unchanged.

- `webgpu_canvas_frame(width, height)` — **unchanged** (already codec-agnostic).
- `is_webgpu_video_frame_supported(codec: String) -> bool` — replaces the
  current zero-arg, H.264-only version; delegates to the already-generic
  `video_codec_supported(&codec)` plus the existing WebGPU-device check.
- `encode_video_frame_from_webgpu_canvas(codec: String, width, height,
  bitrate_bps) -> Result<Vec<u8>, JsValue>` — generalizes
  `encode_one_h264_frame_from_webgpu_canvas`.
- `webcodecs_gpu_video_fmp4_smoke(codec: String) -> Result<Vec<u8>, JsValue>`
  — generalizes the current zero-arg smoke wrapper; calls the new
  `iso_codec_for(codec)` mapper (§1) inside a generalized `mux_video_chunk`.
- **Open decision to confirm with the maintainer before implementing**:
  whether the existing zero-arg `is_webgpu_video_frame_supported` /
  `webcodecs_gpu_video_fmp4_smoke` names are *replaced* (breaking change for
  `tools/e2e-web` callers, which must pass `"avc1.42E01E"` explicitly
  afterward) or *kept* as thin H.264 convenience wrappers over the new
  codec-parameterized functions. `encode_video_frames` set a precedent for
  the CPU path by adding a new codec-parameterized function name alongside
  the old fixed one (`encode_one_h264_frame` still exists, unexported,
  reused only by the AV smoke test) — the same additive shape is the
  likely-preferred option here, but is a scope call, not assumed.

Deferred, not part of this ADR:

- HEVC Annex-B → HVCC bitstream conversion (§2) — only if real-browser
  verification shows it is actually needed.
- Any `web-sys` fork/patch to add `HevcEncoderConfig`/`AvcEncoderConfig`
  bindings — only if the codec-string-prefix lever proves insufficient.
- GPU-surface **decode** (see § Scope: encode-only below).

## Scope: encode-only — no GPU-surface decode path exists to extend

Checked `crates/mediaway-decoder/src/web/wasm.rs` in full. Its
`decode_video_chunks` is **already** codec-parameterized (`codec: String`,
matches CPU-encode's shape) but has **no** GPU-resident output path at all —
`read_luma_plane` unconditionally reads every decoded `VideoFrame` back to a
CPU `Vec<u8>` via `VideoFrame::copyTo`. No `GPUExternalTexture` /
`importExternalTexture` binding is present anywhere in this crate's `web-sys`
feature list or code (`GpuExternalTexture` grep across the repo only matches
unrelated D3D12/Vulkan GPU-handle code, not `mediaway-decoder::web`). There
is also no `crates/mediaway-decoder/adr/web/` directory — this platform
folder has never had an ADR. Building a decode-side GPU-resident surface
(`VideoFrame` → WebGPU external texture import) is new, not-yet-started
Stage-2/3 decode work, not an extension of anything that exists today — out
of scope for this ADR; would need its own crate-local ADR under
`mediaway-decoder/adr/web/` if pursued.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Duplicate per-codec functions (`encode_one_hevc_frame_from_webgpu_canvas`, …) | Contradicts this file's own established generalize-to-`codec: String` pattern (`encode_video_frames` vs. the old `encode_one_h264_frame`); quadruples the `mux_video_chunk` Codec-mapping problem instead of solving it once |
| Add HEVC Annex-B→HVCC conversion now, unconditionally | Premature — §2's real-browser framing behavior is unverified; would risk shipping an unverified/wrong conversion |
| Patch/fork `web-sys` to add `HevcEncoderConfig`/`AvcEncoderConfig` | Heavier dependency change than justified before confirming the codec-string-prefix lever is actually insufficient |

## Consequences

### Positive

- Reuses already-hardware-adjacent, already-honesty-hardened plumbing
  (`encode_frame_via`, `video_codec_supported`'s real-encode probe) with no
  new `web-sys` features and no new dependencies.
- Symmetric DX with the CPU path (`encode_video_frames`) once landed.
- Container side is already fully ready (`iso-bmff` ADR-0002/0003) except for
  the new codec-string mapper (§1), a small, self-contained addition.

### Negative / Trade-offs

- HEVC's fMP4-mux correctness depends on an unresolved bitstream-framing
  question (§2) that only a real (non-Playwright-bundled) browser can answer
  — this ADR's HEVC scope may need a same-crate follow-up once verified.
- Broadens the real-Chrome-only verification surface: HEVC/AV1/VP9 hardware
  encoders are, if anything, *less* likely than H.264's to be present in a
  CI/headless Chromium build, so most new coverage here will only be
  provable on the maintainer's manual real-browser CDP rig, not CI —
  `tools/e2e-web`'s existing honest-skip pattern must be preserved, not
  loosened.
- Does not address GPU-surface decode; callers wanting a full GPU-resident
  round trip still have a CPU-readback step on the decode side today.

## Open Questions (block implementation, not this ADR)

1. Does `hvc1.` (vs `hev1.`) reliably produce length-prefixed
   `EncodedVideoChunk` output in real Chrome/Edge, matching `iso-bmff`'s
   unconverted `hvc1` sample-entry expectation? (§2)
2. Do HEVC/AV1 hardware encoders in real Chrome/Edge accept the existing
   64×64 smoke resolution, or is a larger minimum size required? (§5)
3. Replace vs. keep the existing zero-arg H.264 convenience wrappers
   (Decision, last bullet)?

## References

- `crates/mediaway-encoder/src/web/wasm.rs`
- `crates/mediaway-decoder/src/web/wasm.rs`
- `docs/ai/wiki/encode/web-gpu-frame.md`
- `docs/ai/wiki/encode/web-real-chrome-bugs.md`
- `docs/ai/wiki/decode/web-video-decode.md`
- `docs/ai/wiki/container/mp4-sample-entries.md`
- `crates/iso-bmff/adr/0002-vp9-sample-entry.md`, `0003-hevc-av1-sample-entry.md`
- `docs/spec/caveats-and-clarity.md` (`webgpu_canvas_frame` catalog row)
- `crates/mediaway-encoder/docs/roadmap.md` § 2 — Web

ADRs are **English**. Numbering is local to this `adr/web/` folder.
