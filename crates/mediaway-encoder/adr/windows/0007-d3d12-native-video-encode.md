# ADR-0007: D3D12 native video encode (H.264, CPU-upload)

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder-windows`

## Context

Windows 11 exposes a **second, independent** hardware video-encode path alongside Media
Foundation: `ID3D12VideoDevice3::CreateVideoEncoder`/`CreateVideoEncoderHeap` +
`ID3D12VideoEncodeCommandList2::EncodeFrame`. This is a real D3D12 API surface (H.264/HEVC
since launch, AV1 since 24H2/WDDM 3.2), reachable through the `windows` crate at zero new
dependency cost — the relevant types live under `windows::Win32::Media::MediaFoundation`
(a metadata-bucketing artifact of `d3d12video.h`, not `Win32::Graphics::Direct3D12`), and
this crate's `Cargo.toml` already enabled both `Win32_Media_MediaFoundation` and
`Win32_Graphics_Direct3D12`.

This is **not** the same path as [`D3d12SharedEncodeBridge`](../src/d3d12_share.rs)
(ADR-0006): that bridge feeds a D3D12 texture into **Media Foundation**'s HW MFT via a
shared D3D12→D3D11 heap (`GpuCopy`) — still WMF underneath. The API described here drives
the **native D3D12 video-encode pipeline** end to end; no `IMFTransform`, no WMF session at
all.

## Decision

> Add [`d3d12_video_encode`](../src/d3d12_video_encode.rs) (+ sibling `bitstream`/`setup`/
> `ops`/`util` files, split to stay under the 1000-line source limit): a self-contained,
> **currently unregistered** backend implementing H.264 CPU-upload NV12 encode via the
> native D3D12 video-encode API.

### Scope this stage (mirrors `mediaway-encoder-linux`'s VA-API staging order)

- **H.264 Main profile only.** CPU-upload NV12 input — one CPU→GPU upload copy per frame
  (`D3D12_COMMAND_LIST_TYPE_COPY` queue, `CopyTextureRegion` into a `DEFAULT`-heap NV12
  texture), fully synchronous (`ID3D12Fence` signal + CPU wait) per frame, matching this
  stage's "not performance-obsessed" CPU-upload contract.
- **Every pushed frame is an independent IDR** — `GOPLength = 1`, zero reference frames in
  `D3D12_VIDEO_ENCODER_PICTURE_CONTROL_DESC` (`ReconstructedPicture` is `None`, per the
  API's documented "only needed if used as a reference" contract). No GOP, no P/B-frames,
  no DPB management. Matches VA-API backend's own "every frame independent IDR" scope cut.
- **Fixed CQP rate control** (`FIXED_QP = 26`). `bitrate_bps` is not honored yet.
- **Hand-written Annex-B SPS/PPS** ([`bitstream.rs`](../src/d3d12_video_encode/bitstream.rs)):
  the D3D12 API only ever emits the **slice** NAL into the compressed-bitstream buffer
  (starting at `FrameStartOffset`) — parameter sets are the application's responsibility.
  `pic_order_cnt_type == 2` (no POC LSB tables), no CABAC, no VUI, no scaling lists — the
  minimal valid SPS/PPS for this configuration. Prepended to every packet (not cached as
  stream `extra_data`), since every packet is independently decodable.
- **Deferred**: Zero-Copy GPU input (`VideoInputPreference::ZeroCopyGpu`), HEVC/AV1,
  reference-frame/GOP support, rate-control tuning (CBR/VBR/QVBR), CABAC, the fuller
  `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` codec-configuration capability probing FFmpeg's
  `d3d12va_encode_h264.c` does (deblocking-mode/CABAC support-flag checks) — this backend
  always requests the same fixed, universally-supported `codec_conf_h264` (CAVLC, standard
  deblocking, no 8x8 transform).

### Real-hardware findings (RTX 4090, this session)

Ground-truthed against FFmpeg's shipped `libavcodec/d3d12va_encode.c` /
`d3d12va_encode_h264.c` (BSD-licensed reference implementation of this exact API) and
verified live against real hardware. Two non-obvious, driver-real behaviors this
implementation had to discover and correct for:

1. **`D3D12_FEATURE_VIDEO_ENCODER_OUTPUT_RESOLUTION` reports a real minimum encode
   resolution** — observed `MinResolutionSupported = 160x64` on the RTX 4090.
   Below it, `CreateVideoEncoderHeap` fails with a bare `E_INVALIDARG` and no other
   diagnostic. [`setup::check_output_resolution`](../src/d3d12_video_encode/setup.rs) now
   queries and validates against this (and `Min`/`MaxResolutionSupported`,
   `Resolution{Width,Height}MultipleRequirement`) before ever calling
   `CreateVideoEncoderHeap`, turning that failure into a clear
   [`EncodeError::Unsupported`](../../mediaway-encoder/src/error.rs).
2. **`D3D12_HEAP_TYPE_READBACK` resources cannot be transitioned to
   `D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE`/`_READ`** — the D3D12 debug layer enforces
   "resources on `READBACK` heaps support only `COMMON`/`COPY_DEST`/`RESOLVE_DEST`", a
   restriction keyed on the **abstract** heap-type enum value. FFmpeg's own
   `d3d12va_encode.c` sidesteps this by resolving the heap type through
   `ID3D12Device::GetCustomHeapProperties(0, D3D12_HEAP_TYPE_READBACK)` first — same
   physical memory / CPU `Map` semantics, but a `D3D12_HEAP_TYPE_CUSTOM` properties value
   that the abstract-type check doesn't fire on.
   [`setup::create_linear_buffer`](../src/d3d12_video_encode/setup.rs) does the same for
   every CPU-visible buffer (the compressed-bitstream and resolved-metadata outputs).
3. **`D3D12_FEATURE_VIDEO_ENCODER_SUPPORT`'s `SuggestedLevel` output is load-bearing** — a
   plausible-looking hardcoded H.264 level (Level 3.1, Level 5.1) reliably fails
   `CreateVideoEncoderHeap`; only the driver-reported `SuggestedLevel` for this exact
   codec/profile/GOP/rate-control/resolution combination works.
   [`setup::check_encoder_support`](../src/d3d12_video_encode/setup.rs) queries it and
   [`setup::level_h264_to_idc`](../src/d3d12_video_encode/setup.rs) maps the result to the
   `level_idc` byte the hand-written SPS needs.

Debugged via `ID3D12Debug::EnableDebugLayer` + `ID3D12InfoQueue::GetMessage` polling in the
test itself (`d3d12_video_encode_tests.rs`) — the debug layer's messages named the exact
failing call/resource/constraint in all three cases; the bare `HRESULT` alone did not.

### Verified end-to-end on real hardware

`d3d12_native_h264_encode_or_skip` ([`d3d12_video_encode_tests.rs`](../src/d3d12_video_encode_tests.rs))
creates a real `ID3D12Device` (adapter 0), checks real `ID3D12VideoDevice3` H.264 support,
opens a real session, pushes 3 synthetic 176x144 NV12 frames through the full pipeline
(CPU upload → `EncodeFrame` → `ResolveEncoderOutputMetadata` → readback), and asserts each
returned packet contains a real SPS NAL (`nal_unit_type == 7`) and a real driver-encoded
IDR slice NAL (`nal_unit_type == 5`) in valid Annex-B. **Passed on the RTX 4090**
(2026-07-29) — a genuine hardware encode, not a skip. Falls back to `eprintln!("skip: ...")`
(never fails the default suite) on machines/drivers without D3D12 H.264 video-encode
support, printing the D3D12 debug-layer's messages when available.

### Not wired into the public API yet

`src/lib.rs` declares `mod d3d12_video_encode;` (**not** `pub mod`) so this module and its
`#[cfg(test)]` hardware-gated tests actually compile and run as part of this crate's normal
`cargo test`/`cargo clippy` — this had been missing entirely (an oversight, not a deliberate
exclusion) until the AV1 addendum above added it; H.264/HEVC's real-hardware verification
claims earlier in this ADR were only reconfirmed once this declaration existed. A later
integration pass still owns making it `pub` and deciding how this path fits into
`AutoVideoEncoder`'s path selection alongside `WmfVideoEncoder` and
`D3d12SharedEncodeBridge` (e.g. a new `EncodePathClass`, or exposed only via direct
low-level `D3d12VideoEncoder::open`).

