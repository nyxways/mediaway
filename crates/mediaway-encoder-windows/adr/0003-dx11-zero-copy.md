# ADR-0003: DX11 Zero-Copy input via DXGI device manager

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder-windows`

## Context

CPU NV12 uses `upload_cpu_nv12` (copy). Product Zero-Copy needs `GpuBufferHandle::DirectX11` into the encoder without readback. The inbox sync MFT does not accept DXGI surfaces; hardware encoder MFTs do, via `IMFDXGIDeviceManager` + `MFCreateDXGISurfaceBuffer`.

## Decision

> When [`VideoInputPreference::ZeroCopyGpu`](../../mediaway-encoder/src/video.rs):
>
> 1. Require `VideoEncoderConfig::d3d11_device` (opaque `ID3D11Device*`).
> 2. Enumerate a **hardware** H.264 encoder MFT (`MFTEnumEx` + `MFT_ENUM_FLAG_HARDWARE`).
> 3. Bind `MFCreateDXGIDeviceManager` / `ResetDevice` / `MFT_MESSAGE_SET_D3D_MANAGER`.
> 4. Unlock async MFTs (`MF_TRANSFORM_ASYNC_UNLOCK`) and drive `IMFMediaEventGenerator` credits.
> 5. Submit frames with `MFCreateDXGISurfaceBuffer` — **never** silent GPU→CPU readback.
>
> [`VideoInputPreference::CpuUploadOk`](../../mediaway-encoder/src/video.rs) keeps the sync inbox MFT + `upload_cpu_nv12`.

Mismatch (CPU frames on DX11 session or DX11 frames on CPU session) → `EncodeError::Unsupported`.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Readback DX11 → NV12 into sync MFT | Silent/slow; violates ADR-0006 |
| Software MFT + DXGI | Not supported by inbox encoder |
| Device inferred only from first texture | `SET_D3D_MANAGER` must precede streaming |

## Consequences

### Positive

- Honest Zero-Copy path; CPU path unchanged

### Negative / Trade-offs

- Needs a HW encoder MFT on the machine; otherwise Zero-Copy open fails
- Async event pump adds complexity vs sync CPU path

## References

- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md)
- Caveat catalog: CPU upload row remains; DX11 path is Zero-Copy
