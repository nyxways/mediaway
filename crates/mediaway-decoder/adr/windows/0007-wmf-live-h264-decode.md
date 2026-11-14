# ADR-0007: Live H.264 GPU decode through the inbox DXVA decoder MFT

- **Status**: Accepted
- **Date**: 2026-11-15
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (`windows` module)

## Context

The receiving side of a live window share (`mediaway-encoder` `adr/nvenc/0002`) needs H.264
decoded on the GPU into D3D11 textures with: one frame out per packet in, recovery after
packet loss, a mid-stream size change (the shared window was resized), and a cheap CPU NV12
copy for a renderer that cannot import D3D11 textures yet.

[ADR-0001](0001-wmf-h264-dx11-out.md)'s Zero-Copy path enumerates `MFT_ENUM_FLAG_HARDWARE`
decoders. On the reference RTX 4090 (and an Intel UHD 770) that finds **nothing**: GPU vendors
register hardware *encoder* MFTs, not H.264 *decoder* MFTs. GPU decode of H.264 on Windows is
the inbox **synchronous** "Microsoft H264 Video Decoder MFT", which decodes with DXVA on the
device of a D3D11 device manager handed to it with `MFT_MESSAGE_SET_D3D_MANAGER`.

## Decision

> A dedicated `windows::WmfLiveH264Decoder` (+ `WmfLiveH264Config`) that uses the first
> synchronous, `MF_SA_D3D11_AWARE` H.264 decoder MFT accepting the caller's device manager.

- **Proof of GPU decode, not assumed:** open requires the output stream to provide its own
  samples (`MFT_OUTPUT_STREAM_PROVIDES_SAMPLES`, D3D11 mode) and every output sample must be
  a DXGI surface; otherwise `DecodeError::NoHardwareDecoder` (new variant), so a caller can
  fall back to software. The hardware test also checks the texture carries
  `D3D11_BIND_DECODER` (a DXVA output surface).
- **Latency:** `MF_LOW_LATENCY` on the transform. Measured: the frame for packet *n* is
  available right after `push_packet(n)` (no reorder delay) for a B-frame-free stream.
- **Loss:** `reset()` sends `MFT_MESSAGE_COMMAND_FLUSH`; at open and after a reset, packets are
  dropped until one with an IDR slice (NAL type 5, scanned only up to the first slice NAL).
  Feeding past a gap *without* a reset also keeps decoding (concealment) — the MFT does not
  stall.
- **Size change:** `MF_E_TRANSFORM_STREAM_CHANGE` → re-negotiate the NV12 output type; frame
  size = minimum display aperture (the SPS crop), so 1080p reports 1920 x 1080 from a
  1920 x 1088 surface.
- **Output:** `GpuBufferHandle::DirectX11 { texture, subresource }` — a slice of the MFT's
  texture array, valid until the next push / poll / reset / flush.
- **Readback:** `read_nv12(y, y_stride, uv, uv_stride)` copies the held frame through one
  staging texture (re-created only on a coded-size change) into caller buffers. Costly path
  (GPU→CPU, blocking `Map`), named and opt-in; no per-frame allocation.
- Annex-B input only (what the live encoder emits); no `extra_data` needed (SPS/PPS precede
  every IDR).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Fix ADR-0001's path to fall back to the inbox MFT | Changes a general path other callers rely on (async event pump, AVCC, timestamp repair); left for its own change. |
| D3D12 native decode (ADR-0002) | Paused on a real GPU hang; not hardware-verified. |
| Vulkan Video decode | Works (H.264 hardware-verified), but the consumer's capture / encode side is D3D11 and the renderer cannot import Vulkan images either; one D3D11 device on both ends is simpler. |
| Software decode + upload | Exactly what the user asked to avoid. |

## Consequences

### Positive

- Hardware decode verified end to end against the live NVENC encoder (frame count, size,
  luma PSNR, loss recovery, resize), with latency measured (crate roadmap, stage 9).

### Negative / Trade-offs

- Relies on the inbox MFT's DXVA support for the stream's profile / level / size; when absent
  the caller sees `NoHardwareDecoder` rather than a slower software decode.
- The MFT's surface pool is small; holding many frames is not possible (one is held).

## References

- [ADR-0001](0001-wmf-h264-dx11-out.md), [ADR-0006 workspace caveats](../../../../docs/adr/0006-caveats-and-clarity.md)
- `mediaway-encoder` `adr/nvenc/0002-live-h264-session.md`
- Microsoft: "H.264 Video Decoder" (Media Foundation) — DXVA, `CODECAPI_AVLowLatencyMode`
