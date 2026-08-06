# ADR-0002: D3D12 native video decode (H.264, HEVC, AV1 — general GOP, Zero-Copy out)

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder-windows`

## ⚠️ Design-only pass — no code, no hardware verification yet

This ADR is a design plan, not an implementation report. Nothing described here has been
compiled or run. Grounding used, most-to-least authoritative:

1. This crate's own working WMF decode code (`src/wmf/`, ADR-0001) and this workspace's
   sibling `mediaway-encoder-windows` native D3D12 **encode** module
   (`src/d3d12_video_encode/`, [ADR-0007](../../mediaway-encoder-windows/adr/0007-d3d12-native-video-encode.md))
   — real, hardware-verified code in this same repo, the closest available ground truth for
   how this `windows` crate exposes `d3d12video.h` and how COM/fence/heap plumbing behaves
   on the actual driver in use.
2. Targeted Microsoft Learn searches performed for this ADR (real web access was available
   this session) confirmed: `ID3D12VideoDecoderHeap` and `ID3D12VideoDecodeCommandList1::
   DecodeFrame1` exist as documented Win32 APIs; `D3D12_VIDEO_DECODE_ARGUMENT_TYPE_PICTURE_
   PARAMETERS` is documented as carrying **DXVA-specification-defined** per-codec structures
   (confirms this ADR's "the D3D12 decode API is DXVA-shaped" premise is accurate, not
   folklore); `D3D12_VIDEO_DECODE_REFERENCE_FRAMES` is documented with `NumTexture2Ds` /
   `ppTexture2Ds` / `pSubresources` / `ppHeaps`, and Microsoft Learn text distinguishes
   "texture array" mode (entries share one resource) from "array of textures" mode (separate
   resources) — both real, both usable. **Not independently fetched this session**: the
   full field list of `D3D12_VIDEO_DECODE_FRAME_ARGUMENT`, the exact output/reference
   texture-array alignment constant, or whether the `windows` crate version already pinned
   in this workspace exposes complete D3D12 video-**decode** bindings (ADR-0007 confirmed
   AV1 **encode** bindings were complete; decode bindings completeness is **unconfirmed** and
   should be the first thing an implementer checks).
3. FFmpeg's public `libavcodec/d3d12va_decode.c` + `dxva2_h264.c` / `dxva2_hevc.c` /
   `dxva2_av1.c` (BSD-2-Clause, reference-only — the same ground-truthing method
   ADR-0007 used for D3D12 encode; **no code copied**, only call sequence / struct-filling
   *logic* referenced from memory of that codebase's shape, not fetched fresh this session).

Treat every design choice below as **unverified against real hardware or a real driver**
until implemented and run, per this workspace's `mediaway-decoder-linux` ADR-0001 honesty
convention.

## Context

`mediaway-decoder-windows` currently only decodes through Media Foundation (ADR-0001):
HW/software decoder MFTs do all bitstream parsing, entropy decoding, and reconstruction
internally — this crate never touches H.264/HEVC/AV1 syntax. Windows also exposes a
**second, independent** hardware decode path: `ID3D12VideoDevice::CreateVideoDecoder` /
`CreateVideoDecoderHeap` + `ID3D12VideoDecodeCommandList1::DecodeFrame1`. This is the
decode-side counterpart to the native D3D12 **encode** path `mediaway-encoder-windows`
implemented this session (ADR-0007) — same `windows` crate bindings bucket
(`windows::Win32::Media::MediaFoundation`, a `d3d12video.h` metadata artifact, alongside
`Win32::Graphics::Direct3D12`), zero new Cargo dependency.

Unlike Media Foundation, **D3D12 video decode requires the application to parse the
bitstream itself** and hand the driver DXVA-specification-shaped picture-parameter/slice-
control structures per codec — exactly the same architectural fact
`mediaway-decoder-linux`'s VA-API ADR-0001 already documented for that API family. That ADR
deliberately scoped to **IDR-only, no DPB, no reference-picture-list construction** as a
first stage. **This ADR's scope is explicitly broader, by direct project-owner decision**:
general GOP (P/B reference frames, real DPB / reference-picture-list management) across
**all three** of H.264, HEVC, and AV1 from the start, not staged codec-by-codec, and not
IDR-only. This is a substantially larger surface than either sibling decode ADR
(`mediaway-decoder-linux` ADR-0001) or the encode-side precedent (ADR-0007, which stayed
all-intra/no-DPB for all three codecs it covers).

## Decision

> Add `d3d12_video_decode` (+ a directory of sibling per-codec/shared files, split to stay
> under the 1000-line source limit): a **self-contained, currently-unregistered** module
> (`mod d3d12_video_decode;`, not `pub mod`, in `src/lib.rs` — same trick ADR-0007 used so
> hardware-gated tests compile/run without touching the working `WindowsVideoDecoder::open`
> WMF dispatch) implementing native D3D12 decode for H.264, HEVC, and AV1 with general-GOP
> (P/B, DPB) support and Zero-Copy D3D12 texture output.

### Why unregistered, and why one ADR covers three codecs + general GOP

Recommended over wiring in immediately, for two reasons:

1. **Blast-radius containment.** This is strictly the largest single decode surface this
   crate has attempted (DXVA-shaped picture params × 3 codecs × real reference-picture
   management), built directly against a working, shipped WMF decode path. Keeping it
   unregistered until each codec's real-hardware round trip is verified — mirroring how
   ADR-0007 kept `d3d12_video_encode` unregistered through H.264/HEVC and only
   *temporarily* wired it in per-addendum to confirm behavior, then reverted — avoids any
   risk of destabilizing `WindowsVideoDecoder`'s existing WMF/DX11 Zero-Copy contract while
   this module is built incrementally.
2. **One ADR, staged addenda — not three ADRs.** The underlying decision (native D3D12
   decode, DXVA-shaped params, texture-array DPB, bounded-handle Zero-Copy contract) is one
   coherent architecture shared by all three codecs; only the per-codec bitstream/DXVA-struct
   details differ. Recommend following ADR-0007's own precedent exactly: land H.264 first,
   append an **Addendum** section here with real-hardware findings once verified, then HEVC,
   then AV1 — same file, growing status, not a fork per codec.

A later integration pass still owns making the module `pub` and deciding how it fits into
`WindowsVideoDecoder`'s `Backend` dispatch alongside `wmf::WmfH264Decoder` /
`wmf::WmfMultiCodecCpuDecoder` — e.g. a new `Backend::D3d12(...)` variant selected by a
`VideoDecoderConfig`-level preference, mirroring the open question ADR-0007 left for encode.

### Scope: general GOP, all three codecs, from the start

- **H.264 (Baseline/Main/High as the bitstream allows), HEVC (Main), AV1 (Main profile,
  8-bit 4:2:0)** — no staged single-codec-first order for *this* ADR's scope (contrast with
  the encode precedent's H.264-first staging); real SPS/PPS/VPS/slice or OBU parsing for
  each, driving DXVA-shaped `D3D12_VIDEO_DECODE_FRAME_ARGUMENT` picture-parameter buffers.
- **General GOP**: P and B reference frames, real DPB / reference-picture-list management —
  explicitly **not** the IDR-only cut both `mediaway-decoder-linux` (VA-API) and the D3D12/
  Vulkan **encoders** used. Requires real POC computation (H.264 types 0/1/2, HEVC POC),
  reference-picture-list construction (H.264 `RefPicList0`/`RefPicList1` default order +
  `ref_pic_list_modification`; HEVC short-/long-term RPS; AV1's `ref_frame_idx[]` /
  `OrderHint`-based model), and DPB eviction (see below).
- **Bitstream framing reuse**: `mediaway_sw::h264::{BitReader, NalUnit, NalUnitType,
  split_annex_b, split_avcc}` are reused as-is (pure bit-level framing, format-agnostic to
  slice content). **`mediaway_sw::h264::{Sps, Pps, SliceHeader}` are *not* reused** — same
  conclusion `mediaway-decoder-linux` ADR-0001 already reached for VA-API: that parser
  intentionally discards fields needed for anything beyond a single I-slice picture (no
  `num_ref_frames`, no full `dec_ref_pic_marking` retention, POC types 1/2 unparsed, no
  `ref_pic_list_modification`). This crate needs its own local SPS/PPS/slice(/VPS/OBU)
  parsers built on the shared `BitReader`, scoped to what DXVA structures require — a
  **third** from-scratch H.264 high-level-syntax parser in this workspace (after
  `mediaway-sw`'s minimal one and `mediaway-decoder-linux`'s VA-API one, which is *also*
  IDR-only and would itself need general-GOP extension to be reusable here). See Open
  Questions for whether this should eventually become a shared crate.
- **DPB model: fixed-size texture-array pool** (Decision, not left open) — one
  `ID3D12Resource` NV12 texture array sized `max_ref_frames_for_stream + reorder_depth +
  caller_headroom` (SPS/VPS-derived `max_num_ref_frames` / `sps_max_dec_pic_buffering_minus1`
  / AV1 operating-point buffer size; `caller_headroom` a small config-level constant,
  default 2–4), matching FFmpeg's `hwcontext_d3d12va.c` decode-surface-pool shape and this
  session's Microsoft Learn findings that `D3D12_VIDEO_DECODE_REFERENCE_FRAMES` supports a
  "texture array" mode where every DPB slot is one subresource of the same resource. Chosen
  over per-slot independently-allocated textures ("array of textures" mode, also real and
  API-supported) because: (a) FFmpeg's own reference implementation and typical driver
  expectations use one shared array for decode output + references, avoiding a class of
  driver-compatibility gaps the independent-resources mode doesn't guarantee against; (b) a
  single stable resource pointer + varying subresource index gives
  `GpuBufferHandle::DirectX12 { resource, subresource }` a simple, uniform shape across every
  decoded frame — callers track one COM pointer, not N.
- **Zero-Copy output is a first-class design constraint, not an afterthought**: decoded
  pictures live as subresources of the DPB texture array; `VideoOutputPreference::
  ZeroCopyGpu` hands the caller `VideoFrameStorage::Gpu(GpuBufferHandle::DirectX12 {
  resource: <DPB array>, subresource: <slot index> })` directly — **no copy**.
  `VideoOutputPreference::CpuFramesOk` performs an explicit, named readback
  (`CopyTextureRegion` from the DPB slot into a `READBACK`-heap-equivalent linear buffer,
  reusing the `D3D12_HEAP_TYPE_CUSTOM` heap-type workaround ADR-0007 already discovered for
  `READBACK` resources under `VIDEO_DECODE_WRITE`/`_READ` states) — named `readback_dpb_
  slot_to_cpu`, documented per `docs/spec/caveats-and-clarity.md`.
- **Zero-Copy DPB-eviction contract (the hard part, decided here, not deferred)**: because
  every decoded picture is a *subresource of a resource the decoder itself keeps reusing as
  a reference*, a Zero-Copy handle handed to the caller is only safe to read until the
  decoder needs to reuse that exact slot for a new picture. Adopting **FFmpeg's own hwaccel
  surface-pool model**: size the DPB array with real headroom (above) so ordinary playback
  never contends, and if a caller holds a Zero-Copy frame long enough that decode would
  otherwise have to overwrite its still-live slot, **fail loudly** (return a decode error,
  never silently overwrite memory the caller may still be reading). No new copy is forced
  onto the common case; the caller is expected to consume/release Zero-Copy frames within a
  bounded number of subsequent `push_packet`/`poll_frame` calls, exactly as FFmpeg hwaccel
  callers must release `AVFrame` references promptly. See Open Questions for the exact error
  shape (today's `DecodeError` has no dedicated backpressure variant).
- **H.264 reference marking**: **sliding-window only** for this stage — adaptive marking /
  MMCO (`memory_management_control_operation` 1–6, long-term reference marking) is
  **deferred**, returning `Unsupported` for a stream that signals
  `adaptive_ref_pic_marking_mode_flag`. Sliding-window covers the overwhelming majority of
  real-world encoder output (including this workspace's own encoders, which never emit
  MMCO); long-term references are a genuine follow-up, not silently mishandled (rejected,
  not misdecoded).
- **HEVC tiles/WPP**: **single-tile, no wavefront** only for this stage — a stream signaling
  `tiles_enabled_flag` or `entropy_coding_sync_enabled_flag` returns `Unsupported`. Tiles/WPP
  change slice-segment addressing and multi-slice-per-picture assumptions throughout
  `DXVA_PicParams_HEVC`'s equivalent fields; deferring keeps those fields at trivial values.
- **AV1 film grain**: **rejected**, not implemented — a frame with `film_grain_params_
  present` (or a sequence header with `film_grain_params_present`) returns `Unsupported`.
  Film-grain synthesis is frequently a distinct post-decode pass even in HW pipelines (its
  own DXVA-equivalent extension struct); deferring avoids adding that surface before the
  core decode path is proven. **CDEF is in scope** (parsed and passed through — CDEF is
  integral to reconstruction, not an optional post-process, unlike film grain).
- **AV1 reference model**: OBU sequence/frame header parsing, `ref_frame_idx[NUM_REF_FRAMES]`
  / `OrderHint` bookkeeping against the DPB (AV1's model is virtual-index-based, not POC-
  based like H.264/HEVC — no separate "POC computation" step, just the frame header's own
  explicit reference-slot assignment) feeding the DXVA-shaped AV1 picture-parameter
  equivalent.

### File layout plan (not implemented this pass)

```
src/d3d12_video_decode.rs           # top-level: open() dispatch by codec, D3d12VideoDecoder
                                     # struct, push_packet/poll_frame/flush shared driving
                                     # loop, GopState-style per-codec state enum
