# Windows encode — D3D12 native Video Encode API

Split out of [`windows-encode.md`](windows-encode.md) (100-line limit) — this page covers
only the **native D3D12 Video Encode API** path (`ID3D12VideoDevice3`/`ID3D12VideoEncoder`),
a real, distinct encode API separate from feeding D3D12 textures into WMF, reachable with
**zero new dependency cost** via `windows` features this crate already enables.

- **Implemented 2026-07-29:**
  [`d3d12_video_encode`](../../../../crates/mediaway-encoder/src/windows/d3d12_video_encode.rs)
  — H.264 Main, CPU-upload NV12, all-intra, fixed CQP, hand-written Annex-B SPS/PPS (driver
  only emits the slice NAL). **Not wired into the public API yet** (`auto.rs`/
  `WindowsVideoEncoder`) — `lib.rs` declares a private `mod d3d12_video_encode;` so its
  hardware-gated tests still compile/run under normal `cargo test`. **Real hardware encode
  confirmed on an RTX 4090** — real SPS+IDR NALs out of a real `EncodeFrame`. Three
  driver-real gotchas (ground-truthed against FFmpeg's `d3d12va_encode.c`, no code copied):
  (1) `D3D12_FEATURE_VIDEO_ENCODER_OUTPUT_RESOLUTION` reports a real minimum resolution
  (160x64 observed) — below it `CreateVideoEncoderHeap` fails `E_INVALIDARG` with no other
  diagnostic; (2) `D3D12_HEAP_TYPE_READBACK` resources cannot be transitioned to
  `VIDEO_ENCODE_WRITE`/`_READ` — resolve via `GetCustomHeapProperties` first; (3) a
  hardcoded H.264 level reliably fails heap creation — must use
  `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT`'s driver-reported `SuggestedLevel`.
- **HEVC extension (2026-07-29):** same module, HEVC Main profile
  (`hevc.rs`/`ops_hevc.rs`/`bitstream_hevc.rs`). Real hardware encode confirmed — genuine
  VPS(32)/SPS(33)/PPS(34)/IDR(19/20) Annex-B NALs. Two more gotchas: (1)
  `D3D12_FEATURE_VIDEO_ENCODER_CODEC_CONFIGURATION_SUPPORT` reports unsupported
  unconditionally for HEVC on this driver — sweep `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT`
  directly instead; (2) codec config needs fixed 32x32 CTU + full 4x4–32x32 TU range +
  `USE_ASYMETRIC_MOTION_PARTITION`, only surfaced via the debug layer at `CreateVideoEncoder`
  time, not the advisory query.
- **AV1 extension (2026-07-29), corrected (2026-08-07):** implemented —
  `av1.rs`/`ops_av1.rs`/`bitstream_av1.rs`. The original "blocked by this driver" conclusion
  was **wrong** — it was reading the wrong feature query. The official D3D12 AV1 spec states
  `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` never works for AV1 (always `CODEC_NOT_SUPPORTED`,
  regardless of driver); the real query is `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT1`. Switching
  to it on the RTX 4090 (same driver as the original finding) surfaces a real, different, and
  narrower rejection instead: `CODEC_CONFIGURATION_NOT_SUPPORTED` for this backend's
  all-`FEATURE_FLAG_NONE` config. `D3D12_FEATURE_VIDEO_ENCODER_CODEC_CONFIGURATION_SUPPORT`
  reports this driver *requires* `AUTO_SEGMENTATION | CDEF_FILTERING |
  LOOP_RESTORATION_FILTER` to be *declared* (session-level only — no frame is forced to
  actually use them). Declaring them unblocked `EncodeFrame` itself, which then surfaced (and
  this pass fixed) two more real bugs: this driver also forces
  `ENABLE_FRAME_SEGMENTATION_AUTO` per frame once declared, and a pre-existing
  `ReferenceFramesReconPictureDescriptors` bug (zeroed `Default` looked like a valid DPB slot
  0 instead of the required `0xFF` sentinel for "unused"). A third real bug — AV1's resolved-
  metadata buffer needs a larger, AV1-specific layout than H.264/HEVC's, previously
  under-allocated — was found and fixed the same pass. `EncodeFrame` now succeeds and
  `ffprobe` parses the real sequence header out of the output, but the frame data still isn't
  decodable (`libdav1d`: 100% error rate) — confirmed *not* a segmentation mismatch (driver's
  own post-encode `NumSegments == 0` matches this backend's hardcoded value). No FFmpeg D3D12
  AV1 reference exists to diff against (unlike H.264/HEVC GOP or Vulkan AV1); root cause not
  yet found. D3D12 Video Decode not attempted at all — distinct API surface.
- **H.264/HEVC GOP/P-frame support (2026-08-06):** `gop_size > 1` now real, single forward
  reference (mirrors Vulkan's own GOP design). New `gop.rs`/`gop_hevc.rs` (pure-Rust
  `H264GopState`/`HevcGopState`) + `setup::ReconPool` — **one** 2-slice texture array,
  ping-ponged, not two separate resources. **Real hardware `IPPIPPI` cadence confirmed on
  the RTX 4090** for both codecs, reproduced repeatedly. Two real device-removal incidents
  on the way there (H.264 only — HEVC reused the fix from the start and worked first try),
  both root-caused via the official D3D12 spec (fetched to
  `local/standards/d3d12-video-encoding-h264-hevc/` per
  `docs/conventions/external-standards.md`) rather than more hardware guessing: (1)
  `RECONSTRUCTED_FRAMES_REQUIRE_TEXTURE_ARRAYS` is set on this driver — separate individual
  resources are invalid, not merely suboptimal; (2) the actual root cause —
  `D3D12_VIDEO_ENCODER_PICTURE_CONTROL_FLAG_USED_AS_REFERENCE_PICTURE` must be set on every
  frame providing a non-null `ReconstructedPicture` output; the resource alone, without the
  flag, is undefined behavior.
- **Row-based intra refresh (2026-08-06):** `VideoEncoderConfig::intra_refresh_period` —
  unbounded GOP (`GOPLength = 0`) + continuous refresh waves instead of periodic IDR, for
  H.264 and HEVC. Reuses the GOP work's `ReconPool`/`USED_AS_REFERENCE_PICTURE` wiring
  unchanged. **Real-hardware finding:** `GENERAL_SUPPORT_OK` passing is not sufficient —
  `D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOLUTION_SUPPORT_LIMITS.MaxIntraRefreshFrameDuration`
  (previously read and discarded by this backend) is the real, resolution-dependent cap;
  `0` means unusable at any nonzero duration even though the coarser mode check passed.
  `open` now validates `period <= MaxIntraRefreshFrameDuration && > 0` before committing,
  falling back to periodic-GOP/IDR-only otherwise. On this RTX 4090 at the tested
  resolutions that cap is `0`, so both hardware tests land in the documented fallback
  rather than a live refresh cadence — the capability-gated path itself (no device
  removal, no invalid `EncodeFrame` reaching the driver) is confirmed correct.
- **AV1 subregion-metadata fix, decodability bug still open (2026-08-07):** the official
  spec's resolved-metadata layout has a `D3D12_VIDEO_ENCODER_FRAME_SUBREGION_METADATA`
  entry (`bStartOffset`/`bSize`) this backend's buffer was already sized for but never
  read — fixed `read_packet_av1` to extract `[bStartOffset, bSize)` instead of trusting
  `EncodedBitstreamWrittenBytesCount` verbatim. **Ruled out as the actual decode bug**:
  `bStartOffset == 0` on the RTX 4090 (no behavior change), confirmed by feeding real
  encoded packets through `ffmpeg`/`libdav1d` directly — same `Decode error rate 1`
  before and after. Root cause remains unfound; no FFmpeg D3D12 AV1 reference exists to
  diff against, and `dav1d`'s CLI-level error is too coarse to localize.
- **CBR rate control + live `set_bitrate` (2026-08-07):** `setup::RateControlState`
  (`Cqp`/`Cbr`) replaces the old bare CQP field; `open` probes CBR once more at the
  already-chosen GOP/intra-refresh tier and falls back to CQP with no error if this
  driver rejects it. New `VideoEncoder::set_bitrate` (default `Unsupported`, real for
  Vulkan H.264 and this backend's H.264/HEVC) mutates `TargetBitRate` in place — live,
  no reopen, since `ops`/`ops_hevc`/`ops_av1` already rebuild the rate-control struct
  fresh every `EncodeFrame`. **Real CBR selected and `set_bitrate` accepted on the RTX
  4090 for both H.264 and HEVC** (not the fixed-QP fallback).

See [ADR-0007](../../../../crates/mediaway-encoder/adr/windows/0007-d3d12-native-video-encode.md)
(+ its 2026-08-06 and 2026-08-07 addenda) for full detail on every finding above, including
how the debug layer (`ID3D12InfoQueue`) surfaced each one.
