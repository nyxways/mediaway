# ADR-0001: `VideoToolbox` `VTDecompressionSession` via `objc2`, H.264 CPU-output decode

- **Status**: Accepted
- **Date**: 2026-08-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (module `mediaway-decoder::apple`, ADR-0021-style `#[cfg]`-gated
  backend — no separate `mediaway-decoder-apple` crate)

## ⚠️ Zero real-hardware / zero compile verification in this session

**Read this before relying on this crate.** Same structural constraint as
`mediaway-encoder::apple` ADR-0001: this repo's dev environment (Windows host) cannot
cross-compile Apple code at all — Apple SDK headers/frameworks cannot legally be built outside
macOS/Xcode. Every API name, signature, and constant cited below is a **direct read** of the
locally cloned [`objc2`](https://github.com/madsmtm/objc2) checkout
(`local/vendor-ref/objc2/generated/`, `generated` submodule initialized), not a paraphrase from
memory or web search. Where local source does not settle a question, this ADR says so explicitly
(§ Open questions) instead of guessing.

**Corpus note (parent-task discrepancy):** the task briefing that produced this ADR referenced an
already-shipped `mediaway-decoder::android` sibling backend and an `android-decode.md` wiki page.
Neither exists in this repository state — `crates/mediaway-decoder/src/` has no `android/` or
`apple/` submodule at all before this change (confirmed via direct directory listing), and
`mediaway-decoder` has no Android/Apple `[target.'cfg(...)'.dependencies]` entries in its
`Cargo.toml`. This ADR is grounded instead in the backends that **do** exist: `mediaway-decoder::linux::vaapi`
(closest decode structural precedent — CPU-output, lazy pipeline creation) and
`mediaway-decoder::vulkan` (closest **general-GOP** decode precedent — H.264 hardware-verified,
non-IDR-only), plus `mediaway-encoder::apple` (closest `objc2`-usage precedent, same framework
family, decode is its natural counterpart).

## Context

Apple platforms (macOS + iOS) are the last "Other" backend for this crate's decode side
(`docs/roadmap.md` Stage 4), the decode counterpart to the just-implemented
`mediaway-encoder::apple` (VTCompressionSession) backend. VideoToolbox's decode API,
`VTDecompressionSession`, is the only supported HW-accelerated H.264 decode path on Apple
platforms.

### Why decode needs no app-side H.264 bitstream parser (unlike this crate's VA-API backend)

`mediaway-decoder::linux::vaapi`'s ADR-0001 explains that VA-API requires the **application** to
parse SPS/PPS/slice headers and build `VAPictureParameterBufferH264`/`VASliceParameterBufferH264`
by hand, because the VA-API driver only does entropy decoding + reconstruction, not bitstream
parsing. VideoToolbox is architected the opposite way, like Android's `AMediaCodec` (the
`objc2` binding source confirms this structurally, not just by analogy): `VTDecompressionSession`
takes a `CMSampleBuffer` containing framed NAL data plus a `CMVideoFormatDescription` (built once
from SPS/PPS), and the **decoder itself** — hardware or software, VideoToolbox's choice — does
all NAL/slice/entropy parsing and reference-picture management internally. This crate never
touches H.264 slice syntax for this backend, only NAL-level framing (Annex-B ↔ AVCC) and SPS/PPS
extraction for the one-time format description.

### VideoToolbox manages its own DPB — general GOP (P/B frames), not IDR-only

Grounded via two direct reads of `local/vendor-ref/objc2/generated/CoreMedia/CMSampleBuffer.rs`
and `.../VideoToolbox/VTDecompressionSession.rs`:

- `CMSampleBufferCreate`'s own doc comment gives a **worked example of an out-of-order H.264 GOP**
  (`P2, B0, B1, I5, B3, B4` in decode order) with distinct `presentationTimeStamp` /
  `decodeTimeStamp` per `CMSampleTimingInfo` entry — VideoToolbox decode is explicitly designed
  for exactly this input shape, not just IDR streams.
- `VTDecompressionOutputCallback`'s doc comment states plainly: **"This function will not
  necessarily be called in display order."**

