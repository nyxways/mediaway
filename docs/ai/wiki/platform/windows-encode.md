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
- **WMF AV1 encode is already codec-generically dispatched** (ADR-0004: `MFTEnumEx`, no
  hardcoded CLSID, same as HEVC/VP9) — a later premise that this needed "wiring up" was
  wrong. **ADR-0010 implemented (2026-08-19)**: `refresh_extradata` is now codec-aware —
  `iso_bmff::bitstream::av1::to_av1c` builds a real `av1C` from the Sequence Header OBU for
  `CodecKind::Av1`; H.264 still uses `avc::to_avcc`; HEVC/VP9 keep the pre-existing
  raw-bytes-verbatim fallback (their own config-record gap is separate, not fixed here).
- **Real `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, …)` encoder probe finding, this host (RTX
  4090 + Intel UHD 770, 2026-08-19)**: an AV1 encoder MFT genuinely **is** registered —
  `MFT_ENUM_FLAG_HARDWARE`-filtered enumeration finds `"NVIDIA AV1 Encoder MFT"` and
  `"Intel® Hardware Accelerated AV1 Encoder MFT"`. This refines, not contradicts, the H.264
  finding above: it was H.264-specific, not "no HW MFT for any codec." But unfiltered
  (`SORTANDFILTER`-only, the flag set `open_cpu`'s CPU-upload path uses) finds **zero** AV1
  MFTs — unlike HEVC/VP9, which each have a software Store-extension encoder in that set —
  so AV1 CPU-upload still gets `Unsupported`. AV1 DX11 Zero-Copy finds the hardware MFT but
  still fails downstream with `EncodeError::Backend` (D3D11-aware/type-negotiation stage),
  the same pre-existing failure class already seen there for HEVC/VP9 DX11 on this host —
  not a new bug, out of ADR-0010's scope. Net: the `av1C` fix is sans-io-unit-verified only
  on this host (no AV1 packet has ever been produced through WMF here to check end-to-end).
  Probe: `wmf::video::tests::list_encoder_mfts_for_each_codec`. See
  [ADR-0010](../../../../crates/mediaway-encoder/adr/windows/0010-wmf-av1-encode-config-record-and-mft-probe.md).
