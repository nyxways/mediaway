# ADR-0005: BGRA (ARGB32) Zero-Copy encode input

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder-windows`

## Context

DXGI Desktop Duplication and WGC produce **BGRA** textures. Mediaway encode previously required **NV12**, forcing a named GpuCopy or silent convert. Proven Windows recorders configure HW MFTs with `MFVideoFormat_ARGB32` first, falling back to NV12.

## Decision

> For [`VideoInputPreference::ZeroCopyGpu`](../../mediaway-encoder) with [`PixelFormat::Bgra8`](../../mediaway-common):
>
> 1. Enumerate HW encoders with **no input type filter** (output codec only).
> 2. `SetInputType` attempts **ARGB32 then NV12**.
> 3. Submit caller `GpuBufferHandle::DirectX11` via `MFCreateDXGISurfaceBuffer` — still Zero-Copy when ARGB32 is accepted.
>
> CPU upload remains NV12-only (`upload_cpu_nv12`).

## Consequences

- Screen capture → encode can stay Zero-Copy without VideoProcessor when the HW MFT accepts ARGB32.
- If only NV12 is accepted, callers must supply NV12 or a future named GpuCopy converter.

## References

- ADR-0003 · live-recorder `encode/mft_video.rs` `configure_input_type` (reference only)