## Addendum (2026-07-29): HEVC encode, AV1 probe, decode deferred

Extends the scope above with HEVC Main-profile encode, same CPU-upload/all-intra/fixed-CQP
staging. Still self-contained/unregistered — same verification method (temporarily wired
into `src/lib.rs`, confirmed byte-identical revert after).

### HEVC — implemented and real-hardware verified

- New files: [`hevc`](../src/d3d12_video_encode/hevc.rs) (`open`-time feature queries +
  `ID3D12VideoEncoder`/`ID3D12VideoEncoderHeap` creation for
  `D3D12_VIDEO_ENCODER_CODEC_HEVC`), [`ops_hevc`](../src/d3d12_video_encode/ops_hevc.rs)
  (per-frame `EncodeFrame` recording), [`bitstream_hevc`](../src/d3d12_video_encode/bitstream_hevc.rs)
  (hand-written Annex-B VPS/SPS/PPS — HEVC needs a third parameter set and a 2-byte NAL
  header, different enough from H.264's that the existing `bitstream.rs` wasn't
  generalized, only its shared `RbspWriter`/emulation-prevention helpers were). The encoder
  struct's per-frame GOP state became a `GopStructure` enum (`H264(..)` / `Hevc(..)`) so
  `open`/`push_frame` dispatch on the active codec without a second `Option` field.
