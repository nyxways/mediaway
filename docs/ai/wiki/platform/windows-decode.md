# Windows decode (WMF)

- Module: `mediaway-decoder::windows`
- Codecs: H.264 / HEVC / AV1 / VP9 HW decoder MFT → DX11 `GpuBufferHandle` (Nv12)
- CPU out (`VideoOutputPreference::CpuFramesOk`): verified working for H.264 (2026-08-05) —
  see § CPU decode bug below.
- README: OS · GPU / D3D11 decode 🆗 (open may skip without HW MFT)
- ADR: [0001](../../../../crates/mediaway-decoder/adr/windows/0001-wmf-h264-dx11-out.md)
- Benches: [`docs/benchmarks.md`](../../../../crates/mediaway-decoder/docs/windows/benchmarks.md)
  (Criterion, `sw_wmf_h264_cpu` vs `zc_wmf_h264_dx11`). Same `ad-hoc` box as the
  encode page also had **no** working Media Foundation decode HW MFT on either GPU
  (`DecodeError::Unsupported` on both NVIDIA RTX 4090 and Intel UHD 770) even though
  `ffmpeg h264_cuvid`/NVDEC decodes fine outside MF — `zc_wmf_h264_dx11` came back
  N/A there, consistent with `open_dx11_zero_copy_or_skip`'s own graceful skip.

## CPU decode bug (found + fixed 2026-08-05)

Found while adding `mediaway-ffi`'s decode C ABI (`adr/pipeline/0004-auto-decode-c-abi.md`):
two real bugs, both fixed — both tests un-`#[ignore]`d:

- `cpu_roundtrip.rs` (moved from `tests/windows/` — nested `tests/<dir>/*.rs` files aren't
  auto-discovered by `cargo test`) returned **zero frames** for a real WMF-encoder H.264
  stream, no error. Root cause: the WMF encoder emits **Annex-B packets but an avcC
  `extra_data`** (normalized for the container), so the decoder's AVCC→Annex-B conversion
  — keyed on `extra_data` alone — corrupted the Annex-B packets and the MFT never emitted.
  Fix (`wmf/shared.rs::packet_to_sample`): per-payload Annex-B start-code probe
  (`iso_bmff::bitstream::avc::is_annex_b`, the muxer's own test); AVCC packets still convert.
- `decode_smoke.rs` (mux → demux → decode, 10 frames) looked like a `poll_frame` abort
  (`Alignment::new_unchecked requires a power of two`) but traced to a **double-free in
  the test**: `mediaway_encode_session_finish` consumes the encode session; the test then
  called `mediaway_encode_session_close(session)` anyway — stray close removed; all 10
  frames decoded fine. The std-`Alignment` attribution was wrong.

## D3D12 native decode (H.264 implemented, unregistered — ADR-0002)

- A **second**, independent decode path (`ID3D12VideoDecoder`/`ID3D12VideoDecoderHeap`/
  `DecodeFrame1`) alongside WMF, self-contained and **not** wired into `WindowsVideoDecoder`
  yet (`mod d3d12_video_decode;`, non-`pub`, same trick ADR-0007's encode module used). Unlike
  WMF, the app parses the bitstream itself (DXVA-shaped picture-parameter buffers per codec).
- Scope: general GOP (P/B refs, real DPB) for **H.264** (real SPS/PPS/slice/POC-types-0/1/2/
  ref-list/sliding-window-DPB logic); HEVC narrowed to single-forward-ref P-slice; AV1
  narrowed to `KEY_FRAME`-only (no reference use at all) — see § below for both.
- 45 pure sans-io unit tests pass (parsing, POC, ref-list construction, DPB eviction — no
  hardware needed); `cargo check`/`clippy` clean.
- **Paused, not hardware-verified — real GPU hang, not a soft error**: `windows` crate 0.62.2
  has the D3D12 decode API surface but **no DXVA picture-parameter structs** — hand-defined
  here, ground-truthed against Wine's `dxva.h` mirror. `push_packet` on a real IDR+P-frame
  H.264 bitstream triggers a genuine `DXGI_ERROR_DEVICE_HUNG` TDR (GPU driver reset) on this
  workspace's reference RTX 4090, not just a clean error return.
- Debugged with `ID3D12Debug`/`ID3D12InfoQueue` (same technique as ADR-0007's encode
  findings) and **fixed 3 real bugs**: readback buffer row-pitch alignment; NV12 chroma
  plane (`slot + num_slots`) never barriered to `VIDEO_DECODE_WRITE`/`READ` (luma-only) —
  strongest hang candidate; RBSP-vs-raw bit offset (later found itself wrong, see below).
  **Hang reproduced after all three** — debug layer clean, so the remaining bug is very
  likely opaque `DXVA_PicParams_H264` blob content (invisible to the debug layer). **6 real
  hardware TDRs** total finding this.
- **Static-only follow-ups (2026-08-05 + 2026-08-07, no hardware run)**: diffed against real
  DXVA producers (FFmpeg `dxva2_h264.c`, GStreamer `gstdxvah264decoder.cpp`) — fixed
  `wBitFields` bit 14 sourced from the wrong SPS field and a wrong `Reserved16Bits` default.
  Then fetched the **primary spec** itself (`docs/standards/registry.toml` id
  `dxva-h264-decoding`): it states `BitOffsetToSliceData` **is** the de-emulated RBSP bit
  offset (`parse_slice_header`'s own return value); the raw-buffer-position formula the spec
  also gives is the **accelerator's** internal translation, not the host's. This means the
  earlier "Bug 3" fix (raw-byte translation + `+8`) was backwards — replaced with a direct
  pass-through (+byte round-up for CABAC, a real requirement previously missing entirely).
  124 unit tests + clippy clean; re-run on real hardware same day — **hang reproduces
  identically, 8th TDR**, so this real, spec-confirmed fix is ruled out as the sole cause
  (kept regardless — correct per spec either way). No further hardware attempts without a
  new concrete lead (see ADR addendum: WMF-DXVA2 dump-and-diff or single-MB synthetic
  stream, both not yet tried). Struct field order/layout re-confirmed correct separately
  (Wine `dxva.h` mirror) — no layout bug.
- `GpuBufferHandle::DirectX12` has no `subresource` field — Zero-Copy output uses a local
  `DecodedOutput` type instead; flagged as a cross-crate follow-up. DPB = one fixed-size
  NV12 texture array; callers must release outstanding handles promptly or get a
  backpressure error (FFmpeg hwaccel surface-pool model), never a silent overwrite.
- ADR: [0002](../../../../crates/mediaway-decoder/adr/windows/0002-d3d12-native-video-decode.md)
  (2026-07-30 addendum: full hang-debugging trail). HEVC/AV1: parallel `hevc*.rs`/`av1*.rs`
  files, **implemented, sans-io-verified only** — [HEVC](windows-decode-d3d12-hevc.md)
  (ADR-0004), [AV1](windows-decode-d3d12-av1.md) (ADR-0005). Do not run any of the three
  D3D12 decode hardware tests.

## wgpu decode interop bridge (implemented — ADR-0003)

- `D3d11SharedDecodeBridge`: shares this crate's WMF DX11 Zero-Copy decode
  output (above) into a D3D12 resource `mediaway::wgpu`'s `WgpuDx12DecodeBridge`
  can wrap as a `wgpu::Texture`. `GpuCopy`, not Zero-Copy — see
  [zero-copy/gpu-interop](../zero-copy/gpu-interop.md).
- `src/d3d11_shared_decode_bridge.rs`, implemented 2026-07-31: `open` (caller D3D11 + caller
  D3D12 device, two-sided LUID check, shared NV12 texture), `copy_from_decoded` (cross-device
  `GetDevice()` guard, `CopySubresourceRegion` + bounded query/flush poll),
  `d3d12_resource_handle`. `open_same_adapter_or_skip` hardware-verified this session (not
  just a skip) — a real same-adapter D3D11+D3D12 device pair opened on the RTX 4090.
- Real finding beyond the ADR's own flagged risk list: `ID3D11DeviceContext::GetData`'s
  `windows`-crate `Result<()>` collapses S_OK and S_FALSE (both non-negative HRESULTs) to
  `Ok(())` — a poll loop must check the actual `BOOL` out-param, not just `.is_ok()`, or it
  can return before the GPU copy actually retires.
- `copy_from_decoded` itself is still unverified against real decode output — no working
  H.264 decode HW MFT available (same limitation `open_dx11_zero_copy_or_skip` hits above).
- ADR: [0003](../../../../crates/mediaway-decoder/adr/windows/0003-d3d11-shared-decode-bridge.md)
  — **Accepted**, implemented (2026-07-31 addendum has the signature-by-signature account).
