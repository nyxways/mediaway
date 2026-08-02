# mediaway-decoder-windows — roadmap

Windows Media Foundation + DX11 decode backend.  
Facade: [`mediaway-decoder`](../../mediaway-decoder/docs/roadmap.md).  
Platform order: **Windows first**. Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Workspace member + docs / ADR surface
- [x] Trait impl placeholders (`open` → `Unsupported` on non-Windows)

### 1 — WMF H.264 DX11 Zero-Copy

- [x] HW decoder MFT enum (`MFT_CATEGORY_VIDEO_DECODER`, H.264 → NV12)
- [x] DXGI device manager + `SET_D3D_MANAGER` + async unlock
- [x] `push_packet` / `poll_frame` / `flush` with DXGI output surfaces
- [x] Texture lifetime until next `push_packet` / `poll_frame` / `flush` recycles
- [x] CPU decode path (`VideoOutputPreference::CpuFramesOk`) — software H.264 decoder MFT
      (`MFT_ENUM_FLAG_SYNCMFT`), no `ID3D11Device` / DXGI manager required
- [x] Bitstream round-trip smoke test — `tests/cpu_roundtrip.rs` encodes via
      `mediaway-encoder-windows` CPU-upload H.264 and decodes the real packets through the
      new CPU path (dev-dependency; skips honestly if MF is unavailable)

### 2 — Integration

- [x] Demuxer → decode → encode smoke with `mediaway-container` — see
      `mediaway-pipeline`'s `tests/trim_and_splice_windows.rs` (decode → trim → splice →
      re-encode → mux → demux → decode round trip) and `examples/pipeline/trim_and_splice.rs`
- [x] Annex-B vs AVCC extradata policy documented + tested — see ADR-0001; AVCC-framed
      demuxed `extra_data`/packets are converted to Annex-B before reaching the MFT
      (`iso_bmff::bitstream::avc::{parse_avc_decoder_config, annex_b_sequence_header,
      avcc_payload_to_annex_b}`)

### 3 — Opus decode (research + real MFT, not yet wired into a public API)

- [x] Verified via real `MFTEnumEx`/`CoCreateInstance` (Windows 11 host, this session)
      that Windows ships an inbox Opus **decoder** MFT — `CLSID_MSOpusDecoder` /
      `CMSOpusDecMFT`, `{63E17C10-2D43-4C42-8FE3-8D8B63E46A6A}` — but **no** inbox Opus
      **encoder** MFT (see `mediaway-encoder-windows`'s `docs/roadmap.md` for that side).
      `MFAudioFormat_Opus` = `{0000704F-0000-0010-8000-00AA00389B71}` (confirmed from the
      `windows` crate's SDK-metadata-derived binding, not hand-transcribed).
- [x] `src/wmf/opus.rs` — self-contained `WmfOpusDecoder` MFT session (open / push_packet /
      poll_frame / flush), gated behind a new `audio` Cargo feature. The decoder only ever
      offers one output type (`MFAudioFormat_Float`, 32-bit IEEE float, at the negotiated
      rate/channels) — a hand-built 16-bit PCM output type is rejected
      (`MF_E_INVALIDMEDIATYPE`), so the session takes the decoder's own proposed output
      type from `GetOutputAvailableType` instead of constructing one.
- [x] Real decode round trip in `src/wmf/opus_tests.rs` — pushes a real, spec-valid
      minimal 1-byte Opus packet (RFC 6716 §3.1 TOC-only / zero-length-frame PLC-DTX
      signal, since there is no inbox encoder to produce real compressed audio) and
      asserts a real 480-samples/channel Float32 PCM frame comes back at 48 kHz stereo.
      No new Cargo dependency needed for this — a full encode→decode round trip using an
      external Opus encoder is a separate, deliberate deps-policy decision, not taken here.
- [ ] Not wired into any public entry point: `mediaway-decoder` has no `AudioDecoder`
      trait today (only `VideoDecoder`), so `WmfOpusDecoder` has no facade shape to
      implement against. Designing that trait, plus wiring `mod opus;`/`CodecKind::Opus`
      into `src/lib.rs` / `src/wmf/codec.rs`, is a follow-up integration pass (left
      unregistered here deliberately).

### 4 — HEVC / AV1 / VP9 CPU decode (research + real MFT sessions, not yet wired into a public API)

Verification host: Windows 11, NVIDIA RTX 4090 + Intel UHD 770 (this session). Real
`MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
input=<codec subtype>, output=MFVideoFormat_NV12)` results (see
`src/wmf/video_cpu_tests.rs::list_decoder_mfts_for_each_codec`, which also runs the
unfiltered enumeration for comparison — both returned the same single MFT per codec here):

| Codec | Decoder MFT found | Friendly name |
|-------|--------------------|----------------|
| HEVC  | yes (Store extension) | `HEVCVideoExtension` |
| AV1   | yes (Store extension) | `AV1VideoExtension` |
| VP9   | yes (Store extension) | `VP9VideoExtensionDecoder` |

All three are optional Windows Store extensions (HEVC Video Extensions / AV1 Video
Extension / VP9 Video Extensions), not inbox — absence on a clean Windows install (no
extensions bought/installed) is a real, expected possibility this crate must keep handling
honestly (`DecodeError::Unsupported` from `open_sw_decoder`, no fabricated fallback).

- [x] `src/wmf/video_cpu.rs` — `WmfMultiCodecCpuDecoder`, a self-contained CPU (software)
      decode session for HEVC/AV1/VP9 mirroring `h264.rs`'s CPU-only path (`open_sw_decoder`
      + `configure_decode_types` + direct `ProcessInput`/`ProcessOutput`), but without
      H.264's DX11 Zero-Copy branch or its AVCC→Annex-B `extra_data`/NAL conversion (these
      codecs' packets/`extra_data` are used as produced by `mediaway-encoder-windows`
      as-is). Gated behind the existing `video` feature; kept unregistered from
      `src/lib.rs` (declared in `src/wmf/mod.rs` only, no `pub(crate) use`) — same
      not-yet-wired posture as `src/wmf/opus.rs`, since `mediaway-decoder`'s Windows
      backend dispatches every codec through `WmfH264Decoder` today.