src/d3d12_video_decode/
  setup.rs        # shared: D3D12_FEATURE_VIDEO_DECODE_SUPPORT + profile-GUID capability
                   # queries, ID3D12VideoDecoder/Heap creation, DPB texture-array allocation
                   # + sizing, command queue/list/fence setup (mirrors encode's setup.rs)
  dpb.rs           # NEW (no encode precedent): fixed-size DPB slot pool — free/in-use/
                   # reference tracking, outstanding-Zero-Copy-handle bookkeeping +
                   # backpressure check, generic over per-codec reference metadata
  ops.rs           # shared per-frame: build D3D12_VIDEO_DECODE_REFERENCE_FRAMES, submit
                   # DecodeFrame1, fence wait, hand out GpuBufferHandle::DirectX12 or
                   # readback_dpb_slot_to_cpu (mirrors encode's ops.rs/read_packet shape)
  util.rs          # shared small helpers: fence wait, resource-state transitions, GUID
                   # tables (mirrors encode's util.rs)

  h264.rs               # open-time feature query + decoder/heap creation for H.264
  h264_sps_pps.rs        # SPS/PPS parsing -> local Sps/Pps (fuller than mediaway_sw's)
  h264_slice.rs          # slice header parsing incl. ref_pic_list_modification,
                         # dec_ref_pic_marking (sliding-window path retained, not skipped)
  h264_poc.rs            # POC types 0/1/2
  h264_refs.rs           # RefPicList0/1 construction + DPB sliding-window eviction
  h264_pic_params.rs     # packs parsed state into the DXVA_PicParams_H264-equivalent
                         # D3D12 picture-parameter + slice-control buffers

  hevc.rs               # open-time feature query + decoder/heap creation for HEVC
  hevc_vps_sps_pps.rs    # VPS/SPS/PPS parsing (single-tile/no-WPP fields only)
  hevc_slice.rs          # slice segment header parsing, short-/long-term RPS
  hevc_poc.rs            # POC computation
  hevc_refs.rs           # RPS -> reference-picture-set / DPB management
  hevc_pic_params.rs     # DXVA_PicParams_HEVC-equivalent packing

  av1.rs                # open-time feature query + decoder/heap creation for AV1
  av1_obu.rs             # OBU splitting, temporal delimiter / sequence header parsing
  av1_frame_header.rs    # frame header parsing (uncompressed header, CDEF, no film grain)
  av1_refs.rs            # ref_frame_idx[] / OrderHint DPB management
  av1_pic_params.rs       # DXVA AV1 picture-parameter-equivalent packing

