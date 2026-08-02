# ADR-0001: DXGI Desktop Duplication for screen Zero-Copy

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-windows`

## Context

Screen capture for encode pipelines needs a DXGI path that hands `ID3D11Texture2D` to downstream HW encoders without silent GPU→CPU readback. Desktop Duplication API (DDA) is the standard Windows mechanism for full-display capture with D3D11 textures.

## Decision

> For [`CaptureSource::Screen`](../../mediaway-device/src/video.rs) with [`CaptureOutputPreference::ZeroCopyGpu`](../../mediaway-device/src/video.rs):
>
> 1. Require caller-owned `VideoCaptureConfig::d3d11_device` (`ID3D11Device*` on the adapter that owns the output).
> 2. `IDXGIOutput1::DuplicateOutput` per `output_index`.
> 3. `AcquireNextFrame` → wrap texture as [`GpuBufferHandle::DirectX11`](../../mediaway-common/src/gpu.rs) with [`PixelFormat::Bgra8`](../../mediaway-common/src/formats.rs) (honest desktop format — not NV12).
> 4. Caller must [`release_frame`](../../mediaway-device/src/video.rs) (`IDXGIOutputDuplication::ReleaseFrame`) before the next acquire.
> 5. **No** `MapDesktopSurface` / silent readback on the Zero-Copy path.
>
> Camera sources remain `CaptureError::Unsupported` until a later ADR. Window capture is [`ADR-0004`](0004-wgc-window-capture.md) (`WindowsWindowCapture`).

`DXGI_ERROR_WAIT_TIMEOUT` → `Ok(None)`. `DXGI_ERROR_ACCESS_LOST` / `ACCESS_DENIED` → `CaptureError::AccessDenied`.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `BitBlt` / GDI capture | CPU copy; not Zero-Copy |
| WinRT `GraphicsCaptureItem` only | Different lifetime/API; defer for window capture ADR |
| Map DDA surface to CPU by default | Violates ADR-0006; hides readback cost |

## Consequences

### Positive

- Honest Zero-Copy BGRA textures into `mediaway-encoder-windows` DX11 path (format conversion remains encoder’s job)
- Explicit `release_frame` documents duplication lifetime

### Negative / Trade-offs

- DDA requires compatible adapter/device; secure desktop / session changes need session recreate
- BGRA desktop surfaces ≠ encoder NV12 — compositor/convert may be required upstream of encode

## References

- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md)
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md)
- Encoder DX11 ADR: [`mediaway-encoder-windows/adr/0003-dx11-zero-copy.md`](../../mediaway-encoder-windows/adr/0003-dx11-zero-copy.md)
