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
- [x] AV1 all-intra encode implemented (hand-written OBU temporal delimiter + sequence
      header + frame header, see `bitstream_av1.rs`/`ops_av1.rs`) — the original
      `CODEC_NOT_SUPPORTED` driver-gap conclusion was **wrong**, corrected 2026-08-07: it
      was calling the wrong feature query (`_SUPPORT` never works for AV1, per the official
      spec). Switching to `_SUPPORT1` surfaces a real, narrower, likely-fixable rejection
      instead — this driver requires `AUTO_SEGMENTATION | CDEF_FILTERING |
      LOOP_RESTORATION_FILTER` in the codec configuration. Still not hardware round-tripped —
      needs real CDEF/restoration/segmentation bitstream support in `bitstream_av1.rs` to
      match. See ADR-0007's 2026-08-07 addendum.
- [x] H.264 GOP/P-frame support (single forward reference, `gop_size > 1`) — real hardware
      verified (RTX 4090, real `IPPIPPI` NAL cadence). See ADR-0007's 2026-08-06 addendum.
- [x] HEVC GOP/P-frame support — ported same session, same design, worked on the first
      real-hardware attempt (root cause already known from H.264). Real hardware verified
      (RTX 4090, real `IPPIPPI` NAL cadence).
- [x] Row-based intra refresh (`VideoEncoderConfig::intra_refresh_period`, H.264 + HEVC) —
      unbounded GOP + continuous refresh waves instead of periodic IDR. Capability-gated on
      `MaxIntraRefreshFrameDuration` (a real, resolution-dependent driver cap this backend
      previously read and discarded); on this RTX 4090 at the tested resolutions that cap is
      `0`, so both hardware tests exercise the documented IDR-only fallback rather than a
      live refresh cadence — the capability-gated path itself is confirmed correct (no
      device removal, no invalid `EncodeFrame` reaches the driver). See ADR-0007's
      2026-08-06 addendum.
- [ ] D3D12 Video Decode API (`ID3D12VideoDevice`/`ID3D12VideoDecoder`/
      `ID3D12VideoDecoderHeap`) — distinct API surface from encode, not started this pass.
- [ ] Still not wired into `src/lib.rs` / `auto.rs` — self-contained, unregistered by
      design until an integration pass decides how it fits `AutoVideoEncoder`'s path
      selection.
- [ ] Zero-Copy GPU input and rate-control tuning remain deferred for H.264/HEVC/AV1.
      Intra-refresh remains deferred and is only meaningful once GOP/P-frame support
      exists for a codec (all-intra streams have nothing to "refresh").
