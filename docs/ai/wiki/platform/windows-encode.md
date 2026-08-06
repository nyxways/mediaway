# Windows encode (WMF)

- Module: `mediaway-encoder::windows`
- Codecs: H.264 / HEVC / AV1 / VP9 (`wmf/codec.rs` → MF subtypes)
- CPU: H.264 inbox MFT; others via `MFTEnumEx`
- Zero-Copy: HW MFT + DXGI; input ARGB32 then NV12 (BGRA desktop ZC)
- **GpuCopy bridge:** `D3d12SharedEncodeBridge` — D3D12 shared → native D3D11 (`OpenSharedResource1`; not D3D11On12)
- AAC: sync MFT (PCM → raw AAC)
- ADR: 0001–0005 · [0006 D3D12 share](../../../../crates/mediaway-encoder/adr/windows/0006-d3d12-shared-to-d3d11.md)
  · [0007 D3D12 native encode](../../../../crates/mediaway-encoder/adr/windows/0007-d3d12-native-video-encode.md)
- Benches: [`docs/benchmarks.md`](../../../../crates/mediaway-encoder/docs/windows/benchmarks.md)
  (Criterion, `sw_wmf_h264_cpu` vs `zc_wmf_h264_dx11`). **Driver quirk observed on one
  `ad-hoc` box:** neither an NVIDIA RTX 4090 nor an Intel UHD 770 registered a working
  Media Foundation **encode** HW MFT for H.264 there — NVENC exists but isn't exposed
  as an `IMFTransform` on that driver; `zc_wmf_h264_dx11` came back N/A, not a bug.
  Don't assume every "real HW" box can exercise the DX11 Zero-Copy encode bench.
  Same box, same reason: `mediaway::wgpu`'s `GpuCopy` bridge smoke test and the
  pre-existing `auto_open_gpu_copy_via_d3d12_bridge_or_skip` test both skip
  here too (2026-07-29) — cross-checked as the identical root cause, not two
  separate bugs.
- **D3D12 Video Encode API** (`ID3D12VideoDevice3`/`ID3D12VideoEncoder`) is a
  real, distinct native encode API — separate from feeding D3D12 textures
  into WMF. H.264/HEVC/AV1 all-intra, H.264/HEVC GOP + row-based intra
  refresh, real hardware-verified on an RTX 4090; AV1 needs real
  CDEF/restoration/segmentation bitstream support to clear a driver
  codec-configuration requirement (not the flat driver gap once believed).
  See [`windows-encode-d3d12.md`](windows-encode-d3d12.md) for full detail
  (split out to stay under this page's 100-line limit).
