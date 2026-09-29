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
  (Criterion, `sw_wmf_h264_cpu` vs `zc_wmf_h264_dx11`). **The old "NVENC isn't exposed as
  an `IMFTransform`" finding is retired** (2026-09-18): NVIDIA's encoder MFTs exist but are
  *async*, and three sequencing bugs made every DX11 Zero-Copy open fail — see
  [async-mft](../encode/async-mft.md). DX11 Zero-Copy H.264/HEVC now encodes on an RTX 4090.
  The `zc_wmf_h264_dx11` N/A row and the two `GpuCopy` bridge smoke tests that skipped for
  the same reason (2026-07-29) have not been re-measured since.
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
  `CodecKind::Av1`; H.264 still uses `avc::to_avcc`; HEVC builds a real `hvcC` via `to_hvcc`
  (#98, [ADR-0006 iso-bmff](../../../../crates/iso-bmff/adr/0006-hevc-in-mp4.md)); only VP9
  keeps the raw-bytes-verbatim fallback (separate gap).
- **Real `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, …)` encoder probe finding, this host (RTX
  4090 + Intel UHD 770, 2026-08-19)**: an AV1 encoder MFT genuinely **is** registered —
  `MFT_ENUM_FLAG_HARDWARE`-filtered enumeration finds `"NVIDIA AV1 Encoder MFT"` and
  `"Intel® Hardware Accelerated AV1 Encoder MFT"`. This refines, not contradicts, the H.264
  finding above: it was H.264-specific, not "no HW MFT for any codec." But unfiltered
  (`SORTANDFILTER`-only, the flag set `open_cpu`'s CPU-upload path uses) finds **zero** AV1
  MFTs — unlike HEVC/VP9, which each have a software Store-extension encoder in that set —
  so AV1 CPU-upload still gets `Unsupported`. AV1 DX11 Zero-Copy found the hardware MFT but
  failed downstream with `EncodeError::Backend` at the time — that failure class was the
  async-MFT sequencing bugs ([async-mft](../encode/async-mft.md), ADR-0012), fixed for H.264
  and HEVC; AV1 was not re-measured. Net: the `av1C` fix was sans-io-unit-verified only
  (no AV1 packet had been produced through WMF here to check end-to-end).
- **Timestamps + drop safety (2026-09-18/19):** `to_hns`/`from_hns` are an exact round trip
  and `dts` comes from the MFT — [windows-timestamps](windows-timestamps.md). Dropping an
  async hardware encoder waits a measured 50 ms before releasing the MFT (NVIDIA crash,
  ADR-0012 § Addendum); `VideoEncoder::finish(self)` flushes and returns the tail.
  Probe: `wmf::video::tests::list_encoder_mfts_for_each_codec`. See
  [ADR-0010](../../../../crates/mediaway-encoder/adr/windows/0010-wmf-av1-encode-config-record-and-mft-probe.md).
