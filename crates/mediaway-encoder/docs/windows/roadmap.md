# mediaway-encoder-windows — roadmap

Windows Media Foundation + DX11 encode backend.  
Facade: [`mediaway-encoder`](../../mediaway-encoder/docs/roadmap.md).  
Platform order: **Windows first**. Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Workspace member + docs / ADR surface
- [x] Trait impl placeholders (`open_*` → `Unsupported`)

### 1 — WMF H.264

- [x] MF transform / sample path → [`Packet`](../../mediaway-common) (sync `CLSID_MSH264EncoderMFT`)
- [x] CPU NV12 upload path (`upload_cpu_nv12`, documented cost)
- [x] DX11 `DirectX11` Zero-Copy push (HW MFT + DXGI; needs `d3d11_device`)
- [x] Extradata / `StreamInfo` sequence header when available

### 2 — AAC + integration

- [x] WMF AAC
- [x] Smoke with `mediaway-container` + `mediaway-test-media` (integration `av_fmp4_smoke`)
- [x] DX11 Zero-Copy fMP4 smoke (`av_fmp4_zc_smoke`)

### 3 — Multi-codec

- [x] HEVC / AV1 / VP9 via MF subtype + `MFTEnumEx` (CPU + DX11 Zero-Copy paths)
- [x] D3D12 shared → native D3D11 bridge (`D3d12SharedEncodeBridge`, GpuCopy)
- [ ] Proven CI/`machine_id` cells for each codec (promote 🆗 → ⚡ where earned) —
      `ad-hoc` numbers collected for H.264 `sw_wmf_h264_cpu` encode/decode only
      (see [`docs/benchmarks.md`](benchmarks.md)); `zc_wmf_h264_dx11` is N/A on the
      one ad-hoc host tried (no adapter registers a working D3D11-aware Media
      Foundation encode/decode MFT there), and HEVC/AV1/VP9 are still unmeasured.
      Stays unchecked: no `ref-*` machine profile evidence yet (`ref-*` promotion of
      an ad-hoc box is a maintainer decision, see
      [`docs/benchmarks/machines.md`](../../../docs/benchmarks/machines.md)), and
      per-codec ⚡ promotion needs Zero-Copy numbers, not just `sw`.

### 4 — Opus (research: no inbox encoder MFT)

- [x] Verified via real `MFTEnumEx(MFT_CATEGORY_AUDIO_ENCODER, ..., MFAudioFormat_Opus)`
      (Windows 11 host, this session) that Windows ships **no** inbox Opus encoder MFT:
      the filtered enumeration returns zero results, and none of the 9 registered audio
      encoder MFTs on that machine (AAC, ALAC, FLAC, MP3/ACM, MPEG-2, WMAudio, AMR-NB, WM
      Speech, LPCM DVD-Audio) is Opus. The `windows` crate's Media Foundation bindings
      also expose no Opus encoder CLSID constant (only a decoder one — see
      `mediaway-decoder-windows`'s `docs/roadmap.md`).
- [ ] No `WmfOpusEncoder` was implemented — there is nothing real to wire up. Revisit if
      a future Windows release ships one, or if a non-inbox (bundled) Opus encoder becomes
      an option under `docs/conventions/deps-policy.md`.

### 5 — D3D12 native video-encode (separate from WMF, see ADR-0007)

- [x] H.264 Main, CPU-upload NV12, all-intra, fixed CQP — real hardware verified (RTX 4090)
- [x] HEVC Main, same staging — real hardware verified (RTX 4090); required a hardcoded
      (not driver-queried — that query itself reports unsupported on this driver) codec
      configuration: fixed `32x32` CTU, full `4x4..32x32` TU range,
      `USE_ASYMETRIC_MOTION_PARTITION` required. See ADR-0007 addendum.
- [x] AV1 encode support probed for real (`D3D12_FEATURE_VIDEO_ENCODER_CODEC`) — this
      machine (Windows 11 24H2, RTX 4090) reports `IsSupported == true`. Encode itself
      **not implemented** — AV1's OBU/sequence-header bitstream is substantially more
      machinery than H.264/HEVC's NAL-based parameter sets; scoped as its own follow-up.
- [ ] D3D12 Video Decode API (`ID3D12VideoDevice`/`ID3D12VideoDecoder`/
      `ID3D12VideoDecoderHeap`) — distinct API surface from encode, not started this pass.
- [ ] Still not wired into `src/lib.rs` / `auto.rs` — self-contained, unregistered by
      design until an integration pass decides how it fits `AutoVideoEncoder`'s path
      selection.
- [ ] Zero-Copy GPU input, reference-frame/GOP support, rate-control tuning remain
      deferred for both H.264 and HEVC.
