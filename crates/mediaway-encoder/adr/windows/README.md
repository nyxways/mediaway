# mediaway-encoder-windows — ADRs

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-wmf-h264-surface.md) | WMF H.264 / AAC encode surface | Accepted |
| [0002](0002-windows-crate.md) | `windows` crate for Media Foundation | Accepted |
| [0003](0003-dx11-zero-copy.md) | DX11 Zero-Copy via DXGI device manager | Accepted |
| [0004](0004-multi-codec-wmf.md) | Multi-codec WMF (HEVC / AV1 / VP9) | Accepted |
| [0005](0005-bgra-dxgi-input.md) | BGRA (ARGB32) Zero-Copy encode input | Accepted |
| [0006](0006-d3d12-shared-to-d3d11.md) | D3D12 shared → native D3D11 (GpuCopy) | Accepted |
| [0007](0007-d3d12-native-video-encode.md) | D3D12 native video encode (H.264, CPU-upload) | Accepted |
| [0008](0008-d3d12-native-encode-gpu-input.md) | D3D12 native encode: GPU-texture (Zero-Copy) input | Proposed |
| [0009](0009-native-capture-shared-handle-zero-copy.md) | Native (non-wgpu) capture-to-encode shared-handle Zero-Copy | Proposed |
| [0010](0010-wmf-av1-encode-config-record-and-mft-probe.md) | WMF AV1 encode: `av1C` config-record correctness + real encoder-MFT probe (dispatch already done, ADR-0004) | Accepted |
| [0011](0011-shared-encode-bridge-external-resource.md) | `D3d12SharedEncodeBridge::open_with_resource` — caller-owned shared resource | Accepted — hardware-verified |

Template: copy from [`mediaway-encoder/adr/template.md`](../../mediaway-encoder/adr/template.md).

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../docs/adr/).