- `d3d12_native_hevc_encode_or_skip`
  ([`d3d12_video_encode_tests.rs`](../src/d3d12_video_encode_tests.rs)) pushes 3 synthetic
  256x192 NV12 frames through the full HEVC pipeline and asserts each packet contains real
  VPS (`nal_unit_type == 32`), SPS (`33`), PPS (`34`), and IDR slice (`19`/`20`) NALs.
  **Passed on the RTX 4090** (2026-07-29) — genuine hardware HEVC encode.

**Two more real-hardware findings, both specific to HEVC's codec-configuration struct**
(ground-truthed by sweeping `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT`'s `GENERAL_SUPPORT_OK`
flag across candidate configurations, then reading the D3D12 debug layer on the remaining
failure — see [`hevc.rs`](../src/d3d12_video_encode/hevc.rs)'s module doc for the full
sweep):

1. **`D3D12_FEATURE_VIDEO_ENCODER_CODEC_CONFIGURATION_SUPPORT` reports `IsSupported ==
   false` unconditionally for HEVC Main on this driver** — even though basic codec support,
   output-resolution, and resource-requirement queries all report supported. This is the
   query that would let this backend discover the driver's real coding-unit/transform-unit
   size range the way H.264's `SuggestedLevel` is queried (not hardcoded); it appears
   simply unimplemented for HEVC on this driver, not a genuine "unsupported" answer. Worked
   around by feeding candidate configurations directly into
   `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` instead (the query H.264's fixed codec config
   already relies on) and reading `ValidationFlags` on failure
   (`CODEC_CONFIGURATION_NOT_SUPPORTED`) to narrow the search.
2. **`MaxLumaCodingUnitSize` must be exactly `32x32`** — not `64x64` (the larger, more
   "capable"-looking choice), not left at the same value as `MinLumaCodingUnitSize`. Paired
   with a full transform-unit range (`4x4..32x32`) and
   `max_transform_hierarchy_depth_{inter,intra} == 3` (the legal maximum for that CU/TU
   range). And **`D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC_FLAG_USE_ASYMETRIC_MOTION_PARTITION`
   is required, not optional** — `CheckFeatureSupport`'s `GENERAL_SUPPORT_OK` stays `true`
   without it, but the real `ID3D12VideoDevice3::CreateVideoEncoder` call then fails, with
   the D3D12 debug layer reporting verbatim: *"Asymetric motion partition is required to be
   set."* A case where the advisory support query under-reports what object creation
   actually enforces — only found by making the real call and reading `ID3D12InfoQueue`.
   See [`hevc::default_codec_config_hevc`](../src/d3d12_video_encode/hevc.rs) for the
   resulting fixed configuration.

