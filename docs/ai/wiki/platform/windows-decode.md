# Windows decode (WMF)

- Crate: `mediaway-decoder-windows`
- Codecs: H.264 / HEVC / AV1 / VP9 HW decoder MFT → DX11 `GpuBufferHandle` (Nv12)
- CPU out: Stage 1 `Unsupported`
- README: OS · GPU / D3D11 decode 🆗 (open may skip without HW MFT)
- ADR: [0001](../../../crates/mediaway-decoder-windows/adr/0001-wmf-h264-dx11-out.md)
- Benches: [`docs/benchmarks.md`](../../../crates/mediaway-decoder-windows/docs/benchmarks.md)
  (Criterion, `sw_wmf_h264_cpu` vs `zc_wmf_h264_dx11`). Same `ad-hoc` box as the
  encode page also had **no** working Media Foundation decode HW MFT on either GPU
  (`DecodeError::Unsupported` on both NVIDIA RTX 4090 and Intel UHD 770) even though
  `ffmpeg h264_cuvid`/NVDEC decodes fine outside MF — `zc_wmf_h264_dx11` came back
  N/A there, consistent with `open_dx11_zero_copy_or_skip`'s own graceful skip.

## D3D12 native decode (H.264 implemented, unregistered — ADR-0002)

- A **second**, independent decode path (`ID3D12VideoDecoder`/`ID3D12VideoDecoderHeap`/
  `DecodeFrame1`) alongside WMF, self-contained and **not** wired into `WindowsVideoDecoder`
  yet (`mod d3d12_video_decode;`, non-`pub`, same trick ADR-0007's encode module used). Unlike
  WMF, the app parses the bitstream itself (DXVA-shaped picture-parameter buffers per codec).
- Scope (broader than the encode-side D3D12 precedent and the Linux VA-API sibling): **general
  GOP** (P/B refs, real DPB) across H.264 + HEVC + AV1 from the start, not IDR-only. **H.264
  implemented this round** (real SPS/PPS/slice/POC-types-0/1/2/ref-list/sliding-window-DPB
  logic, `src/d3d12_video_decode/`); HEVC/AV1 are follow-up addenda.
- 45 pure sans-io unit tests pass (parsing, POC, ref-list construction, DPB eviction — no
  hardware needed); `cargo check`/`clippy` clean.
- **Paused, not hardware-verified — real GPU hang, not a soft error**: `windows` crate 0.62.2
  has the D3D12 decode API surface but **no DXVA picture-parameter structs** — hand-defined
  here, ground-truthed against Wine's `dxva.h` mirror. `push_packet` on a real IDR+P-frame
  H.264 bitstream triggers a genuine `DXGI_ERROR_DEVICE_HUNG` TDR (GPU driver reset) on this
  workspace's reference RTX 4090, not just a clean error return.
- Debugged with `ID3D12Debug`/`ID3D12InfoQueue` (same technique as ADR-0007's encode
  findings) and **fixed 3 real bugs**: readback buffer sized tightly-packed instead of
  row-pitch-aligned; NV12's chroma plane (`slot + num_slots`) never barriered to
  `VIDEO_DECODE_WRITE`/`READ` (luma-only) — the strongest hang candidate; RBSP-vs-raw bit
  offset never translated, corrupting `BitOffsetToSliceData` on any stream with escape bytes
  before `slice_data()`. **The hang still reproduces after all three fixes** — the debug
  layer now reports zero validation messages, meaning the remaining bug is very likely inside
  the opaque `DXVA_PicParams_H264` blob content itself (invisible to the debug layer).
  **6 real hardware TDRs** were triggered finding this — by explicit project-owner decision,
  further hardware iteration here is **paused** rather than continuing to reset the machine's
  GPU on speculation. Next real step if resumed: diff this backend's picture-param fill
  byte-for-byte against a working WMF/DXVA2 reference on the same stream, or Nsight Aftermath.
- `GpuBufferHandle::DirectX12` has no `subresource` field — Zero-Copy output currently uses a
  local `DecodedOutput` type instead of forcing it through the shared facade type; flagged as
  a cross-crate follow-up.
- DPB = one fixed-size NV12 texture array; Zero-Copy output points at DPB subresources
  directly; callers must release Zero-Copy frames promptly or get a backpressure error
  (FFmpeg hwaccel surface-pool model), never a silent overwrite.
- ADR: [0002](../../../crates/mediaway-decoder-windows/adr/0002-d3d12-native-video-decode.md)
  (2026-07-30 addendum has the full hang-debugging trail).

## wgpu decode interop bridge (implemented — ADR-0003)

- `D3d11SharedDecodeBridge`: shares this crate's WMF DX11 Zero-Copy decode
  output (above) into a D3D12 resource `mediaway-wgpu`'s `WgpuDx12DecodeBridge`
  can wrap as a `wgpu::Texture`. `GpuCopy`, not Zero-Copy — see
  [zero-copy/gpu-interop](../zero-copy/gpu-interop.md).
- `src/d3d11_shared_decode_bridge.rs`, implemented 2026-07-31: `open` (caller
  D3D11 + caller D3D12 device, two-sided LUID check, shared NV12 texture),
  `copy_from_decoded` (cross-device `GetDevice()` guard, `CopySubresourceRegion`
  + bounded query/flush poll), `d3d12_resource_handle`.
  `open_same_adapter_or_skip` hardware-verified this session (not just a
  skip) — a real same-adapter D3D11+D3D12 device pair opened successfully on
  the RTX 4090.
- Real finding beyond the ADR's own flagged risk list: `ID3D11DeviceContext::
  GetData`'s `windows`-crate `Result<()>` collapses S_OK and S_FALSE (both
  non-negative HRESULTs) to `Ok(())` — a poll loop must check the actual
  `BOOL` out-param, not just `.is_ok()`, or it can return before the GPU copy
  actually retires.
- `copy_from_decoded` itself is still unverified against real decode output —
  there is still no working H.264 decode HW MFT available (same limitation
  `open_dx11_zero_copy_or_skip` above already hits).
- ADR: [0003](../../../crates/mediaway-decoder-windows/adr/0003-d3d11-shared-decode-bridge.md)
  — **Accepted**, implemented (2026-07-31 addendum has the signature-by-signature account).