- [x] **Real bug found + fixed**: `MF_E_TRANSFORM_STREAM_CHANGE` handling. H.264's
      `apply_stream_change` (in `h264.rs`, shared via `shared::configure_decode_types`)
      rebuilds an output media type from the width/height already known at `open()` and
      re-submits it. That never gets exercised for H.264 in practice (the caller-given
      width/height already matches the bitstream), but the HEVC/AV1 Store-extension
      decoder MFTs on this host only learn the real output geometry once they parse the
      first frame, and **reject** a caller-reconstructed output type after a stream change
      (confirmed with a raw-HRESULT diagnostic: `SetOutputType` with our own type failed,
      `ProcessOutput` kept returning `MF_E_TRANSFORM_STREAM_CHANGE` (`0xC00D6D61`) forever,
      surfacing as `DecodeError::Backend` from `flush()`). Fix: `negotiate_nv12_output_type`
      in `video_cpu.rs` queries the MFT's own `GetOutputAvailableType(0, i)` candidates and
      `SetOutputType`s with the one whose subtype is NV12, instead of reconstructing one —
      confirmed working via the same raw-HRESULT diagnostic (`ProcessOutput` then returned
      a real sample). `h264.rs`/`shared.rs` were **not** touched (this crate's shared
      helpers, out of this session's scope) — the same latent fragility likely exists there
      too if a future H.264 decoder MFT ever needs mid-stream renegotiation; worth
      revisiting in a follow-up.
- [x] Real end-to-end CPU decode verified for **HEVC and VP9**: `src/wmf/video_cpu_tests.rs`
      encodes one real gradient (non-flat, so decode output can be checked for actual
      varying content, not a zeroed buffer) NV12 frame via `mediaway-encoder-windows`
      CPU-upload (already real on this host — see `mediaway-encoder-windows`'s
      `docs/roadmap.md`), then decodes the real packets through `WmfMultiCodecCpuDecoder`.
      Both asserted real 64×64 NV12 output with genuine pixel variance.
- [ ] **AV1 encode→decode round trip not verified this way**: this host has no AV1 encoder
      MFT at all (`mediaway-encoder-windows`'s `docs/roadmap.md`: `MFTEnumEx` for
      `MFT_CATEGORY_VIDEO_ENCODER` + `MFVideoFormat_AV1` returns nothing usable), so there
      is no real Mediaway-encoded AV1 bitstream to decode this way.
- [x] **AV1 decode partially verified via a system-`ffmpeg` oracle** (optional test/dev
      oracle, [ADR-0002](../../../docs/adr/0002-system-oracle.md);
      `src/wmf/video_cpu_tests.rs::decode_real_ffmpeg_av1_bitstream_or_skip`, skips cleanly
      when `ffmpeg` is absent): encoded a real `testsrc` pattern with `ffmpeg`'s
      `libaom-av1` into an IVF file (parsed locally — 32-byte header + `[size][pts][OBU
      payload]` chunks), then decoded through `WmfMultiCodecCpuDecoder`. Real finding: the
      `AV1VideoExtension` decoder MFT accepts the real bitstream and negotiates a real
      output type via `ProcessOutput`'s stream-change path, but for this content it only
      ever proposes `MFVideoFormat_AYUV` — `GetOutputAvailableType(0, 1)` immediately
      returns `MF_E_NO_MORE_TYPES`, NV12 is never offered. Since this crate's decode
      sessions are NV12-only by design, `negotiate_nv12_output_type` returns
      `DecodeError::Unsupported` (not `Backend` — this is an honest "real decoder MFT, but
      this stream doesn't negotiate to the pixel format this crate supports", not a
      transport failure) and the test skips rather than asserting a fabricated pass. Why
      the MFT proposes AYUV instead of NV12 for this particular `libaom-av1` stream was not
      root-caused (would need reverse-engineering the extension's internal negotiation
      logic against AV1 sequence-header color config); worth a follow-up if AV1 CPU decode
      becomes a priority.
- [ ] Not wired into any public entry point — same reasoning as Opus above; a follow-up
      integration pass decides whether HEVC/AV1/VP9 route through `WmfMultiCodecCpuDecoder`
      as-is, or `h264.rs`/`shared.rs` grow the `negotiate_nv12_output_type` fix and this
      module folds into `WmfH264Decoder` (crate-local ADR territory either way — multiple
      codecs already share `video_subtype`/`is_supported_video_codec` in `codec.rs`).
- [ ] AYUV (or other non-NV12) output support, DX11 Zero-Copy for these three codecs, and
      root-causing the AV1 AYUV-vs-NV12 negotiation are all out of scope here.

### 5 — D3D12 native video-decode (separate from WMF, see ADR-0002)

- [x] ADR-0002 drafted: general-GOP (P/B, DPB) H.264/HEVC/AV1 decode via
      `ID3D12VideoDevice`/`ID3D12VideoDecoder`/`ID3D12VideoDecoderHeap` +
      `ID3D12VideoDecodeCommandList1::DecodeFrame1`, fixed-size texture-array DPB, Zero-Copy
      `GpuBufferHandle::DirectX12` output with a bounded-outstanding-handle backpressure
      contract (FFmpeg hwaccel surface-pool model).
- [x] **H.264 implemented** (this session; HEVC/AV1 not started — separate follow-up
      tasks): real SPS/PPS/slice-header parsing (`h264_sps_pps.rs`/`h264_slice.rs`, built
      on `mediaway_sw::h264::BitReader`/`NalUnit`, not the IDR-only `mediaway_sw::h264::
      {Sps,Pps,SliceHeader}`), POC types 0/1/2 (`h264_poc.rs`), `RefPicList0`/`RefPicList1`
      construction + sliding-window DPB eviction (`h264_refs.rs`), hand-defined DXVA-shaped
      picture-parameter/slice-control/scaling-matrix structs (`h264_pic_params.rs` — see
      Addendum: the `windows` crate has the D3D12 decode *plumbing* but not the DXVA
      structs themselves), a codec-generic fixed-size DPB slot pool (`dpb.rs`) and
      `open`-time helpers (`setup.rs`), and per-frame `DecodeFrame1` submission + CPU
      readback (`ops.rs`). Still unregistered (`mod d3d12_video_decode;`, not `pub mod`).
      `cargo check`/`clippy --all-targets`/`test`, all `--features video`: clean, 0
      warnings, 45 unit tests (pure SPS/PPS/slice/POC/ref-list/DPB logic, no hardware)
      + 1 hardware-gated integration test pass. See ADR-0002's 2026-07-29 Addendum for
      full findings (DXVA struct absence, DPB sizing used, real scope cuts found only
      while implementing — SP/SI slices, explicit weighted prediction, custom scaling
      lists).
- [x] **Root-caused the real hardware hang** (2026-07-30, ADR-0002 2026-07-30 Addendum):
      instrumented the integration test with `ID3D12Debug`/`ID3D12InfoQueue` (same
      technique ADR-0007 used). Found and fixed three real bugs: (1) readback buffer
      sized as tightly-packed NV12 instead of row-pitch-aligned; (2) **NV12's two
      planes only had the luma plane's `ResourceBarrier`d — the chroma plane stayed in
      `COMMON`**, which `DecodeFrame1` rejected with a named debug-layer message and is
      the strongest candidate for the actual GPU hang; (3) `DXVA_Slice_H264_Long::
      BitOffsetToSliceData` was computed against the de-emulated RBSP instead of being
      translated back to the raw (escape-bytes-included) NAL payload the hardware
      actually reads. Ruled out: coded resolution too small (tested at CIF 352x288,
      hang reproduced identically) and `D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS`
      (reverted, hang persisted identically). **Honest current status: the GPU hang
      still reproduces** after all three fixes, but the D3D12 debug layer now reports
      **zero** validation messages before the TDR — every API-usage/resource-state
      concern it can check is clean, so the remaining root cause is very likely inside
      the opaque `DXVA_PicParams_H264`/`DXVA_Qmatrix_H264` blob content itself (not
      visible to the debug layer). Six real hardware TDRs were triggered this session;
      stopped further blind hardware iteration deliberately rather than keep resetting
      the GPU on speculation — see the Addendum's "not yet tried" list (byte-for-byte
      diff against this crate's own working WMF/DXVA2 decode path; Nsight Aftermath;
      synthetic minimal streams) for the next session's starting point.