### AV1 — implemented, blocked from a full hardware round-trip by this driver

Extends the scope above with AV1 Main-profile encode, same CPU-upload/all-intra/fixed-CQP
staging, using the `windows` crate's AV1 D3D12 video-encode bindings (confirmed complete —
`D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_CODEC_DATA`/`_CODEC_CONFIGURATION`/
`_SEQUENCE_STRUCTURE`/etc., all under `windows::Win32::Media::MediaFoundation` like
H.264/HEVC's types — contrary to this ADR's original scope note, the bindings were never
the blocker).

- New files: [`av1`](../src/d3d12_video_encode/av1.rs) (`open`-time feature queries +
  `ID3D12VideoEncoder`/`ID3D12VideoEncoderHeap` creation for `D3D12_VIDEO_ENCODER_CODEC_AV1`,
  requesting the most conservative codec configuration —
  `D3D12_VIDEO_ENCODER_AV1_FEATURE_FLAG_NONE`), [`ops_av1`](../src/d3d12_video_encode/ops_av1.rs)
  (per-frame `EncodeFrame` recording, plus its own packet readback —
  [`ops_av1::D3d12VideoEncoder::read_packet_av1`](../src/d3d12_video_encode/ops_av1.rs) is
  **not** the shared `ops::read_packet` H.264/HEVC use, since AV1's `OBU_FRAME` needs a
  per-frame `leb128` size field the driver's compressed byte count determines — unlike
  H.264/HEVC's start-code-delimited NALs, which need no length prefix at all),
  [`bitstream_av1`](../src/d3d12_video_encode/bitstream_av1.rs) (hand-written OBU writer:
  temporal delimiter + sequence header, fixed per session; frame header, fixed per this
  backend's all-constant configuration — every field ground-truthed against the AV1
  Bitstream & Decoding Process Specification v1.0.0, see that file's module doc for the
  exact spec sections). `GopStructure` gained an `Av1(..)` variant alongside `H264`/`Hevc`.
- Design constraint unique to AV1 among the three codecs: because this backend sets AV1's
  loop-filter/quantization/CDEF/segmentation fields itself (the driver does not
  auto-decide them the way it does deblocking-mode selection for H.264/HEVC), every field
  [`bitstream_av1::write_frame_header`](../src/d3d12_video_encode/bitstream_av1.rs) writes
  into the bitstream must exactly match what
  [`ops_av1::encode_frame_av1`](../src/d3d12_video_encode/ops_av1.rs) passes to
  `EncodeFrame` via `D3D12_VIDEO_ENCODER_AV1_PICTURE_CONTROL_CODEC_DATA` — both sides are
  under this backend's control, so this is a correctness discipline (documented at each
  call site), not an unknown.