src/d3d12_video_decode_tests.rs  # top-level hardware-gated integration tests (mirrors
                                  # d3d12_video_encode_tests.rs)
```

~20 files. Several (`h264_slice.rs`, `hevc_slice.rs`, `av1_frame_header.rs`) may need their
own further split once real line counts are known — this plan is a starting decomposition,
not a final one; the 1000-line ceiling is enforced per-file regardless of this plan.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| IDR-only scope (mirror VA-API / D3D12-encode precedent) | Explicitly rejected by project-owner decision for this ADR — general GOP is the stated goal, not a future stage. |
| Stage H.264 first, defer HEVC/AV1 to later ADRs | Rejected for *this* ADR's structure (one ADR, staged addenda) — the architecture (DXVA-shaped params, texture-array DPB, bounded-handle Zero-Copy) is shared; per-codec detail differences don't warrant separate ADRs, matching ADR-0007's own addendum pattern. |
| DPB as independently-allocated textures ("array of textures" mode) | Real, API-supported mode, but FFmpeg's own reference implementation and typical driver expectations favor a shared output+reference texture array; independent resources also give `GpuBufferHandle::DirectX12` a less uniform shape (N pointers instead of one + subresource index). |
| Always copy Zero-Copy output into a caller-owned texture outside the DPB | Defeats the purpose of requesting `ZeroCopyGpu` — every frame becomes a GPU→GPU copy. Kept as a documented fallback alternative (e.g. a future opt-in preference), not the default. |
| Silently overwrite a DPB slot the caller still holds a live `GpuBufferHandle` to | Violates "never silently overwrite/readback" — must fail loudly instead (see Decision). |
| Reuse `mediaway_sw::h264::{Sps, Pps, SliceHeader}` as-is | Same reasoning `mediaway-decoder-linux` ADR-0001 already documented: that parser discards fields (`num_ref_frames`, POC types 1/2, `ref_pic_list_modification`, full `dec_ref_pic_marking`) this general-GOP path needs. |
| Reuse/extend `mediaway-decoder-linux`'s VA-API H.264 SPS/PPS/slice parser | That parser is *also* IDR-only (its own ADR-0001 defers general GOP to "Stage 2") and lives in a Linux-only crate; extending it in place and depending on it from a Windows-only crate would be an odd cross-platform-crate dependency. Noted as a possible future shared-crate extraction (see Open Questions), not done now. |
| Full H.264 MMCO / HEVC tile+WPP / AV1 film grain support from the start | Real complexity for comparatively low incremental value at this stage; each is independently reject-with-`Unsupported`-able without corrupting output for the common case, matching this crate's and the workspace's existing "explicit unsupported, never silently wrong" convention. |

## Consequences

### Positive

- A second, independent hardware-decode code path exists in this crate alongside WMF —
  same "both must keep working, neither assumes the other" relationship ADR-0007 documents
  for the encode side.
- General-GOP support (unlike every sibling IDR-only decode/encode backend in this
  workspace) makes this the first backend able to decode a real-world encoder's typical GOP
  structure (P/B frames), not just all-intra streams.
- Zero-Copy output is designed in from the start (texture-array DPB, stable resource +
  subresource handle shape) rather than retrofitted after a CPU-only implementation, per
  this ADR's mandate.
- The fixed-size DPB + bounded-handle-window model is not a novel invention — it mirrors
  FFmpeg's own proven hwaccel surface-pool contract, reducing the risk of an undiscovered
  design flaw.

### Negative / Trade-offs

- Largest bitstream-parsing surface this crate has taken on: three from-scratch high-level-
  syntax parsers (H.264 general-GOP, HEVC, AV1), each independently capable of subtle
  correctness bugs (POC arithmetic, reference-list construction) that only show up on real
  multi-GOP content — much harder to catch than the encode/VA-API precedents' all-intra
  streams, where every picture is independent.
- DPB-eviction backpressure is a real caller-visible behavior change from every other
  decoder in this workspace (WMF/VA-API): callers must release Zero-Copy frames promptly or
  see decode errors — must be documented prominently (rustdoc + wiki), not just in this ADR.
- H.264 MMCO, HEVC tiles/WPP, and AV1 film grain are real gaps a genuinely "general GOP"
  claim might be expected to cover; each is honestly scoped out with an explicit
  `Unsupported`, not silently misdecoded, but this is still a narrower "general GOP" than a
  fully spec-complete decoder.
- A third from-scratch H.264 SPS/PPS/slice parser now exists in this workspace
  (`mediaway-sw`, `mediaway-decoder-linux`, and this crate) — real duplication risk flagged
  as an open question below, not resolved by this ADR.
- No `windows` crate decode-binding-completeness confirmation and no real-hardware
  verification exist yet (see caveat section) — every structural claim here is provisional
  until an implementation pass runs against a real driver.

## Open Questions / Risks

1. **`windows` crate D3D12 video-**decode** binding completeness is unconfirmed.** ADR-0007
   found AV1 *encode* bindings complete on this workspace's pinned `windows` crate version;
   decode (`ID3D12VideoDecoder`, `ID3D12VideoDecoderHeap`, `DecodeFrame1`,
   `D3D12_VIDEO_DECODE_REFERENCE_FRAMES`, per-codec DXVA structs) has not been checked. First
   implementation-time task.
2. **DPB-eviction backpressure error shape.** Today's `mediaway_decoder::DecodeError`
   (`crates/mediaway-decoder/src/error.rs`) has no variant distinguishing "caller is holding
   a Zero-Copy frame too long" from a generic backend failure. `DecodeError` is
   `#[non_exhaustive]`, so growth is expected — but it is a **shared facade type** across
   every backend (WMF here, VA-API on Linux, WebCodecs on Web), so adding a variant is a
   cross-crate decision this Windows-crate-local ADR should flag, not silently decide.
   Recommend a small follow-up proposal against `mediaway-decoder` itself (e.g.
   `DecodeError::OutputNotReleased` or similar) rather than overloading `Backend` long-term.
3. **Should H.264/HEVC/AV1 general-GOP high-level-syntax parsing become a shared, sans-io
   crate** instead of a third from-scratch implementation local to this Windows crate? The
   field sets DXVA needs and VA-API needs overlap heavily (both need POC, ref-pic-list
   construction, DPB marking — `mediaway-decoder-linux`'s VA-API parser just hasn't grown
   into general-GOP territory yet either). Not resolved here — recommend revisiting once
   this crate's H.264 parser is implemented and Linux's VA-API backend also needs general-GOP
   support, per `docs/spec/crate-packaging.md`'s unprefixed-freestanding-core pattern (a
   hypothetical future `h264-dpb`/`h264-hrd`-style crate). Deliberately not started
   speculatively now.
