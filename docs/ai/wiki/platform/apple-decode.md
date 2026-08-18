# Apple decode (`mediaway-decoder::apple`)

ADR: [`crates/mediaway-decoder/adr/apple/0001-videotoolbox-h264-cpu-out.md`](../../../../crates/mediaway-decoder/adr/apple/0001-videotoolbox-h264-cpu-out.md)
— **Accepted, zero compile verification** (no Apple SDK in this dev environment; same posture as
`mediaway-encoder::apple`). Implemented at `src/apple/` per this ADR — `cargo check`/`clippy`
pass on this Windows host (the real `objc2-*`-calling code in `videotoolbox/video.rs` is
`cfg`-gated to Apple targets and cannot be type-checked here; the pure tick/NV12 helpers in
`videotoolbox/codec.rs` have no `objc2-*` dependency and are covered by real, passing unit
tests). Not wired into `auto`/`capability`.

**Corpus note:** no `mediaway-decoder::android` backend exists in this repo yet either, despite
being referenced by a prior task briefing — this ADR is grounded in `linux::vaapi` (CPU-output
structure) and `vulkan` (general-GOP precedent) instead. Do not assume an Android decode ADR
exists until one is actually written.

## Scope (Stage 1)

- H.264 only, CPU NV12 (`VideoRange`) readback via `CVPixelBufferLockBaseAddress` +
  `width_of_plane`/`base_address_of_plane`. No Zero-Copy output yet
  (`GpuBufferHandle::Metal` already exists in `mediaway-common`, wiring deferred).
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
`ConcreteType::type_id()`), not a raw pointer cast. `None` → skip the frame, documented, not a
panic.

## Deps (added to `Cargo.toml`)

Same `objc2-*` family/version (`"0.3"`) `mediaway-encoder::apple` already pins, in a
`[target.'cfg(any(target_os = "macos", target_os = "ios"))'.dependencies]` block:
`objc2-video-toolbox` (`VTDecompressionSession`, `VTDecompressionProperties`, `VTErrors`,
`VTSession`, `objc2-core-media`, `objc2-core-video`), `objc2-core-media` (adds `CMBlockBuffer`
to the encoder's own list), `objc2-core-video` (`CVReturn` instead of the encoder's
`CVPixelBufferPool`), `objc2-core-foundation` (no `CFArray` this stage). See the ADR's Decision
section for the exact reasoning per feature.

## Open items (not settled from local `objc2` source)

- `VTDecompressionSessionInvalidate` callback-cutoff ordering — same unconfirmed gap the encoder
  ADR flagged for `VTCompressionSessionInvalidate`; mitigated the same way (`wait_for_asynchronous_frames()`
  before `invalidate()` in `Drop`, not proven sufficient).
- The `CMBlockBuffer::with_memory_block` second `memcpy` the ADR flagged: resolved during
  implementation by taking the straightforward owned-copy path (`Box<Vec<u8>>`, freed by a
  `custom_block_source.FreeBlock` callback) rather than an unproven raw-`Bytes`-pointer handoff —
  see `create_block_buffer` in `videotoolbox/video.rs`.
