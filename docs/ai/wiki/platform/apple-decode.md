# Apple decode (`mediaway-decoder::apple`)

ADRs: `adr/apple/README.md` (full index) — 0001 H.264, 0002 HEVC/VP9/AV1 (**wired into
`mediaway::platform`'s `AutoDecoder`/`decoder_support`**), 0003 Zero-Copy output, 0006 ProRes.
All **Accepted, zero compile verification** (no Apple SDK in this dev environment, same posture as
`mediaway-encoder::apple`). Implemented at `src/apple/` — `cargo check`/`clippy` pass on this
Windows host (`videotoolbox/{video,format_desc}.rs`'s real `objc2-*` calls are `cfg`-gated to
Apple targets and cannot be type-checked here; `videotoolbox/codec.rs`'s pure helpers have no
`objc2-*` dependency and are covered by real, passing unit tests).

## Zero-Copy output — real, one handle outstanding at a time

`VideoOutputPreference::ZeroCopyGpu` skips `lock_base_address`/plane copy — the callback takes a
**new**, independent `CFRetained::retain` on the decoded `CVPixelBuffer`, hands its bits out as
`GpuBufferHandle::Metal`. Unlike VA-API's DMA-BUF Zero-Copy (`adr/linux/0006`),
`CVPixelBufferPool` **grows on demand** rather than reusing a fixed slot — no `outstanding`/
DPB-style tracking needed; the real risk is unbounded growth, not corruption. Contract:
`VideoToolboxVideoDecoder` holds the last-`poll_frame`-returned handle's retain and drops
(releases) it at the start of the *next* `push_packet`/`poll_frame`/`flush` — same "valid until
the next call" convention every other Zero-Copy decoder here documents, enforced by plain Rust
`Drop` instead of manual fd/slot bookkeeping. `PendingFrame { frame, zero_copy_retain }` bundles
the optional retain into `pending`'s element type — no second, separately-locked collection.

## HEVC — mirrors H.264 exactly

`CMVideoFormatDescriptionCreateFromHEVCParameterSets` (VPS+SPS+PPS, dedicated entry point same
shape as H.264's own) + a new `iso_bmff::bitstream::hevc` module (Annex-B ↔ `hvcC`, structurally
identical to `iso_bmff::bitstream::avc` generalized from 2 parameter-set types to 3 — a genuine
workspace-level addition, not backend-private code). Same lazy session creation, same NAL framing
per packet (`to_hvcc`), same general-GOP black-box-DPB posture as H.264.

## VP9/AV1 — no bitstream parsing, container-supplied config record required

`VideoToolbox` has **no per-frame parameter-set entry point** for either codec (confirmed: grepped
every generated `objc2-video-toolbox`/`objc2-core-media` file, only the plain
`kCMVideoCodecType_{VP9,AV1}` type constants exist). The only construction path is generic
`CMVideoFormatDescriptionCreate(codecType, width, height, extensions)` with the config record
(`vpcC`/`av1C`) wrapped as a `SampleDescriptionExtensionAtoms` extension atom
(`format_desc::create_raw`) — this backend does **not** parse the VP9/AV1 bitstream itself to
synthesize one (unlike `linux::vaapi`'s from-scratch VP9/AV1 parsers, which exist because VA-API's
session API genuinely needs full picture parameters this crate must derive). `extra_data` **must**
already hold a valid `vpcC`/`av1C` at `open()` — `DecodeError::Unsupported` if empty, no in-band
lazy discovery like H.264/HEVC get.

**Corpus note:** ADR-0001 is grounded in `linux::vaapi`/`vulkan`, not an Android decode ADR — none
existed in this repo when it was written.

**ProRes** is a *third* construction shape (neither parameter-set entry point nor extension
atom) — just geometry, `format_desc::create_raw_no_extension`, `extensions: None`, session built
eagerly at `open()` unconditionally, no config record at all. See [apple-prores](apple-prores.md).

## Scope (ADR-0001, H.264 — HEVC/VP9/AV1/ProRes scope is above)

- H.264 CPU NV12 (`VideoRange`) readback via `CVPixelBufferLockBaseAddress` +
  `width_of_plane`/`base_address_of_plane`. Zero-Copy output (`GpuBufferHandle::Metal`) — see the
  section above — landed via ADR-0003, for all four codecs this backend supports.
- **General GOP (P/B frames), not IDR-only** — `VTDecompressionSession` is a black-box HW/SW
  decoder (like Android's `AMediaCodec`, unlike VA-API): the OS owns the DPB/reference-picture
  list entirely. This crate never builds one.
- Format description: `CMVideoFormatDescriptionCreateFromH264ParameterSets` from raw SPS/PPS NAL
  bytes (no start code) — reuses `iso_bmff::bitstream::avc::parse_avc_decoder_config` unchanged.
- Byte framing: AVCC 4-byte length-prefixed only, via `iso_bmff::bitstream::avc::to_avcc`
  (Annex-B → AVCC) reused unchanged in the decode direction too. Non-4-byte length sizes or
  multiple SPS/PPS → `DecodeError::Unsupported`, never silently misdecoded.

## Output ordering — real landmine, resolved not ignored

`VTDecompressionOutputCallback`'s own doc comment: **"will not necessarily be called in display
order."** Fix: set `kVTDecodeFrame_EnableTemporalProcessing` on every `decode_frame` call —
VideoToolbox itself reorders to presentation order internally. This crate implements **no**
PTS-sorting reorder buffer of its own. Trade-off: the callback becomes genuinely async (may fire
after `decode_frame` returns) — bridged via the same `Arc<Mutex<VecDeque<VideoFrame>>>` shape
`mediaway-encoder::apple` already uses for its own callback.

## `CVImageBuffer → CVPixelBuffer` downcast — checked, not unchecked

Confirmed real: `objc2_core_foundation::CFType::downcast_ref::<CVPixelBuffer>()` (checked via
`ConcreteType::type_id()`), not a raw pointer cast. `None` → skip the frame, not a panic.

## Deps (added to `Cargo.toml`)

Same `objc2-*` family/version (`"0.3"`) `mediaway-encoder::apple` already pins, in a
`[target.'cfg(any(target_os = "macos", target_os = "ios"))'.dependencies]` block:
`objc2-video-toolbox`, `objc2-core-media` (`CMBlockBuffer`), `objc2-core-video` (`CVReturn`),
`objc2-core-foundation` — see the ADR's Decision section for exact reasoning per feature.
ADR-0002 added `CFData`/`CFPropertyList` (VP9/AV1's extension-atom dictionary). ADR-0006 (ProRes)
added zero new deps/features — everything it needs was already enabled.

## Open items (not settled from local `objc2` source)

- `VTDecompressionSessionInvalidate` callback-cutoff ordering — same unconfirmed gap the encoder
  ADR flagged for `VTCompressionSessionInvalidate`; mitigated the same way (`wait_for_asynchronous_frames()`
  before `invalidate()` in `Drop`, not proven sufficient).
- The `CMBlockBuffer::with_memory_block` second `memcpy` the ADR flagged: resolved during
  implementation by taking the straightforward owned-copy path (`Box<Vec<u8>>`, freed by a
  `custom_block_source.FreeBlock` callback) rather than an unproven raw-`Bytes`-pointer handoff —
  see `create_block_buffer` in `videotoolbox/video.rs`.
