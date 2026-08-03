# mediaway-wgpu

GPU framework adapter: bridges `wgpu::Device`/`wgpu::Texture` into Mediaway's
`GpuBufferHandle`/`GpuDeviceHandle`, so an app already rendering or computing
with [`wgpu`](https://github.com/gfx-rs/wgpu) can hand frames to Mediaway
encode without a forced CPU readback.

`wgpu` itself has no video-encode API. This crate does not add one — it is an
import/export bridge only ([`docs/spec/gpu-interop.md`](../../docs/spec/gpu-interop.md)):
it extracts the native GPU handle `wgpu`'s backend already holds (via `wgpu`'s
own HAL escape hatches) and hands it to an existing Mediaway platform encode
backend. The encode session itself stays the caller's own
`mediaway_encoder::VideoEncoder`.

Stage 1 (Windows): `WgpuDx12Bridge` — DX12 HAL extraction →
`mediaway-encoder-windows`'s `D3d12SharedEncodeBridge` → WMF hardware H.264
encode. Path class `GpuCopy` (one GPU→GPU copy + a CPU↔GPU sync stall per
frame, each direction), not Zero-Copy — `wgpu` has no D3D11 backend, and
Windows Media Foundation rejects `D3D11On12`-wrapped textures. Hardware-tested
end-to-end for encode (`wgpu::Texture` → H.264); the decode direction (WMF
DX11 decode output → `wgpu::Texture`, NV12) is hardware-tested for
construction only — the pixel-content round trip itself is unverified, since
no available test hardware has a working H.264 decode HW MFT yet.

See [`docs/roadmap.md`](docs/roadmap.md) for stage plan, and
[`adr/0001-dx12-hal-gpucopy-bridge.md`](adr/0001-dx12-hal-gpucopy-bridge.md)
for the design decision (including an honest "unverified this session" caveat
— no shell/build tool was available while this crate was authored).
