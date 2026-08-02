# Windows encode (WMF)

- Crate: `mediaway-encoder-windows`
- Codecs: H.264 / HEVC / AV1 / VP9 (`wmf/codec.rs` → MF subtypes)
- CPU: H.264 inbox MFT; others via `MFTEnumEx`
- Zero-Copy: HW MFT + DXGI; input ARGB32 then NV12 (BGRA desktop ZC)
- **GpuCopy bridge:** `D3d12SharedEncodeBridge` — D3D12 shared → native D3D11 (`OpenSharedResource1`; not D3D11On12)
- AAC: sync MFT (PCM → raw AAC)
- ADR: 0001–0005 · [0006 D3D12 share](../../../crates/mediaway-encoder-windows/adr/0006-d3d12-shared-to-d3d11.md)
  · [0007 D3D12 native encode](../../../crates/mediaway-encoder-windows/adr/0007-d3d12-native-video-encode.md)
- Benches: [`docs/benchmarks.md`](../../../crates/mediaway-encoder-windows/docs/benchmarks.md)
  (Criterion, `sw_wmf_h264_cpu` vs `zc_wmf_h264_dx11`). **Driver quirk observed on one
  `ad-hoc` box:** neither an NVIDIA RTX 4090 nor an Intel UHD 770 registered a working
  Media Foundation **encode** HW MFT for H.264 there — NVENC exists but isn't exposed
  as an `IMFTransform` on that driver; `zc_wmf_h264_dx11` came back N/A, not a bug.
  Don't assume every "real HW" box can exercise the DX11 Zero-Copy encode bench.
  Same box, same reason: `mediaway-wgpu`'s `GpuCopy` bridge smoke test and the
  pre-existing `auto_open_gpu_copy_via_d3d12_bridge_or_skip` test both skip
  here too (2026-07-29) — cross-checked as the identical root cause, not two
  separate bugs.
- **D3D12 Video Encode API** (`ID3D12VideoDevice3`/`ID3D12VideoEncoder`,
  Windows 11+, H.264/HEVC from launch + AV1 since 24H2) is a real, distinct
  native encode API — separate from feeding D3D12 textures into WMF —
  reachable with **zero new dependency cost** via `windows` features this
  crate already enables. **Implemented 2026-07-29:**
  [`d3d12_video_encode`](../../../crates/mediaway-encoder-windows/src/d3d12_video_encode.rs)
  — H.264 Main, CPU-upload NV12, all-intra (every frame independent IDR),
  fixed CQP, hand-written Annex-B SPS/PPS (driver only emits the slice NAL).
  **Not wired into the public API yet** (`auto.rs`/`WindowsVideoEncoder`) —
  but `lib.rs` does declare a private `mod d3d12_video_encode;` (added
  2026-07-29, was missing entirely before then) so its hardware-gated tests
  actually compile and run as part of normal `cargo test`. **Real hardware
  encode confirmed on an
  RTX 4090** (`d3d12_native_h264_encode_or_skip` — real SPS+IDR NALs out of a
  real `EncodeFrame`), a genuinely different result from the WMF HW-MFT gap
  noted above (D3D12 native encode bypasses WMF's `IMFTransform` layer
  entirely, so NVENC being unexposed to WMF on this driver doesn't apply
  here). Three driver-real gotchas found only by running on real hardware
  (ground-truthed against FFmpeg's shipped `d3d12va_encode.c`, no code
  copied): (1) `D3D12_FEATURE_VIDEO_ENCODER_OUTPUT_RESOLUTION` reports a real
  minimum resolution (160x64 observed) — below it `CreateVideoEncoderHeap`
  fails `E_INVALIDARG` with no other diagnostic; (2) `D3D12_HEAP_TYPE_READBACK`
  resources cannot be transitioned to `VIDEO_ENCODE_WRITE`/`_READ` — resolve
  via `GetCustomHeapProperties` first (same memory, sidesteps the abstract
  heap-type check); (3) a hardcoded H.264 level reliably fails heap creation —
  must use `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT`'s driver-reported
  `SuggestedLevel`. See
  [ADR-0007](../../../crates/mediaway-encoder-windows/adr/0007-d3d12-native-video-encode.md)
  for full detail + how the debug layer (`ID3D12InfoQueue`) surfaced these.
- **HEVC extension (2026-07-29):** same module, HEVC Main profile added
  (`hevc.rs`/`ops_hevc.rs`/`bitstream_hevc.rs` — HEVC's 2-byte NAL header + VPS
  as a third parameter set didn't fold into the H.264 writer, only its
  `RbspWriter`/emulation-prevention helpers are shared). **Real hardware
  encode confirmed on the RTX 4090** — genuine VPS(32)/SPS(33)/PPS(34)/
  IDR(19/20) Annex-B NALs. Two more driver-real gotchas: (1)
  `D3D12_FEATURE_VIDEO_ENCODER_CODEC_CONFIGURATION_SUPPORT` reports
  unsupported unconditionally for HEVC on this driver — sweep
  `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` directly instead; (2) codec config
  needs fixed 32x32 CTU + full 4x4–32x32 TU range +
  `USE_ASYMETRIC_MOTION_PARTITION`, which only surfaced via the debug layer's
  exact message at `CreateVideoEncoder` time, not the advisory query.
- **AV1 extension (2026-07-29):** implemented — `av1.rs`/`ops_av1.rs`/
  `bitstream_av1.rs` (hand-written OBU temporal-delimiter/sequence-header/
  frame-header writer; `ops_av1`'s packet readback is its own function, not
  the H.264/HEVC-shared one, since AV1's `OBU_FRAME` needs a per-frame
  `leb128` size field). **Blocked from a real hardware round-trip on the
  RTX 4090**: `D3D12_FEATURE_VIDEO_ENCODER_CODEC` reports
  `IsSupported=true` (codec-presence probe), but the full
  `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` query reports
  `CODEC_NOT_SUPPORTED` for every configuration tried — this NVIDIA
  consumer driver doesn't appear to implement AV1 through the **D3D12
  Video Encode API** yet, even though the same GPU's NVENC SDK path (a
  different API) does encode AV1 (see `mediaway-encoder-nvenc`). Code is
  spec-complete and skips honestly (`d3d12_native_av1_encode_or_skip`) —
  no bitstream/`pic_data` bug ruled out yet since `CreateVideoEncoder` is
  never reached. See [ADR-0007](../../../crates/mediaway-encoder-windows/adr/0007-d3d12-native-video-encode.md)'s
  AV1 addendum. D3D12 Video Decode (`ID3D12VideoDecoder`, DXVA-shaped
  picture params, DPB) not attempted at all — distinct API surface,
  comparable scope to the encode work above.