**Blocked at `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` on the tested hardware** (RTX
4090, Windows 11 24H2 build 26100, NVIDIA driver 32.0.15.9579, 2026-07-29): unlike HEVC's
driver quirk (a specific codec-*configuration* query reporting unsupported while the real
`GENERAL_SUPPORT_OK` check accepts a swept-in config), AV1's full support query reports
`ValidationFlags == D3D12_VIDEO_ENCODER_VALIDATION_FLAG_CODEC_NOT_SUPPORTED` — the
coarsest possible rejection — for every codec configuration tried, **despite**
`D3D12_FEATURE_VIDEO_ENCODER_CODEC` (the cheaper codec-presence probe already recorded by
`d3d12_av1_encode_codec_probe`) reporting `IsSupported == true`. Read together, this looks
like the OS/runtime recognizing `D3D12_VIDEO_ENCODER_CODEC_AV1` as a known codec identifier
without this NVIDIA consumer driver actually implementing an AV1 encode session through the
**D3D12 Video Encode API** — a different, narrower surface than NVENC's own SDK, which
*does* encode AV1 on this same GPU (see [`mediaway-encoder-nvenc`](../../mediaway-encoder-nvenc/adr/0001-nvenc-h264-first.md)'s
2026-07-29 hardware verification). `check_encoder_support`/`check_output_resolution`/
`check_resource_requirements` all pass first — only the final `GENERAL_SUPPORT_OK` query
fails, before `CreateVideoEncoder` is ever reached, so none of the bitstream/`pic_data`
code above has been exercised end to end on real hardware yet.
`d3d12_native_av1_encode_or_skip` ([`d3d12_video_encode_tests.rs`](../src/d3d12_video_encode_tests.rs))
records this honestly: it skips (does not fail) with the driver's exact `ValidationFlags`
reasoning available via the same `ID3D12InfoQueue` dump path H.264/HEVC's tests use, ready
to flip to a real pass the moment a driver implements this. Whoever picks this back up
should re-run it first — no code change should be needed if only the driver catches up.

### D3D12 Video Decode — deferred, not attempted

`ID3D12VideoDevice`/`ID3D12VideoDecoder`/`ID3D12VideoDecoderHeap` is a distinct API surface
from everything above (decode command lists, DXVA-shaped picture-parameter structs per
codec, DPB/reference-picture management even for IDR-only streams) — comparable in scope to
the HEVC encode work above, and this session's time budget went to HEVC encode (implemented
and verified) and the AV1 probe (real finding) instead. Not started; no partial/broken code
left behind. A future pass should treat it as its own ADR, mirroring how encode got one.

## Addendum (2026-08-06): H.264 GOP/P-frame support (single forward reference)

Extends the H.264 scope above from "every frame an independent IDR" to real `gop_size > 1`
support — single forward reference only (no B-frames, no multi-reference, no long-term
references), matching this workspace's Vulkan H.264 GOP precedent
([`adr/vulkan/0002-vulkan-gop-rate-control.md`](../vulkan/0002-vulkan-gop-rate-control.md)),
adapted to D3D12's very different reference-frame API shape. HEVC/AV1 are untouched by this
addendum (stay all-intra); `gop_size == 1` remains the byte-identical default for every
existing caller.

### Shape

- New pure-Rust [`gop.rs`](../src/d3d12_video_encode/gop.rs): `H264GopState`/`FrameDecision`,
  no D3D12 types — `frame_num`/`poc` (POC type 2: `poc = 2 * frame_num`, reset at every IDR),
  `is_idr` on GOP boundaries. `gop_size <= 1` always returns `is_idr: true, frame_num: 0,
  poc: 0`, reproducing the original all-IDR sequence exactly.
- New `setup::ReconPool`: **one** `ID3D12Resource` texture array with 2 array slices (not
  two separate resources — see the real-hardware finding below), ping-ponged each frame
  (single forward reference only ever needs one live reference at a time). Allocated only
  when `gop_size > 1`; `gop_size == 1` sessions never touch this at all.
- `D3d12VideoEncoder` gained `h264_gop_state: Option<H264GopState>`, `recon_pool:
  Option<ReconPool>`, `frame_decoding_order: u32`, `last_h264_reference: Option<(u32,
  u32)>` (the previous GOP-mode frame's POC/decoding-order — the only reference a P frame
  ever needs). All four stay unset/unused outside GOP mode.
