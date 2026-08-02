# ADR-0006: D3D12 shared → native D3D11 encode bridge

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder-windows`

## Context

Games / engines often render on **D3D12** (or wgpu DX12). WMF hardware encoders want a **native** `ID3D11Device` + `ID3D11Texture2D`. Wrapping via **D3D11On12** causes `MF_E_UNSUPPORTED_D3D_TYPE` on NVENC/MF (storm-chaser isolation).

## Decision

> Provide [`D3d12SharedEncodeBridge`](../src/d3d12_share.rs):
>
> 1. `CreateCommittedResource` with `D3D12_HEAP_FLAG_SHARED` + `ALLOW_RENDER_TARGET`
> 2. `CreateSharedHandle` → same-adapter native `D3D11CreateDevice` → `OpenSharedResource1`
> 3. Label path **GpuCopy** — caller copies into the shared D3D12 resource once per frame; not false Zero-Copy

Do **not** use D3D11On12 for MF encode input.

## Consequences

- Engine integrations get an honest GpuCopy contract + DX11 handles for existing ZC encode.
- Extra VRAM copy is expected and documented.

## References

- storm-chaser `shared_texture` / `win-interop-minimal` (reference only)
- ADR-0003 DX11 Zero-Copy · ADR-0005 BGRA input