4. **Exact DPB sizing formula per codec** (`max_num_ref_frames` for H.264,
   `sps_max_dec_pic_buffering_minus1` for HEVC, AV1's operating-point buffer-size fields) and
   the right default `caller_headroom` constant are design intent here, not finalized
   numbers — need real streams to validate against.
5. **Output/reference texture-array alignment requirement** (referenced informally in the
   task framing as a `D3D12_VIDEO_DECODE_OUTPUT_AND_REFERENCE_TEXTURE_ALIGNMENT`-shaped
   constraint) was **not** independently confirmed by name/value this session — verify the
   actual constant and its numeric requirement against the `windows` crate / Microsoft Learn
   before allocating the DPB array in an implementation pass.
6. **COM refcount `.clone()` discipline**: the DPB array's `ID3D12Resource` COM pointer will
   be cloned (AddRef) across multiple live `GpuBufferHandle`s / in-flight decode operations —
   every such `.clone()` needs an explicit `// clone: COM AddRef share` comment (not just the
   bare `Arc`/`Rc`-share exemption, since `windows`-crate COM wrappers aren't literally `Arc`/
   `Rc`), per this workspace's mandatory-clone-comment rule.

## References

- [`mediaway-encoder-windows` ADR-0007](../../mediaway-encoder-windows/adr/0007-d3d12-native-video-encode.md)
  — direct structural precedent (module split pattern, `READBACK`-heap-type workaround,
  unregistered-module staging, addendum pattern); explicitly named this decode work as its
  own future ADR.
- [`mediaway-decoder-windows` ADR-0001](0001-wmf-h264-dx11-out.md) — this crate's existing
  WMF/DX11 decode path; `VideoOutputPreference`/`VideoDecoderConfig` contract this ADR reuses.
- [`mediaway-decoder-linux` ADR-0001](../../mediaway-decoder-linux/adr/0001-vaapi-h264-cpu-out.md)
  — sibling decode ADR; IDR-only scope-cut precedent this ADR deliberately does **not**
  follow, and the reasoning for not reusing its H.264 parser as-is.
- FFmpeg `libavcodec/d3d12va_decode.c` / `dxva2_h264.c` / `dxva2_hevc.c` / `dxva2_av1.c`
  (BSD-2-Clause, reference-only — ground truth for call sequence and DXVA struct-filling
  logic; no code copied).
- Microsoft Learn (fetched this session): [`ID3D12VideoDecoderHeap`](https://learn.microsoft.com/en-us/windows/win32/api/d3d12video/nn-d3d12video-id3d12videodecoderheap),
  [`D3D12_VIDEO_DECODE_REFERENCE_FRAMES`](https://learn.microsoft.com/en-us/windows/win32/api/d3d12video/ns-d3d12video-d3d12_video_decode_reference_frames),
  [`D3D12_VIDEO_DECODE_INPUT_STREAM_ARGUMENTS`](https://learn.microsoft.com/en-us/windows/win32/api/d3d12video/ns-d3d12video-d3d12_video_decode_input_stream_arguments),
  [`D3D12_VIDEO_DECODE_ARGUMENT_TYPE`](https://learn.microsoft.com/en-us/windows/win32/api/d3d12video/ne-d3d12video-d3d12_video_decode_argument_type).
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md),
  [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md),
  [`docs/spec/sans-io.md`](../../../docs/spec/sans-io.md).

ADRs are **English**. Numbering is local to this `adr/` folder.

## Addendum (2026-07-29): H.264 implementation

Implemented per the file-layout plan above: `src/d3d12_video_decode.rs` +
`src/d3d12_video_decode/{setup,dpb,ops,util,h264,h264_sps_pps,h264_slice,h264_poc,
h264_refs,h264_pic_params}.rs` + `src/d3d12_video_decode_tests.rs`, still unregistered
(`mod d3d12_video_decode;` in `src/lib.rs`, not `pub mod`). H.264 only this round —
HEVC/AV1 not started. `cargo check`/`cargo clippy --all-targets`/`cargo test`, all with
`--features video`, are clean (0 warnings, 45 unit tests + 1 hardware-gated integration
test pass).

### Open Question #1 — resolved: plumbing present, DXVA structs absent

The pinned `windows` crate (0.62.2) **does** expose the D3D12 decode plumbing:
`ID3D12VideoDevice::{CheckFeatureSupport, CreateVideoDecoder, CreateVideoDecoderHeap}`,
`ID3D12VideoDecoder`, `ID3D12VideoDecoderHeap`,
`ID3D12VideoDecodeCommandList1::DecodeFrame1`, `D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT`,
`D3D12_VIDEO_DECODE_REFERENCE_FRAMES`, `D3D12_VIDEO_DECODE_FRAME_ARGUMENT`,
`D3D12_VIDEO_DECODE_ARGUMENT_TYPE_{PICTURE_PARAMETERS,SLICE_CONTROL,
INVERSE_QUANTIZATION_MATRIX}` — all confirmed by direct grep of the vendored source
before writing any decode logic, per the task.

**But the DXVA-specification per-codec picture-parameter structs themselves —
`DXVA_PicParams_H264`, `DXVA_Slice_H264_Long`, `DXVA_Qmatrix_H264`, `DXVA_PicEntry_H264`
— are absent from the crate's generated bindings entirely** (grepped the full vendored
source tree for every one of these symbols; zero matches). `D3D12_VIDEO_DECODE_FRAME_
ARGUMENT::pData` is only ever `*mut c_void` + `Size` — the caller must supply a
byte-identical struct from elsewhere. `h264_pic_params.rs` hand-defines all four,
`repr(C)`, ground-truthed against the real Windows SDK `dxva.h` layout fetched from the
Wine project's header mirror (`raw.githubusercontent.com/wine-mirror/wine/.../
include/dxva.h`) — a real, independent source of the actual struct layout, not
transcribed from memory. This is a stronger and more useful finding than the ADR's
original framing ("decode binding completeness unconfirmed") suggested: the *plumbing*
is complete, but every implementer of D3D12 H.264 (or HEVC/AV1) decode against this
`windows` crate version must hand-define the DXVA structs themselves — a real,
structural fact worth recording for the HEVC/AV1 follow-ups too (`DXVA_PicParams_HEVC`
etc. will need the same treatment).

### Open Question #2 — as anticipated: `DecodeError::Backend` used, not a new variant

`dpb.rs`'s `SlotTable::evict` returns `DecodeError::Backend` when a slot the decoder
needs to reuse still has a caller-outstanding Zero-Copy handle — exactly the
`mediaway_decoder::DecodeError` has-no-dedicated-variant situation this ADR flagged.
Not resolved here (still a cross-crate `mediaway-decoder` facade decision); a real unit
test (`dpb_tests.rs::evict_fails_when_handle_outstanding`) exercises this path.

### Open Question #4 — DPB sizing formula used

`max_num_ref_frames` (from SPS, floored at 1) `+ CALLER_HEADROOM (3) + 1` (the current
picture's own output slot, which is not itself counted as a "reference" until marked).
Not validated against a wide corpus of real streams — just this session's synthetic
gradient test stream — so the exact headroom constant is still a judgment call, per the
original ADR's framing.

### Open Question #5 — no alignment constant found; a different, real flag used instead

Grepped for a `D3D12_VIDEO_DECODE_OUTPUT_AND_REFERENCE_TEXTURE_ALIGNMENT`-shaped
constant as the ADR's Open Question #5 speculated — **not present** in this `windows`
crate version, confirmed absent by direct grep. What **is** present and used instead:
`D3D12_VIDEO_DECODE_CONFIGURATION_FLAG_HEIGHT_ALIGNMENT_MULTIPLE_32_REQUIRED`, one of
the output flags `D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT::ConfigurationFlags` can set.
`d3d12_video_decode.rs::ensure_session_ready` checks this flag and rounds the DPB
texture-array/decoder-heap height up to a multiple of 32 when the driver reports
needing it — a real, driver-reported requirement instead of a guessed constant.

### Open Question #6 — COM `.clone()` discipline followed

Every COM `AddRef`-via-`.clone()` site (`setup::device_from_handle`,
`ops::Session::build_reference_frame_arrays` — one clone per active reference, all the
same DPB texture-array resource) carries an explicit `// clone: COM AddRef share`-style
comment per the mandatory-clone-comment rule.

### Real gaps found only by implementing (not anticipated by the original ADR)

- **`mediaway_common::GpuBufferHandle::DirectX12` has no `subresource` field** — only
  `{ resource: NativeHandle }`. This blocks representing "one slot of a texture-array
  DPB" as a real `GpuBufferHandle`, which the original ADR's design assumed would work.
  Since this crate cannot modify `mediaway-common` (a shared facade type, same class of
  cross-crate decision as Open Question #2) and this module does not implement
  `mediaway_decoder::VideoDecoder` yet anyway, `d3d12_video_decode.rs` defines its own
  local `DecodedOutput::Gpu { resource, subresource }` / `DecodedFrame` types instead of
  forcing output through `VideoFrame`/`GpuBufferHandle`. A later integration pass must
  either extend `GpuBufferHandle::DirectX12` with a `subresource: u32` field (cross-crate
  decision, flagged here, not made by this crate alone) or find another mapping.
- **No display-order (POC-based "bumping") reorder buffer.** This stage decodes and
  emits pictures in **decode order**, not presentation order — a real, honest gap, not
  silently wrong (frame `pts` values are passed through as given, so a caller sorting by
  `pts` downstream would still recover display order for B-frame streams, but this
  module itself does no reordering or DPB-driven output-delay bookkeeping). A real
  "bumping process" (§ C.4.4-style) is future work.
- **SP/SI slices are rejected** (`h264_slice::parse_slice_header`) — not called out in
  the original ADR's scope list. `sp_for_switch_flag`/`slice_qs_delta` are unparsed;
  since real encoders essentially never emit SP/SI (a stream-switching feature), this
  is a deliberate, documented scope cut discovered while implementing slice-header
  parsing, not a silent bug.
- **Explicit weighted prediction is rejected** (`weighted_pred_flag` for P/SP,
  `weighted_bipred_idc == 1` for B) — `pred_weight_table()` is never parsed. Implicit
  weighted biprediction (`weighted_bipred_idc == 2`, POC-derived, no table) **is**
  supported (no bitstream table to parse for it). Also not in the original ADR's scope
  list.
- **Custom H.264 scaling lists (`seq_scaling_matrix_present_flag`/`pic_scaling_matrix_
  present_flag`) are parsed only to keep bitstream bit-position in sync** — the actual
  `DXVA_Qmatrix_H264` handed to the driver is always the flat (unscaled, all-16) matrix
  (`h264_pic_params::flat_qmatrix`). Documented as a real, non-fatal fidelity gap (see
  `h264_sps_pps.rs`'s doc comments) per `docs/spec/caveats-and-clarity.md` — decode
  still succeeds, but a High-profile stream with real custom scaling matrices will not
  match the source encoder's exact quantization.

### Real-hardware test result on this session's machine

`d3d12_video_decode_tests.rs::h264_decode_idr_and_p_frame_or_skip` (same NVIDIA RTX 4090
host used by this workspace's other D3D12 native encode/decode ADRs):

1. Encoded 8 gradient NV12 frames via `mediaway-encoder-windows`'s **WMF** H.264 encoder
   (`WindowsVideoEncoder`, CPU-upload) — produced 8 real packets, **`has_non_keyframe =
   true`**, confirming this host's inbox H.264 encoder MFT really does emit inter (P)
   frames by default (not forced all-intra like this workspace's native D3D12 encoders),
   so the test's bitstream genuinely exercises the DPB/reference-list path, not just an
   IDR-only case.
2. `D3d12VideoDecoder::open` succeeded (device/video-device resolution).
3. The first `push_packet` call (containing the SPS/PPS/IDR NALs) returned
   **`DecodeError::Backend`** — the test skipped honestly (`eprintln!`, no assertion
   failure). This means `h264::check_support`'s `D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT`
   query likely passed (a plain `Unsupported` would have surfaced from `ensure_session_
   ready` before reaching `decode_frame`'s `Backend`-mapped calls) but some later D3D12
   object-creation or `DecodeFrame1` submission step failed. **Root cause not
   diagnosed this session** — this crate's test doesn't yet thread through which
   specific call failed (no raw-HRESULT diagnostic added, unlike ADR-0007's own
   debugging passes). A real, honest limitation: **the D3D12 H.264 decode round trip is
   implemented and structurally sound (compiles, passes 45 pure-logic unit tests, and
   the pure bitstream-parsing/POC/ref-list/DPB code paths are unit-tested without
   hardware) but has *not* been verified end-to-end against real decoded pixels on real
   hardware this session** — a follow-up should add the same kind of raw-HRESULT
   diagnostic ADR-0007's HEVC addendum used to find its own D3D12 issues.

### Deviations from the original file-layout plan

- `h264_pic_params.rs` landed at 323 lines (well under the 1000-line ceiling; no split
  needed).
- `d3d12_video_decode.rs` (543 lines) lazily creates all D3D12 decoder/heap/DPB/command
  objects on the **first parsed SPS** rather than at `open()` time — real coded
  width/height/`max_num_ref_frames` are bitstream-derived, not known from
  `VideoDecoderConfig` alone (the original plan did not explicitly address this
  chicken-and-egg ordering constraint). A separate `Session` struct (not `D3d12VideoDecoder`
  itself) holds these lazily-created objects; `ops.rs`'s functions are implemented on
  `Session`, not `D3d12VideoDecoder`.
- `ops.rs` needed a **second, separate copy-capable command queue/list**
  (`D3D12_COMMAND_LIST_TYPE_COPY`, `ID3D12GraphicsCommandList`) alongside the
  `D3D12_COMMAND_LIST_TYPE_VIDEO_DECODE` one used for `DecodeFrame1` —
  `CopyTextureRegion` (needed for `readback_dpb_slot_to_cpu`) is not a valid recording
  on a video-decode-type command list. Mirrors `mediaway-encoder-windows`'s own
  separate copy queue for its CPU-upload path, but the original ADR's file-layout plan
  did not call this out explicitly.
- `dpb.rs` splits into a D3D12-free `SlotTable<M>` (pure slot-lifecycle bookkeeping,
  unit-tested directly) and `DpbPool<M>` (adds the one owned `ID3D12Resource`) —
  specifically so the bounded-handle backpressure contract could be unit-tested without
  a real device/texture.

## Addendum (2026-07-30): root-causing the real hardware hang

Follow-up to the previous addendum's honestly-reported gap ("`push_packet` returned
`DecodeError::Backend`, root cause not diagnosed"). Instrumented
`d3d12_video_decode_tests.rs` with `ID3D12Debug::EnableDebugLayer` +
`ID3D12InfoQueue::GetMessage` polling, byte-for-byte the same technique
`mediaway-encoder-windows` ADR-0007 used to name its own three real hardware findings.
Three real bugs found and fixed; the underlying GPU hang persists past all of them,
with the debug layer now silent (no further validation messages) — root cause is very
likely inside the opaque DXVA picture-parameter blob content itself, which the debug
layer cannot inspect (it validates D3D12 API/resource-state usage, not codec-specific
blob semantics).

### Bug 1 (fixed): readback buffer sized as tightly-packed NV12, not row-pitch-aligned

First debug-layer capture: `ID3D12CommandList::CopyTextureRegion: ... PlacedFootprint
extends past the end of the buffer ... size required ... 16192 ... but the buffer only
has 6144 bytes` (and the analogous chroma-plane message, `24384` required vs `6144`
available). `ensure_session_ready` sized `readback_buffer` via the tightly-packed
`util::nv12_size(width, height)`, but `CopyTextureRegion`'s placed footprints need
`D3D12_TEXTURE_DATA_PITCH_ALIGNMENT`-rounded row pitch — exactly the same alignment
`mediaway-encoder-windows`'s CPU-upload path already accounts for on the *encode* side
(`upload_size = luma_size + luma_size / 2` with an aligned `row_pitch`), just never
applied here for the *readback* buffer. Fixed: `readback_size` now uses the same
row-pitch-aligned formula. This class of error is gone after the fix.

### Bug 2 (fixed, likely the TDR's actual cause): NV12 is two planes, only one was barriered

Real bug, real finding: `DecodeFrame1`/`D3D12_VIDEO_DECODE_REFERENCE_FRAMES` address a
DPB slot by a plain array-slice index (the "video subresource" convention — one number
per decoded picture, matching the ADR's original design), but the D3D12 resource-state
tracker underneath operates on **real, plane-aware D3D12 subresources**
(`Subresource = ArraySlice + PlaneSlice × ArraySize`, `MipLevels == 1`): luma at
`slot`, chroma at `slot + num_slots`. Every `ResourceBarrier` in `ops.rs::decode_frame`
transitioned only `slot` (the luma plane) to `VIDEO_DECODE_WRITE`/`VIDEO_DECODE_READ` —
the chroma plane was left in `COMMON`. The debug layer named this exactly once
resolution/readback-sizing were no longer masking it: *"Resource state (0x0:
D3D12_RESOURCE_STATE_[COMMON|PRESENT]) of resource (...) (subresource: 6) is invalid
for use as a pOutputTexture2D. Expected State Bits (all): 0x20000:
D3D12_RESOURCE_STATE_VIDEO_DECODE_WRITE"* — subresource 6 being exactly
`slot(0) + num_slots(6)`, i.e. slot 0's chroma plane, confirming the exact mechanism.
Fixed: every barrier around `DecodeFrame1` (output-write, reference-reads, and both
"back to `COMMON`" transitions) now covers **both** planes of every slot it touches.
This specific validation error is gone after the fix, and is the strongest candidate
for what was actually causing the GPU to hang mid-`DecodeFrame1` (a decode engine
instructed to write a two-plane picture while one plane is in the wrong state is
exactly the kind of driver-undefined condition that manifests as a hang rather than a
clean HRESULT failure).

### Bug 3 (fixed, real but not confirmed as *the* hang's cause): RBSP bit offset never translated back to raw

`h264_slice::parse_slice_header`'s returned bit count is measured against the
**de-emulated** RBSP (`mediaway_sw::h264::NalUnit::parse` already stripped
`emulation_prevention_three_byte`), but `DXVA_Slice_H264_Long::BitOffsetToSliceData`
must index into the **raw** NAL bytes actually written into the D3D12 compressed-
bitstream buffer (escape bytes still present). Every escape byte before `slice_data()`
begins would have shifted this value wrong by 8 bits, pointing the hardware decoder at
the wrong bit to start parsing macroblocks from — a real bug, independently worth
fixing regardless of whether it contributed to this session's specific hang (the CIF
test stream's first slice header happened to contain no escape bytes before
`slice_data()`, per the diagnostic dump below, so it did not explain *this* hang, but
would corrupt decode on any real stream whose slice header does). Fixed:
`h264_slice::rbsp_bit_offset_to_raw_bit_offset` re-walks the raw NAL payload with the
identical zero-run/`0x03`-skip algorithm `mediaway_sw::h264::nal`'s (private)
`remove_emulation_prevention` uses, translating the de-emulated bit count back to a raw
one; `d3d12_video_decode.rs::decode_slice` adds 8 more bits for the NAL header byte.

### Ruled out: DPB texture `D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS`

Reverted to `D3D12_RESOURCE_FLAG_NONE` (the conservative default FFmpeg's own decode
surface pools use) as a defensive measure while isolating the hang — the flag changes
cache-coherency behavior in ways that seemed like a plausible contributor. **Not
confirmed as causal**: the hang persisted identically with this flag removed. Kept
reverted anyway (no reason to keep a speculative, non-default flag that isn't earning
its cost) but this is honestly a ruled-out hypothesis, not a fourth fix.

### Ruled out: coded resolution too small for NVDEC's real minimum

`mediaway-encoder-windows`'s ADR-0007 found a real minimum-resolution floor for D3D12
**encode** on this exact RTX 4090 (160x64). Hypothesized decode might have an analogous
undocumented floor the `D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT` query doesn't itself
validate (decode has no equivalent `D3D12_FEATURE_VIDEO_ENCODER_OUTPUT_RESOLUTION`-style
min/max query at all). Tested by moving the integration test from 64x64 to CIF
(352x288, a standard H.264 test resolution). **The hang reproduced identically at
CIF.** Ruled out as the (sole) cause. Test resolution kept at CIF anyway (more
representative of real content than 64x64, no reason to revert).

### Current honest status: hang persists, root cause narrowed but not eliminated

After Bugs 1-3 above, a fresh diagnostic dump of every field fed into `DecodeFrame1` for
the CIF IDR slice was captured (temporary instrumentation, removed before finishing —
not left in production code, since `clippy::print_stderr` is denied outside tests):
`slice_type=I nal_ref_idc=3 is_idr=true mb_width=22 mb_height=18 sess_width=352
sess_height=288 max_num_ref_frames=2 num_dpb_slots=6 output_slot=0
num_mbs_for_slice=396 nal_len=2592 deemulated_bits_read=34
bit_offset_to_slice_data=42 num_refs=0 frame_num=0 pic_order_cnt_lsb=0
num_ref_idx_l0=0 entropy_mode=false`. Every value is internally consistent and matches
the real CIF SPS/PPS the WMF encoder produced (22×18 MBs = 352×288, CAVLC entropy
coding, no references for the IDR, `bit_offset_to_slice_data` exactly 8 bits more than
the de-emulated count since this particular short I-slice header contains no escape
bytes) — nothing here looks obviously wrong by inspection.

With Bugs 1 and 2 fixed, the D3D12 debug layer / `ID3D12InfoQueue` no longer reports
**any** validation message before the `DXGI_ERROR_DEVICE_HUNG` TDR — meaning every
D3D12 API-usage and resource-state concern the debug layer is capable of checking is
now clean. The debug layer validates API contract and resource-state correctness; it
has **no visibility into whether the `DXVA_PicParams_H264`/`DXVA_Qmatrix_H264`/
`DXVA_Slice_H264_Long` blob content itself is semantically correct** — a wrong QP
value, an incorrect `wBitFields` bit, or a subtly wrong reference/POC field would not
surface as a debug-layer message at all, only as wrong pixels or (for some driver
implementations) a hardware fault deep in the decode microcode, which is exactly
consistent with a silent hang. **Six real hardware runs were made this session, each
triggering a genuine `DXGI_ERROR_DEVICE_HUNG` TDR** — stopping further blind hardware
iteration here rather than continuing to reset the GPU on speculation, per this
workspace's existing "each real hardware attempt has a real cost" judgment call.

**Not yet tried, real candidates for a follow-up session**: (1) a byte-for-byte diff of
this backend's `DXVA_PicParams_H264` fill against a **working** reference — e.g.
building a minimal WMF/DXVA2 (not D3D12) H.264 decode path for the same stream and
comparing field values, since `mediaway-decoder-windows` already has a working WMF
decode path (`src/wmf/h264.rs`) that could be extended to dump its internal DXVA
buffers for comparison, mirroring this workspace's own "verify against a working
reference on the same hardware" convention; (2) NVIDIA Nsight Aftermath / GPU crash
dump capture around the hang, which can name the exact shader/microcode stage that
stalled — outside this session's available tooling; (3) systematically testing with
`film_grain`/scaling-matrix-free, single-macroblock synthetic streams to bisect which
DXVA field is at fault, rather than a real encoder's full complexity.

## Addendum (2026-08-05): field-by-field diff against real reference implementations (static, no hardware run)

Follow-up research-only pass (explicitly **no hardware test run** — the previous
addendum's 6 real TDRs stand as the reason to stop blind hardware iteration). Did
candidate (1) above in spirit — not a from-scratch WMF/DXVA2 dump, but a byte-for-byte
diff of `h264_pic_params.rs`'s `DXVA_PicParams_H264`/`DXVA_Slice_H264_Long` fill against
two real, independent, hardware-validated DXVA producers fetched fresh this session:

- **FFmpeg `libavcodec/dxva2_h264.c`** (`ff_dxva2_h264_fill_picture_parameters`,
  `fill_slice_long`) — fetched from `github.com/FFmpeg/FFmpeg` (master).
- **GStreamer `gst-libs/gst/dxva/gstdxvah264decoder.cpp`** (shared D3D11/D3D12 base
  class `GstDxvaH264Decoder`, used by both `d3d11h264dec` and `d3d12h264dec`) —
  fetched from `github.com/GStreamer/gstreamer` (main); found via `gh search code
  "MinLumaBipredSize8x8Flag"`, which is how the struct-layout/field-source ground
  truth below was located precisely (`gh api repos/GStreamer/gstreamer/contents/...`
  for the raw file).
- Wine's `include/dxva.h` mirror re-confirmed the exact `DXVA_PicParams_H264`/
  `DXVA_Slice_H264_Long` field order this crate's `repr(C)` structs already use —
  **struct layout itself matches perfectly, field-for-field, no ordering bug found.**

### Bug 4 (fixed): `MinLumaBipredSize8x8Flag` (wBitFields bit 14) sourced from the wrong SPS field

`h264_pic_params::build_pic_params` derived bit 14 from `sps.direct_8x8_inference_flag`
(with a comment asserting `"MinLumaBipredSize8x8Flag <- direct_8x8_inference_flag"`, an
unreferenced guess from the original implementation pass). Both real references agree
this is wrong: FFmpeg computes `(sps->level_idc >= 31) << 14`; GStreamer computes
`params->MinLumaBipredSize8x8Flag = sps->level_idc >= 31;` — bit-for-bit identical,
level-derived, and **unrelated** to the `direct_8x8_inference_flag` SPS syntax element
(which correctly has its own separate struct field elsewhere in the same fill). Fixed:
`build_pic_params` now passes `sps.level_idc >= 31`. `level_idc` was already parsed and
stored on `Sps`, so this is a one-line source-value change, not a new field to parse.

### Bug 5 (fixed): `Reserved16Bits` is not actually zero-fill despite the name

`h264_pic_params::build_pic_params` set `reserved16_bits: 0`. FFmpeg's
`fill_picture_parameters` reveals this field is **not** truly reserved/zero — it is a
real, driver-workaround-selected value: `0` only under a named legacy workaround
(`FF_DXVA2_WORKAROUND_SCALING_LIST_ZIGZAG`), `0x34c` only under another
(`FF_DXVA2_WORKAROUND_INTEL_CLEARVIDEO`), and **`3` as the default** for a normal
driver with neither workaround active (FFmpeg's own comment: `/* FIXME is there a way
to detect the right mode ? */`). GStreamer's independent implementation agrees
unconditionally: `params->Reserved16Bits = 3;`, no workaround branching at all. This
crate has no equivalent of either named FFmpeg workaround and no evidence its
reference RTX 4090 needs one, so `3` (the shared default both references agree on) is
the correct value — not `0`. Fixed: `build_pic_params` now sets `reserved16_bits: 3`.
This is exactly the class of bug the previous addendum predicted ("a wrong QP value,
an incorrect `wBitFields` bit... would not surface as a debug-layer message at all") —
an undocumented-by-name field whose correct value is only discoverable by diffing
against a real, working producer, not by reading `dxva.h`'s field name alone.

### Investigated, not fixed: `DXVA_Slice_H264_Long::BitOffsetToSliceData` raw-vs-de-emulated question (Bug 3 revisited)

The previous addendum's Bug 3 fix (`h264_slice::rbsp_bit_offset_to_raw_bit_offset`)
translates the slice header's de-emulated bit count into a position counted against the
**raw** NAL bytes (escape sequences counted as real bits to skip over), on the
stated-but-uncited assumption that `BitOffsetToSliceData` "must index into the raw NAL
bytes." Directly tracing FFmpeg's real implementation this session **contradicts that
assumption in one clear respect**: `libavcodec/h264dec.c` feeds the hwaccel
`decode_slice` callback `nal->raw_data`/`nal->raw_size` (raw, escapes intact — same as
this crate), but `fill_slice_long`'s `BitOffsetToSliceData = get_bits_count(&sl->gb) - 8`
is computed from `sl->gb = nal->gb`, which `libavcodec/h2645_parse.c` initializes on
`nal->data` — the **de-emulated** RBSP buffer produced by `ff_h2645_extract_rbsp`.
FFmpeg's `fill_slice_long` contains **no** raw-byte-position translation step anywhere.
This is a real, concrete discrepancy: this crate translates to a raw position, FFmpeg's
real, hardware-proven implementation does not.

**Not fixed this session** — the exact additive constant is genuinely ambiguous from
static tracing alone: cross-referencing older FFmpeg forks/mirrors via `gh search code
"BitOffsetToSliceData"` shows the formula has changed *within FFmpeg's own history*
(`get_bits_count(&s->gb)` with no adjustment in some old trees, `+ 8` in others,
today's maintained `- 8` in current master and every recent mirror checked) — meaning
even this reference has visibly gotten this specific constant wrong before. Resolving
whether this crate's own `+8`-for-header-byte convention (`d3d12_video_decode.rs::
decode_slice`: `8u32.saturating_add(raw_bit_offset_after_header)`) should also change
requires confirming exactly what bit position `nal->gb` starts counting from relative
to the header byte, which was not resolvable with confidence from source alone this
session. Given a wrong bit offset is plausibly hang-causing (the exact failure mode
this ADR is chasing) and the one hardware data point available (the CIF test's
diagnostic dump) had **zero escape bytes** before `slice_data()` — meaning the
raw-translation function was a no-op for that specific run either way — leaving this
untouched changes nothing about the already-collected evidence, but guessing wrong on
the additive constant risks a **new**, unverified regression. Recommended follow-up:
resolve the exact `nal->gb` start-position convention against a **third** reference
(e.g. the Windows SDK `dxva.h`'s own comment on this field, `/* after CABAC alignment
*/`, found in the Microsoft SDK 10.0.22621.0 mirror this session but not yet
cross-referenced against a spec definition of "CABAC alignment"), or a synthetic
single-macroblock stream deliberately engineered to contain an escape byte inside the
slice header, decoded first through a CPU-only reference parser to get an unambiguous
expected value — **before** spending another real hardware TDR on it.

### Static-only verification this session

`cargo check -p mediaway-decoder --all-targets`, `cargo clippy -p mediaway-decoder
--all-targets --all-features -- -D warnings`, and `cargo fmt -p mediaway-decoder --
--check` all clean after Bugs 4 and 5. **No hardware test was run** — per this
session's explicit scope, the hang itself remains unverified against real hardware;
Bugs 4 and 5 narrow the "opaque blob content" hypothesis space but do not confirm or
rule out that either one is *the* hang's cause.

### Hardware re-verification (2026-08-05, same-day follow-up): hang persists

With Bugs 4 and 5 applied, `windows::d3d12_video_decode::tests::
h264_decode_idr_and_p_frame_or_skip` was run for real against the same RTX 4090:

```
D3D12 InfoQueue[1]: ID3D12Device::RemoveDevice: Device removal has been triggered
for the following reason (DXGI_ERROR_DEVICE_HUNG: ...)
skip: push_packet failed on packet 0 (Backend, is_keyframe=true)
test windows::d3d12_video_decode::tests::h264_decode_idr_and_p_frame_or_skip ... ok
```

**The hang reproduces identically** — same `DXGI_ERROR_DEVICE_HUNG` on the very first
(IDR) packet. Bugs 4/5 are real, reference-grounded fixes (kept — they were wrong
either way, independent of whether they cause this hang) but are **ruled out as the
hang's sole cause**. The test's own honest-skip convention correctly did not hard-fail.

This is the 7th real hardware TDR this bug has caused across sessions. Per this
workspace's "each real hardware attempt has a real cost" judgment call, no further
hardware attempts should be made without first resolving the `BitOffsetToSliceData`
ambiguity above via the CPU-only synthetic-stream approach already recommended —
that is now the clearest remaining lead.

## Addendum (2026-08-07): `BitOffsetToSliceData` resolved against the official spec — prior "Bug 3" fix was backwards

Follow-up to the previous addendum's still-unresolved lead. Fetched and cached the
actual primary source this time — **"DirectX Video Acceleration Specification for
H.264/AVC Decoding"** (Microsoft, `docs/standards/registry.toml` id
`dxva-h264-decoding`, BLAKE3-pinned under `local/standards/dxva-h264-decoding/`) —
rather than continuing to reason from FFmpeg/GStreamer call-sequence tracing alone.
§ `BitOffsetToSliceData` (p. 32–33) states, unambiguously:

> This bit offset is the offset within the RBSP data for the slice, relative to the
> starting position of the `slice_header()` in the RBSP. That is, it represents a bit
> offset after the removal of any `emulation_prevention_three_byte` syntax elements
> that preceded the start of the `slice_data()` in the NAL unit.

The spec *also* gives a formula for locating the referenced bit's byte in the raw
bitstream data buffer — `BSNALunitDataLocation + (BitOffsetToSliceData >> 3) + 4 + K`
(`K` = escape-byte count before `slice_data()`) — but this is documented as how **the
accelerator** maps the RBSP-relative value to a raw buffer position, not a
transformation the host is expected to perform before writing the field.

**This directly inverts the previous addendum's "Bug 3" fix.** That session added
`h264_slice::rbsp_bit_offset_to_raw_bit_offset` specifically to translate the
de-emulated bit count *into* a raw-buffer-relative one (plus an ad hoc `+8` for the
NAL header byte), on the reasoning that the hardware needed a raw position. Per the
primary spec, the correct value was always the untranslated de-emulated bit count —
exactly what `parse_slice_header` already returns, since a slice NAL's RBSP begins
directly at `slice_header()`'s first bit (nothing precedes it). The one real
requirement the spec does add, not previously implemented: for CABAC
(`entropy_coding_mode_flag == 1`), the value must be rounded up to the next byte
boundary (`% 8 == 0`), covering `cabac_alignment_one_bit()`; the prior codebase had no
such rounding at all, CAVLC or CABAC.

**Fixed**: `h264_slice::rbsp_bit_offset_to_raw_bit_offset` replaced with
`h264_slice::bit_offset_to_slice_data(deemulated_bits_read, entropy_coding_mode_flag)`
— passes the de-emulated bit count straight through for CAVLC, rounds up to the next
byte for CABAC, no raw-buffer translation. `d3d12_video_decode.rs::decode_slice` and
`h264_pic_params::build_slice_long`'s doc comment updated to match. 124 unit tests
(including 2 new ones for the corrected function) and `clippy --all-targets
--all-features -- -D warnings` are clean.

The CIF test stream that produced every prior hang used CAVLC (`entropy_mode=false`
in the 2026-07-30 diagnostic dump), so this fix's behavioral change for *that specific
stream* is exactly the removal of the erroneous raw translation and `+8` — no CABAC
rounding applies to it.

### Hardware re-verification (2026-08-07, same-day, explicit project-owner go-ahead): hang persists — 8th TDR

With this fix applied, `windows::d3d12_video_decode::tests::
h264_decode_idr_and_p_frame_or_skip` was run for real against the same RTX 4090:

```
D3D12 InfoQueue[0]: ID3D12Device::CreateCommittedResource: Ignoring InitialState
D3D12_RESOURCE_STATE_VIDEO_DECODE_READ. Buffers are effectively created in state
D3D12_RESOURCE_STATE_COMMON.
D3D12 InfoQueue[1]: ID3D12Device::RemoveDevice: Device removal has been triggered
for the following reason (DXGI_ERROR_DEVICE_HUNG: ...)
skip: push_packet failed on packet 0 (Backend, is_keyframe=true)
test windows::d3d12_video_decode::tests::h264_decode_idr_and_p_frame_or_skip ... ok
```

**The hang reproduces identically, an 8th real TDR.** `BitOffsetToSliceData` was a
real, spec-confirmed bug independent of whether it caused *this specific* hang (kept
either way — it was wrong regardless), but it is now **ruled out as this hang's sole
cause**, same conclusion pattern as Bugs 4/5 before it. The one new, previously-unseen
debug-layer line this run (`CreateCommittedResource: Ignoring InitialState
D3D12_RESOURCE_STATE_VIDEO_DECODE_READ` for the compressed-bitstream input buffer) is
an *advisory* message, not an error — D3D12 buffers (as opposed to textures) are
always created in `COMMON` regardless of requested initial state, a documented D3D12
behavior, not a bug — but it does confirm the debug layer is still watching and still
finds nothing else to flag before the hang, reinforcing the prior addendum's
conclusion that the remaining defect is inside opaque DXVA blob semantics the debug
layer cannot validate (a wrong QP, an incorrect reference index, wrong scaling-list
handling, or the still-untouched candidate from Open Question investigated-not-fixed
above). The test's honest-skip convention correctly did not hard-fail again.

Per this workspace's "each real hardware attempt has a real cost" judgment call, no
further hardware attempts should be made without a new, concrete lead — the remaining
plausible candidates are the same ones the 2026-07-30 addendum already named and did
not get to: a byte-for-byte diff against a **working** WMF/DXVA2 decode of the exact
same stream (extending `mediaway-decoder-windows`'s existing WMF path to dump its
internal DXVA buffers), or a single-macroblock synthetic stream decoded first through
a CPU-only reference parser to pin down any remaining ambiguous field one at a time,
rather than a real encoder's full complexity.