- [ ] Open questions still unresolved (per ADR-0002 Addendum): `DecodeError` has no
      dedicated DPB-backpressure variant (`Backend` used instead, flagged as a
      `mediaway-decoder` facade follow-up); `GpuBufferHandle::DirectX12` has no
      `subresource` field, so this module's Zero-Copy path returns a local
      `DecodedOutput`/`DecodedFrame` type instead of `mediaway_common::VideoFrame`
      (cross-crate follow-up); whether general-GOP H.264/HEVC/AV1 high-level-syntax
      parsing should become a shared sans-io crate (overlaps with
      `mediaway-decoder-linux`'s VA-API needs); no POC-based display-order reorder
      ("bumping") buffer yet — output is in decode order.
- [ ] HEVC, AV1 — not started; separate follow-up tasks per ADR-0002.
- [ ] Integration pass: make the module `pub`, wire into `WindowsVideoDecoder`'s
      `Backend` dispatch, decide the `GpuBufferHandle`/`DecodeError` cross-crate
      questions above.

### 6 — `D3d11SharedDecodeBridge` (wgpu decode interop — ADR-0003)

Companion type to `mediaway-wgpu`'s `WgpuDx12DecodeBridge`
([ADR-0002](../../mediaway-wgpu/adr/0002-decode-to-wgpu-texture-bridge.md)
there): bridges this crate's own WMF DX11 Zero-Copy decode output (Stage 1
above) into a shared D3D12 resource `mediaway-wgpu` can wrap as a
`wgpu::Texture`. `GpuCopy` cost class (one `CopySubresourceRegion` + a bounded
CPU↔GPU query-poll stall per frame), not Zero-Copy.