Both confirm `VTDecompressionSession` is a black-box HW/SW-hybrid decoder — like Android's
`AMediaCodec` (the task's requested comparison point) and unlike this crate's VA-API backend — the
OS/driver owns reference-picture-list construction, MMCO, and POC bookkeeping entirely
internally. This backend's scope is therefore **general GOP (P/B frames)**, not IDR-only: the app
supplies NALs with correct per-packet `pts`/`dts` and gets frames back; it never builds a
reference-picture list itself.

## Decision

> Depend on **`objc2-video-toolbox`, `objc2-core-media`, `objc2-core-video`,
> `objc2-core-foundation`, version `"0.3"`** — the exact same crate family and major/minor pin
> `mediaway-encoder::apple`'s ADR-0001 already reviewed (need / license `Zlib OR Apache-2.0 OR
> MIT` / transitive / maintenance / API stability / unsafe surface — not re-derived here, see that
> ADR) — as a **`[target.'cfg(any(target_os = "macos", target_os = "ios"))'.dependencies]`** entry
> in `mediaway-decoder`'s `Cargo.toml`, mirroring the encoder crate's target-cfg gate exactly (the
> `any(...)` form is required because `target_os = "macos"`/`"ios"` are distinct cfg values).

Confirmed-real feature names for this backend's decode-specific surface (direct reads of
`objc2-video-toolbox/Cargo.toml`'s feature table and the generated decode files):

- `objc2-video-toolbox`: `"VTDecompressionSession"`, `"VTDecompressionProperties"`, `"VTErrors"`,
  `"VTSession"`, `"objc2-core-media"`, `"objc2-core-video"` (the crate's own feature names that
  transitively pull the same fixed `objc2-core-media`/`objc2-core-video` sub-feature sets the
  encoder crate already enables).
- `objc2-core-media`: add `"CMBlockBuffer"` explicitly, same reasoning as the encoder ADR —
  `objc2-video-toolbox`'s own feature list does not include it, but this backend needs
  `CMBlockBuffer::with_memory_block` to wrap the AVCC-framed packet payload for `CMSampleBuffer`.
- `objc2-core-video`: add `"CVReturn"` explicitly (`CVPixelBufferLockBaseAddress`/
  `UnlockBaseAddress` return it; not covered by `objc2-video-toolbox`'s transitive feature list,
  same gap the encoder ADR found and fixed for its own use of `CVReturn`). `"CVPixelBufferPool"`
  is **not** requested this stage — decode never constructs a pool itself; VideoToolbox manages
  its own output pool internally (`kVTDecompressionPropertyKey_PixelBufferPool` exists only to
  *read* the session's pool, not to build one, per `VTDecompressionProperties.rs`).
- `objc2-core-foundation`: `"CFString"`, `"CFDictionary"`, `"CFNumber"` (pixel-format-type
  property dictionary). `"CFArray"` is not needed this stage (no multi-image / MV-HEVC path).

New module `mediaway-decoder::apple` (`src/apple/`), following this crate's own `src/linux/`
shape (thin wrapper delegating to a platform submodule) and `mediaway-encoder::apple`'s module
naming: `AppleVideoDecoder` (public, `#[cfg(any(target_os = "macos", target_os = "ios"))] inner:
Option<videotoolbox::VideoToolboxVideoDecoder>` / non-Apple stub) implementing [`VideoDecoder`],
wrapping an inner `videotoolbox` submodule split the same way `linux::vaapi` and
`encoder::apple::videotoolbox` are split — session logic vs. pure, host-testable helpers:

```
src/apple/mod.rs                     — AppleVideoDecoder (Option-wrapped closed-after-move sentinel)
src/apple/videotoolbox/mod.rs        — pub(crate) re-export, mirrors linux/vaapi/mod.rs
src/apple/videotoolbox/video.rs      — VideoToolboxVideoDecoder: session, callback, VideoDecoder impl
src/apple/videotoolbox/codec.rs      — pure helpers: CMTime<->Rational math, NV12 plane-copy byte
                                        math, AVCC parameter-set pointer/size prep — no VideoToolbox
                                        calls, sibling `codec_tests.rs` runs on any host
```

### Session lifecycle

- **Lazy format description + session creation**, mirroring `linux::vaapi`'s own "pipeline
  creation is lazy" decision (ADR-0001 § Decision) almost verbatim, for the identical reason
  (`VideoDecoderConfig::extra_data` "may be empty until first keyframe"):
  - If `extra_data` (avcC, this crate's established convention — confirmed via
    `src/windows/wmf/video.rs`'s `to_avcc` use, cited by `VideoDecoderConfig`'s own doc comment)
    is non-empty at `open()`, build the format description and `VTDecompressionSession`
    immediately.
  - Otherwise, defer both until the first `push_packet` call whose payload contains SPS+PPS
    (Annex-B in-band parameter sets, detected the same way `iso_bmff::bitstream::avc::to_avcc`
    already detects them — see § Byte framing).
- `CMVideoFormatDescription::from_h264_parameter_sets(allocator: None, parameter_set_count: 2,
  [sps_ptr, pps_ptr], [sps_len, pps_len], nal_unit_header_length, &mut out)` — confirmed real
  signature in `generated/CoreMedia/CMFormatDescription.rs`. Its own doc comment states inputs
  "can come from raw NAL units and must have any emulation prevention bytes needed" — i.e. **raw
  NAL payload, no start code, no length prefix** — exactly what
  `iso_bmff::bitstream::avc::parse_avc_decoder_config`'s `AvcDecoderConfig::{sps, pps}` already
  returns (doc comment: "without start code or length prefix"). Stage 1 passes exactly one SPS +
  one PPS (`sps[0]`/`pps[0]`); a stream with multiple SPS/PPS NALs returns
  `DecodeError::Unsupported` at open rather than silently picking one.
- `VTDecompressionSession::new(allocator: None, video_format_description, video_decoder_specification:
  None, destination_image_buffer_attributes: Some(&nv12_attrs), output_callback: Some(&record),
  &mut session_out)` — confirmed real signature in `generated/VideoToolbox/VTDecompressionSession.rs`.
  `video_decoder_specification: None` lets VideoToolbox pick HW vs. SW, matching the encoder
  backend's identical choice.
- `destination_image_buffer_attributes`: a `CFDictionary` with
  `kCVPixelBufferPixelFormatTypeKey -> CFNumber(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange)`
  — forces NV12/`VideoRange` output. **`VideoRange`, not `FullRange`** (the encoder backend's own
  choice for its *synthetic, self-produced* frames) — because `mediaway_common::ColorRange`'s own
  doc comment names `Video` "the common camera/H.264 convention" and is that type's `#[default]`;
  decode consumes arbitrary third-party H.264 streams, for which `VideoRange` is the more honest
  default absent VUI-based range detection (not implemented this stage — see § Deferred).
- Per-packet: `session.decode_frame(&sample_buffer, decode_flags, source_frame_ref_con,
  info_flags_out: None)` — confirmed real signature. `decode_flags` sets **both**
  `kVTDecodeFrame_EnableAsynchronousDecompression` and `kVTDecodeFrame_EnableTemporalProcessing`
  (real bit constants, confirmed in `generated/VideoToolbox/VTErrors.rs`, `1<<0` / `1<<3`) — see
  § Output ordering for why.
- Flush: `session.wait_for_asynchronous_frames()` — confirmed real: "Waits for any and all
  outstanding asynchronous and delayed frames to complete... automatically calls
  `VTDecompressionSessionFinishDelayedFrames`" — a single call that both finishes decode-order
  delayed frames and drains temporal-processing reorder buffering, then `poll_frame` drains the
  shared queue.
- Teardown (`Drop`): `session.wait_for_asynchronous_frames()` unconditionally (defensive, same
  discipline as the encoder ADR's `complete_frames` pre-`invalidate` call — see § Open questions),
  then `session.invalidate()`, then drop the `CFRetained<VTDecompressionSession>`.

## Byte framing — AVCC length-prefixed, reusing `iso_bmff::bitstream::avc` (both directions)

`CMSampleBuffer`'s payload (wrapped in a `CMBlockBuffer`) must be **AVCC length-prefixed** NAL
data matching the format description's declared `nal_unit_header_length` — VideoToolbox parses
sample data using that length-prefix size, not Annex-B start codes. This is exactly the class of
landmine the VA-API decode ADR and Android encode ADR both called out for their own backends
(byte-framing mismatches silently corrupt output rather than erroring):

- Stage 1 requires **`nal_unit_header_length == 4`** — the overwhelmingly common real-world
  convention (all MP4/H.264 tooling in this workspace already uses it, confirmed:
  `iso_bmff::bitstream::avc::to_avcc`'s own doc says "4-byte length-prefixed"). If
  `extra_data`/in-band avcC parses with `AvcDecoderConfig::nal_length_size != 4`,
  `open()`/`push_packet` returns `DecodeError::Unsupported` — never silently reframed with the
  wrong length size.
- `push_packet(packet)` calls `iso_bmff::bitstream::avc::to_avcc(&packet.payload)` — **already a
  workspace dependency this crate uses** (see `Cargo.toml`, `iso-bmff = { workspace = true }`).
  Annex-B input is converted to 4-byte-length-prefixed AVCC (and yields a fresh `avcc: Some(..)`
  on the first SPS+PPS-bearing packet, feeding the lazy session-creation path above); input that
  is already AVCC-framed is passed through unchanged (`to_avcc`'s documented behavior for non-
  Annex-B input) — this crate does not independently re-verify the pass-through case's
  length-prefix size, inheriting the same "4-byte AVCC" convention this workspace's demuxers
  already emit (same trust boundary `src/windows/wmf/shared.rs` already relies on for this crate).
- `CMBlockBuffer::with_memory_block(structure_allocator: None, memory_block, block_length,
  block_allocator: None, custom_block_source: Some(&release_source), offset_to_data: 0,
  data_length, flags, &mut out)` — confirmed real signature in
  `generated/CoreMedia/CMBlockBuffer.rs`. The AVCC bytes are copied once into a heap-owned
  `Box<[u8]>` (the one real, named `memcpy` on this path — `to_avcc`'s own output is already an
  owned `Bytes`, so this is a second, necessary copy only when `Bytes`'s internal layout is not
  directly `CMBlockBuffer`-compatible; implementation should measure whether `to_avcc`'s `Bytes`
  can be handed to `with_memory_block` via its raw pointer directly instead, avoiding the second
  copy — flagged for the implementation pass, not resolved here), with `custom_block_source`'s
  free callback doing `Box::from_raw` reclaim — same ownership-handoff shape as the encoder
  backend's `upload_cpu_nv12`'s `CVPixelBufferReleasePlanarBytesCallback` pattern.

## Timestamps — `CMSampleTimingInfo` from `Packet::{pts, dts, duration}`

`CMSampleBufferCreate`'s own doc-comment worked example (cited above) sets **distinct**
`presentationTimeStamp`/`decodeTimeStamp` per sample for out-of-order H.264 — this crate's
`Packet` already carries both (`pts: i64`, `dts: i64`, `duration: u64`), so no new type is needed.
Conversion uses `VideoDecoderConfig::time_base: Rational { num: u64, den: u32 }` generally (not
assuming `num == 1`): `CMTime { value: packet.pts * time_base.num as i64, timescale:
time_base.den as i32, .. }` — algebraically exact since `CMTime`'s contract is `value/timescale =
seconds` and `time_base` is ticks-to-seconds (`num/den`). Built once per `push_packet` call as a
single-entry `CMSampleTimingInfo` (`num_sample_timing_entries: 1`, matching
`CMSampleBufferCreate`'s "one entry applies to all samples in this call" contract — this backend
always submits exactly one frame per `CMSampleBuffer`, never a batch).

## Output ordering — VideoToolbox reorders to presentation order; **no reorder buffer in this crate**

Per § Context, VideoToolbox's callback is not display-order by default. Setting
`kVTDecodeFrame_EnableTemporalProcessing` on every `decode_frame` call (confirmed real bit,
doc: "indicates whether the decoder may delay calls to the output callback so as to enable
processing in temporal (display) order") delegates **all** P/B-frame reorder buffering to
VideoToolbox itself — the same "let the black-box HW decoder own it" posture this ADR already
took for DPB/reference management. This crate's `poll_frame` therefore returns frames already in
presentation order; **no PTS-sorting reorder buffer is implemented in this crate.** The
trade-off, paid deliberately: enabling this flag makes the output callback genuinely asynchronous
(may fire after `decode_frame` returns, on a VideoToolbox-internal thread) — handled by the same
`Arc<Mutex<VecDeque<VideoFrame>>>` bridge shape the encoder ADR already uses for its own
(unconditionally async) callback, not a new pattern.

## Callback / output-collection design

`VTDecompressionOutputCallback` (confirmed type: `unsafe extern "C-unwind" fn(*mut c_void, *mut
c_void, OSStatus, VTDecodeInfoFlags, *mut CVImageBuffer, CMTime, CMTime)`) — shape:

- At session creation: build `shared: Arc<Mutex<VecDeque<VideoFrame>>>`; keep one clone in
  `VideoToolboxVideoDecoder { shared, .. }` (used by `poll_frame`); pass
  `Arc::into_raw(shared.clone())` as `decompressionOutputRefCon` — one deliberate "extra" strong
  count reclaimed only in `Drop`, identical to the encoder ADR's callback-lifetime design.
- Inside the callback: reconstruct a **borrow**
  (`let shared = unsafe { &*(ref_con.cast::<Mutex<VecDeque<VideoFrame>>>()) };`), never
  `Arc::from_raw` on every invocation.
- **CPU NV12 readback happens inside the callback**, not deferred: the callback's own doc comment
  warns the `imageBuffer` "may still be referenced by the video decompressor" unless
  `kVTDecodeInfo_ImageBufferModifiable` is set — this backend never modifies the buffer (only
  reads it under `CVPixelBufferLockBaseAddress(kCVPixelBufferLock_ReadOnly)`, confirmed real flag
  in `generated/CoreVideo/CVPixelBuffer.rs`), but copying planes into an owned `Bytes` **before**
  the callback returns is the only point this backend can be certain the buffer's contents are
  still valid for this frame, mirroring VA-API's "read back inside `Picture::sync` +
  `vaGetImage`" discipline.
- **`*mut CVImageBuffer` → `&CVPixelBuffer` downcast**: confirmed real, safe, **checked** cast —
  `CVPixelBuffer: ConcreteType` (via `CVPixelBufferGetTypeID`, confirmed in
  `generated/CoreVideo/CVPixelBuffer.rs`) and `objc2_core_foundation::CFType::downcast_ref::<T:
  ConcreteType>(&self) -> Option<&T>` (confirmed in
  `local/vendor-ref/objc2/framework-crates/objc2-core-foundation/src/base.rs`) together give a
  checked downcast that returns `None` (never an unchecked pointer cast) if the callback's image
  buffer is not concretely a `CVPixelBuffer`. `None` is treated as a dropped/skipped frame with a
  documented rustdoc note, not a panic — H.264 decode to a `CVPixelBuffer` destination is the
  overwhelmingly expected case per Apple's own API design, but this crate does not assume it
  unchecked.
- Plane copy: `pixel_buffer.width_of_plane`/`height_of_plane`/`bytes_per_row_of_plane`/
  `base_address_of_plane` for planes 0 (Y) and 1 (UV) (confirmed real accessors in
  `CVPixelBuffer.rs`), row-by-row copy into one owned `Bytes` (handles stride padding — same
  documented-copy discipline as `linux::vaapi`'s NV12 readback). Defensive check:
  `pixel_buffer.pixel_format_type() == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange`; a
  mismatch (VideoToolbox declined the requested destination format) returns
  `DecodeError::Backend` rather than misinterpreting bytes.
- In `Drop`: `wait_for_asynchronous_frames()`, `invalidate()`, then
  `drop(unsafe { Arc::from_raw(ref_con_ptr) })` — releases the extra strong count.

**Open question, not settled by local source (flagged, not assumed):** same structural gap the
encoder ADR flagged for `VTCompressionSessionInvalidate` — whether
`VTDecompressionSessionInvalidate` guarantees no further callback invocation once it returns is
not stated in the doc comment read (`"ensures a deterministic, orderly teardown"` only). Mitigated
the same way: `Drop` unconditionally calls `wait_for_asynchronous_frames()` (a stronger,
confirmed-blocking drain) **before** `invalidate()`, narrowing the risk window to "does that call
itself fully synchronize the callback thread" — its own doc phrasing implies yes, but this is
still worth confirming against Apple's official documentation once real hardware/docs access
exists.

## ZCA / typestate shape

| `linux::vaapi::VaapiH264Decoder` (CPU-out, closest structural precedent) | `apple::videotoolbox::VideoToolboxVideoDecoder` | Difference |
|---|---|---|
| `Display`/`Config`/`Context`/`Surface` typestate; no async, `&mut self` synchronous decode-then-readback | Single `CFRetained<VTDecompressionSession>` + `Arc<Mutex<VecDeque<VideoFrame>>>` callback bridge | Output collection is push-based (VT pushes into our queue via callback) vs. VA-API's pull-based `Picture::sync` + `vaGetImage` in the same call |
| IDR-only scope ⇒ no DPB at all | General GOP ⇒ DPB fully owned by VideoToolbox internally, invisible to this crate | This crate never builds a reference-picture list either way — VA-API's is scoped away, VideoToolbox's is opaque |
| `#![forbid(unsafe_code)]` (all FFI unsafety lives in `cros-libva`) | `#![allow(unsafe_code)]` + `// SAFETY:` — every `objc2-*` call is `unsafe fn`, same posture `mediaway-encoder::apple` already established for this crate family | Structural, not a choice — VideoToolbox's `objc2-*` bindings are plain-C `unsafe fn` wrappers, no safe layer |

`AppleVideoDecoder` wraps a concrete `videotoolbox::VideoToolboxVideoDecoder` behind `Option`
(closed-after-move sentinel), identical to every other platform wrapper in this crate
(`LinuxVideoDecoder`, encoder's `AppleVideoEncoder`). No `Box<dyn _>` introduced. The callback
bridge's one `Arc` is the same shape the encoder backend already uses for its own async output —
not a new pattern for this crate family.

## `GpuBufferHandle::Metal` — already exists, Zero-Copy deferred

`mediaway-common::GpuBufferHandle::Metal { buffer: NativeHandle }` (confirmed, predates this ADR,
`crates/mediaway-common/src/gpu.rs`) is the natural Zero-Copy output shape for a
`CVPixelBuffer`/`IOSurface` token — same situation the encoder ADR found. `VideoOutputPreference::ZeroCopyGpu`
returns `DecodeError::Unsupported` this stage, matching `linux::vaapi`'s identical deferral for
DMA-BUF. No `mediaway-common` change is needed to declare the future variant; only wiring is
deferred.

## Error handling

Reuses `crate::DecodeError`'s existing 5-variant shape unchanged (`Unsupported`, `NoBackend`,
`InvalidInput`, `Backend`, `Closed`) — no gap found that needs a new variant. `OSStatus != noErr`
from any `VTDecompressionSession*`/`CMVideoFormatDescriptionCreate*` call maps to
`DecodeError::Backend`; unsupported byte-framing/multi-SPS/multi-PPS/non-4-byte-length-size
inputs map to `DecodeError::Unsupported`; calls after `Drop`/close map to `DecodeError::Closed`
via the `Option`-wrapped sentinel, same as every other backend in this crate.

## Scope (this stage)

**In:**

- H.264 decode only, CPU NV12 (`VideoRange`) readback only
  (`VideoOutputPreference::CpuFramesOk`).
- General GOP (P/B frames) — VideoToolbox-managed DPB + display-order reordering via
  `kVTDecodeFrame_EnableTemporalProcessing`.
- SPS/PPS format-description construction from `extra_data` (avcC) or in-band Annex-B, both via
  `iso_bmff::bitstream::avc` reuse. Exactly one SPS + one PPS; 4-byte NAL length size only.
- `mediaway-decoder::apple` module only — **not** wired into any `auto`/capability dispatch,
  matching this crate's other unwired platform backends.

**Out (deferred):**

- Zero-Copy `CVPixelBuffer`/`IOSurface` output (`GpuBufferHandle::Metal`) — returns
  `DecodeError::Unsupported`.
- HEVC/AV1/VP9/ProRes decode (VideoToolbox supports them; this backend does not yet).
- VUI-based `ColorRange`/`Full` detection — `VideoRange` is hardcoded this stage.
- Multiple SPS/PPS, non-4-byte AVCC length sizes, interlaced/field-mode content, MV-HEVC/
  multi-image decode.
- `mediaway-decoder`'s `auto`/`capability` wiring.
- Real hardware/simulator test execution beyond CI compile+clippy, if/when Apple CI jobs are
  added for this crate (no such jobs exist yet for `mediaway-decoder`; the encoder ADR's §
  CI verification plan proposal was not confirmed as implemented and is not re-litigated here).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Hand-written `bindgen`/raw FFI against VideoToolbox/CoreMedia/CoreVideo headers | Same reasoning as every other backend in this workspace and the encoder ADR's identical rejection: reinvents `CFRetained` ref-counting, `OSStatus`/`CVReturn` typing, and struct layouts `objc2-*` already provides, `header-translator`-verified against real Apple SDKs. |
| Scope Stage 1 to IDR-only (VA-API-style) | Explicitly rejected — VideoToolbox's black-box DPB makes general-GOP support essentially free (the app does no reference-list work either way), unlike VA-API where IDR-only genuinely eliminates real implementation work. Scoping down here would forgo a real capability for no implementation-cost reason. |
| Leave `decode_flags` clear (synchronous, decode-order output) + implement a PTS-sorting reorder buffer in this crate | Rejected: duplicates work VideoToolbox already does correctly via `kVTDecodeFrame_EnableTemporalProcessing`, and a correct reorder buffer needs a bound derived from `max_num_reorder_frames` (SPS VUI, unparsed this stage) — strictly more code and more risk than delegating to the OS. |
| `destinationImageBufferAttributes: None` (let VideoToolbox pick its native output format) | Rejected — `VideoDecoderConfig::pixel_format: PixelFormat::Nv12` is a caller contract; not requesting a specific `CVPixelBufferPixelFormatTypeKey` risks a non-NV12 output format this backend cannot honestly claim to have produced. Same reasoning VA-API's ADR gave for querying `VAImageFormat` explicitly rather than trusting a default. |
| `CVPixelBufferCreateWithPlanarBytes`-style pool reuse for output buffers | N/A for decode — VideoToolbox owns and manages its own output pool (`kVTDecompressionPropertyKey_PixelBufferPool` is read-only informational); this crate never constructs pixel buffers on the decode path, only reads them. |

## Consequences

### Positive

- Grounded entirely in real local source (`local/vendor-ref/objc2/generated/`), matching every
  major decision (byte framing, output ordering, downcast safety) to a specific confirmed
  function/doc comment rather than assumption.
- General-GOP decode achievable in Stage 1 at essentially the same implementation cost as
  IDR-only, because VideoToolbox's DPB is opaque to this crate either way — a materially stronger
  Stage-1 capability than this crate's VA-API decode backend for the same amount of new code.
- Reuses `iso_bmff::bitstream::avc` in **both** directions (Annex-B→AVCC for packets,
  `parse_avc_decoder_config` for format-description parameter sets) — no new bitstream-framing
  code, and the exact same trust boundary (4-byte AVCC) this crate's Windows backend already
  relies on.
- `GpuBufferHandle::Metal` already existing means the deferred Zero-Copy stage has no blocking
  type-design work left, only wiring.
- Checked (`downcast_ref`), not unchecked, `CVImageBuffer → CVPixelBuffer` cast — no naive
  pointer-cast landmine shipped.

### Negative / Trade-offs

- **Zero compile verification as authored** — no legal cross-compile path outside Apple tooling;
  every signature above is a research-pass read, not a real local build. Matches the encoder
  ADR's identical caveat.
- **Real, non-trivial `unsafe` surface owned directly by this crate module** — `#![allow(unsafe_code)]`
  + `// SAFETY:` discipline required, unlike `linux::vaapi`'s `#![forbid(unsafe_code)]`.
- `VTDecompressionSessionInvalidate` callback-cutoff ordering is an **unconfirmed** assumption
  (mitigated defensively, not proven) — same open risk class the encoder ADR already accepted for
  its own session type.
- Enabling `kVTDecodeFrame_EnableTemporalProcessing` unconditionally means this backend cannot
  offer a "lowest possible latency, no reorder delay" mode this stage — a real behavior trade-off
  a future stage may want to expose as a config knob.
- `CMBlockBuffer::with_memory_block`'s possible second `memcpy` (§ Byte framing) is flagged, not
  resolved — the implementation pass should confirm whether `to_avcc`'s `Bytes` output can be
  handed over directly.
- `objc2-*` crates are pre-1.0 (`0.3.x`) — same semver-risk class as this workspace's other
  pre-1.0 platform bindings.

## References

- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
- [`docs/spec/crate-packaging.md`](../../../../docs/spec/crate-packaging.md) — `apple` platform
  suffix (single module for macOS+iOS)
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) — honesty
  requirement this ADR follows, including for its own unresolved-detail admissions
- `mediaway-encoder` [ADR-apple/0001](../../../mediaway-encoder/adr/apple/0001-videotoolbox-h264-cpu-upload.md) —
  `objc2-*` dependency review (reused, not re-derived here), callback-bridge precedent, the
  `CreateWithBytes` vs. `CreateWithPlanarBytes` landmine this crate's equivalent NV12-plane-layout
  reasoning mirrors
- `mediaway-decoder` [ADR-linux/0001](../linux/0001-vaapi-h264-cpu-out.md) — CPU-output decode
  structural precedent (lazy pipeline creation, `Cargo.toml` target-cfg shape, stride-aware NV12
  readback discipline), and the explicit contrast baseline for "why this backend needs no app-side
  H.264 parser"
- `mediaway-decoder` [ADR-vulkan/0001](../vulkan/0001-vulkan-video-decode.md) — this crate's
  general-GOP (non-IDR-only) decode precedent
- Local grounding source (read directly, not web-fetched):
  `local/vendor-ref/objc2/generated/VideoToolbox/{VTDecompressionSession,VTDecompressionProperties,VTErrors,VTSession}.rs`,
  `local/vendor-ref/objc2/generated/CoreVideo/{CVPixelBuffer,CVImageBuffer,mod}.rs`,
  `local/vendor-ref/objc2/generated/CoreMedia/{CMSampleBuffer,CMBlockBuffer,CMFormatDescription,CMTime}.rs`,
  `local/vendor-ref/objc2/framework-crates/objc2-core-foundation/src/{base.rs,retained.rs,type_traits.rs}`,
  `local/vendor-ref/objc2/framework-crates/objc2-video-toolbox/Cargo.toml`
- [`objc2` on GitHub](https://github.com/madsmtm/objc2) (`Zlib OR Apache-2.0 OR MIT`)
- `crates/iso-bmff/src/bitstream/avc.rs` — `to_avcc`/`parse_avc_decoder_config`/
  `avcc_payload_to_annex_b` reused unchanged by this backend
- `docs/roadmap.md` § platform order (Windows → Web → Linux → other) · crate
  `docs/roadmap.md` § Stage 4 — Other
- README.md § Codec support — Apple decode H.264/AVC cell target: `👻` → `🆗` once implemented
  (implemented/compiles, not hardware-verified — the same mark `mediaway-encoder::apple` already
  carries for encode)

ADRs are written in **English**.