- `check_encoder_support` gained a `max_reference_frames_in_dpb: u32` parameter — GOP mode
  probes `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` with `MaxReferenceFramesInDPB: 1`; on failure
  (driver can't honor it for this resolution/rate-control combination), `open` silently
  falls back to the original IDR-only GOP struct/support query, matching Vulkan's own
  capability-gated fallback (`supports_p_frames`) — no error surfaced to the caller.

### Real-hardware findings (RTX 4090, this session) — two real device-removal incidents

Getting a real `EncodeFrame` P-frame call to succeed (rather than crash the device) took
three iterations, each grounded in a real finding rather than a guess after the first two:

1. **`D3D12_RESOURCE_FLAG_VIDEO_ENCODE_REFERENCE_ONLY` alone is rejected by the debug
   layer** — `CreateCommittedResource` requires `DENY_SHADER_RESOURCE` alongside it
   (`0x88 = VIDEO_ENCODE_REFERENCE_ONLY | DENY_SHADER_RESOURCE`); `VIDEO_DECODE_REFERENCE_ONLY`
   is a **separate, mutually exclusive** flag for decode output textures — real hardware
   rejects combining it with the encode-reference-only flag ("Unsupported flags specified:
   0x80"), confirmed on this device. Caught cleanly by the debug layer, no device removal.
2. **Separate individual `ID3D12Resource`s for the two recon-pool slots caused a real
   `DXGI_ERROR_DRIVER_INTERNAL_ERROR` device removal** — `CopyTextureRegion` on the
   *unrelated* `input_texture` started reporting a barrier-layout mismatch on the next
   frame, a downstream symptom of the driver already being in an undefined state from the
   prior frame's `EncodeFrame` call. `D3D12_VIDEO_ENCODER_SUPPORT_FLAG_
   RECONSTRUCTED_FRAMES_REQUIRE_TEXTURE_ARRAYS` (confirmed real on this driver, per the
   fetched spec — see below) means separate 2D resources are not a valid choice here, even
   though the spec also says a texture array remains valid when the flag is *not* set — this
   backend now always uses one 2-slice texture array (`setup::create_nv12_texture_array`),
   never separate resources, for the recon pool. Fixing this alone did **not** clear the
   device removal (see next finding) — it was a real, necessary, but not sufficient fix.
3. **The actual root cause: `D3D12_VIDEO_ENCODER_PICTURE_CONTROL_FLAG_USED_AS_REFERENCE_PICTURE`
   was never set.** Providing a non-`NULL` `ReconstructedPicture` output resource without
   also setting this flag on `D3D12_VIDEO_ENCODER_PICTURE_CONTROL_DESC.Flags` is an
   undefined/illegal combination — the official spec (cached locally, see References) states
   the flag is what "indicates to output the reconstructed picture along with the bitstream,"
   i.e. it is not implied by merely passing a non-null resource. Setting it on every GOP-mode
   frame (IDR and P alike — every frame becomes the next frame's single reference) fixed the
   device removal immediately; reproduced clean twice in a row afterward (real `IPPIPPI`
   NAL cadence, `Packet::is_keyframe` correctly `false` for P packets).

Findings 2 and 3 were only pinned down by fetching the official D3D12 video-encode spec
document directly (`docs/conventions/external-standards.md` workflow — `bun
tools/scripts/fetch-standard.ts --ai-agent d3d12-video-encoding-h264-hevc`, cached under
`local/standards/d3d12-video-encoding-h264-hevc/`, registry id
`d3d12-video-encoding-h264-hevc`) rather than guessing further against real hardware after
the first fix attempt didn't clear the device removal — each of the two device-removal
incidents was contained to that one test's own `ID3D12Device` (other tests' independently
created devices were unaffected), never a full-system TDR, but real device-level corruption
is not something to keep guessing against blind.

### Row-based intra refresh (H.264 and HEVC)

Extends the GOP work above with `VideoEncoderConfig::intra_refresh_period` — continuous,
back-to-back row-based refresh waves instead of periodic full IDR frames: the session's
*only* IDR is its first frame, every frame after that is a P frame with a cyclically
advancing band of intra-coded rows, forever. Per the official spec (see the H.264 GOP
section's References below), this **requires an unbounded GOP** (`GOPLength = 0`,
`PPicturePeriod = 1`) — mutually exclusive with periodic-IDR GOP mode, so
`intra_refresh_period` takes priority over `gop_size` when both are set on the same
config. Reuses the GOP work's `ReconPool`/`USED_AS_REFERENCE_PICTURE` machinery
unchanged — a P frame's single reference works identically whether the previous frame
was a periodic-GOP P frame or an intra-refresh P frame.

- `gop.rs`/`gop_hevc.rs`: `H264GopState`/`HevcGopState` gained a `new_intra_refresh(period)`
  constructor (alongside the existing `new(gop_size)`) and an `intra_refresh_frame_index:
  Option<u32>` output on `FrameDecision` — `Some(i)`, `i` in `[0, period)`, on every frame
  of the session except its own startup IDR (`None` there, matching the spec's "disable
  the flag on the non-IR frame").
- `ops.rs`/`ops_hevc.rs`: `D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_FLAG_REQUEST_INTRA_REFRESH`
  and `IntraRefreshConfig` (`Mode: ROW_BASED`, `IntraRefreshDuration: period`) are set on
  every frame with `Some(_)` wave index; `PictureControlDesc.IntraRefreshFrameIndex` carries
  the wave index every frame (`0` outside intra-refresh mode, matching the pre-existing
  hardcoded value byte-for-byte).

**Real-hardware finding: `GENERAL_SUPPORT_OK` passing is not sufficient** —
`D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOLUTION_SUPPORT_LIMITS.MaxIntraRefreshFrameDuration`
(an output of the same `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` query, previously read and
discarded by this backend) is the real, resolution-dependent cap — `0` means row-based
intra refresh is unusable at *any* nonzero duration for that resolution, even though the
coarser mode-level check already passed. Requesting a duration above this cap is only
caught at the real `EncodeFrame` call (a clean, driver-reported "arguments are not
supported" rejection, not a device-removal crash — the debug layer named the exact
constraint verbatim: *"Intra refresh duration specified (N) exceeds the maximum supported
number of intra refresh frames duration 0"*). `setup::check_encoder_support`/
`hevc::check_encoder_support` now return this value alongside the suggested level, and
`open` validates `period <= MaxIntraRefreshFrameDuration && MaxIntraRefreshFrameDuration >
0` before committing to intra-refresh mode, falling back to periodic-GOP/IDR-only
otherwise — the same capability-gated-fallback contract as everything else in this ADR.
On this RTX 4090 at the test resolutions (176x144 H.264, 256x192 HEVC),
`MaxIntraRefreshFrameDuration` reports `0` — intra-refresh mode is not actually usable
here, so both hardware tests below exercise (and pass via) the documented fallback rather
than a live refresh-wave cadence; the capability-gated code path itself is confirmed
correct (no device removal, no invalid `EncodeFrame` call reaches the driver).

`d3d12_native_h264_intra_refresh_encode_or_skip` / `d3d12_native_hevc_intra_refresh_encode_or_skip`
(same test file) push 9 frames each at `intra_refresh_period: 4` and accept either a real
single-IDR-forever cadence or the all-IDR fallback as passing outcomes — both ran clean on
the RTX 4090 (2026-08-06), landing in the fallback branch per the finding above.

### Verified end-to-end on real hardware

`d3d12_native_h264_gop_encode_or_skip` ([`d3d12_video_encode_tests.rs`](../src/d3d12_video_encode_tests.rs)),
`gop_size: 3`, pushes 7 synthetic 176x144 NV12 frames and asserts a real `IPPIPPI` Annex-B
NAL-type cadence (type 5 IDR vs type 1 P) with `Packet::is_keyframe` agreeing with the NAL
type on every packet. **Passed twice in a row on the RTX 4090** (2026-08-06). Also asserts
the documented fallback (all-`IIIIIII`) as an equally valid outcome, in case a future
driver/hardware combination can't honor `MaxReferenceFramesInDPB >= 1` here.

### HEVC GOP/P-frame support — ported same session, worked first try

Same design (`gop_hevc.rs`'s `HevcGopState` — simpler than H.264's, HEVC has no
`frame_num`/`idr_pic_id`, only a `PictureOrderCountNumber` that increments by one per frame
and resets at IDR), same shared `setup::ReconPool` (codec-agnostic — the same texture-array
pool type serves whichever codec is active), same `D3D12_VIDEO_ENCODER_PICTURE_CONTROL_FLAG_
USED_AS_REFERENCE_PICTURE` fix applied from the start (not rediscovered). Unlike H.264, this
worked on the **first real-hardware attempt** — no device removal — because the root cause
was already known going in. `d3d12_native_hevc_gop_encode_or_skip` (same file), `gop_size:
3`, real HEVC Annex-B I/P cadence (IDR `nal_unit_type` 19/20 vs P `nal_unit_type` 1,
`TRAIL_R`). **Passed twice in a row on the RTX 4090** (2026-08-06).

## Consequences

- A second, independent hardware-encode code path exists in this crate alongside WMF. Both
  must be kept working; neither should assume the other's presence.
- H.264, HEVC, and AV1 now share most of this module's plumbing (upload/copy, buffer
  sizing, the fence/queue setup) through `GopStructure`'s codec dispatch, but each has its
  own `open`-time feature-query file (`setup`/`hevc`/`av1`), per-frame recording file
  (`ops`/`ops_hevc`/`ops_av1`), and bitstream writer (`bitstream`/`bitstream_hevc`/
  `bitstream_av1`) — the D3D12 API's per-codec C unions make a shared abstraction not worth
  it at 3 codecs. AV1's packet readback (`ops_av1::read_packet_av1`) is the one place that
  could **not** reuse the shared `ops::read_packet` H.264/HEVC both use — its `OBU_FRAME`
  needs a per-frame `leb128` size field, unlike NAL-based Annex-B.
- The three real-hardware findings above (resolution floor, `READBACK`-heap state
  restriction, `SuggestedLevel` requirement) are exactly the kind of driver-real, only-
  discoverable-by-running-on-real-hardware behavior this crate's wiki calls out — recorded
  here **and** in [`docs/ai/wiki/platform/windows-encode.md`](../../../docs/ai/wiki/platform/windows-encode.md)
  so a future HEVC/AV1/Zero-Copy pass doesn't rediscover them from scratch.
- CPU-upload-only, fixed-QP-only is an intentionally narrow slice — the same trade this
  crate's Linux VA-API sibling made. All-intra-only no longer holds for H.264/HEVC (real
  `gop_size > 1` single-forward-reference support, 2026-08-06 addendum); AV1 stays
  all-intra-only. Zero-Copy GPU input, B-frames/multi-reference, and rate-control tuning
  remain real follow-up work, not implied to be "coming for free."

## References

- FFmpeg `libavcodec/d3d12va_encode.c` / `d3d12va_encode_h264.c` (BSD-2-Clause,
  reference-only — ground-truthed the resource-state/heap-type/`SuggestedLevel` behavior
  above; no code copied, this crate's implementation is independent)
- `libavutil/hwcontext_d3d12va.c` (NV12 CPU-upload `CopyTextureRegion` staging pattern)
- Microsoft Learn: `D3D12_VIDEO_ENCODER_HEAP_DESC`, `ID3D12VideoDevice3::CreateVideoEncoder`
- ADR-0006 (D3D12 shared → D3D11 `GpuCopy` bridge — the *other* D3D12 path in this crate)
- `mediaway-encoder-linux` ADR-0001 (VA-API CPU-upload staging — the scope-cut precedent
  this ADR mirrors)
- `local/standards/d3d12-video-encoding-h264-hevc/d3d12_video_encoding_h264_hevc.md`
  (registry id `d3d12-video-encoding-h264-hevc`, `docs/standards/registry.toml`) — official
  D3D12 video-encode H.264/HEVC picture-control/reference-frame spec; source of the
  `USED_AS_REFERENCE_PICTURE` finding in the 2026-08-06 addendum
- `adr/vulkan/0002-vulkan-gop-rate-control.md` — this workspace's Vulkan H.264 GOP/P-frame
  precedent (single forward reference, no B-frames), the design this addendum's H.264 GOP
  support mirrors for the D3D12 backend