- [x] [ADR-0003](../adr/0003-d3d11-shared-decode-bridge.md) drafted: module
      placement, `DecodeError`/`NativeHandle` reuse (no new variants, no new
      representation), full `unsafe`/`SAFETY` call inventory, `Drop`
      (`CloseHandle` on the shared `HANDLE` only, mirroring
      `D3d12SharedEncodeBridge`), and an honest residual-risk list for the
      `windows`-crate 0.62 signatures not independently fetched this session.
- [x] `src/d3d11_shared_decode_bridge.rs` implemented (2026-07-31): `open` /
      `copy_from_decoded` / `d3d12_resource_handle` per ADR-0003 § Decision.
      Real `windows`-crate 0.62.2 signatures checked against the crate's
      vendored source; two of the six flagged residual-risk items needed a
      real fix (`ID3D12Device::OpenSharedHandle`'s out-param shape,
      `D3D11_RESOURCE_MISC_FLAG`'s `i32`→`u32` cast), one needed a fix in the
      easier-than-assumed direction (`ID3D11DeviceChild::GetDevice` is a
      plain `Result<ID3D11Device>`-returning wrapper), three compiled as the
      ADR assumed. Found and fixed one correctness issue beyond the ADR's own
      list: `ID3D11DeviceContext::GetData`'s `Result<()>` collapses S_OK and
      S_FALSE (both non-negative HRESULTs) to `Ok(())`, so the poll loop
      checks the actual `BOOL` out-param, not just `.is_ok()`. See ADR-0003's
      2026-07-31 Addendum for the full signature-by-signature account.
      `cargo check`/`clippy --all-features -D warnings`/`fmt --check` (new
      files only — this crate's other files have pre-existing, unrelated fmt
      drift)/`test`: clean, 48 lib tests + 1 integration test pass.
- [x] Hardware smoke test mirroring `d3d12_shared_bridge_open_or_skip`'s
      shape (`mediaway-encoder-windows`) —
      `d3d11_shared_decode_bridge_tests.rs::open_same_adapter_or_skip`.
      **Hardware-verified this session, not just a graceful skip**: opened a
      real `ID3D11Device` + `ID3D12Device` pair on the same explicit adapter
      and both `D3d11SharedDecodeBridge::open` and `d3d12_resource_handle()`
      succeeded on the primary adapter.
- [ ] Real decode → bridge → `mediaway-wgpu` round trip — still blocked on
      there being no working H.264 decode HW MFT available (same limitation
      ADR-0001's own test already hits); `copy_from_decoded` itself remains
      unverified against real decode output.
