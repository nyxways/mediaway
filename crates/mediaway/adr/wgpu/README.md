# mediaway-wgpu — ADRs

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-dx12-hal-gpucopy-bridge.md) | wgpu DX12 HAL escape hatch → existing WMF `GpuCopy` bridge | Accepted — hardware-verified |
| [0002](0002-decode-to-wgpu-texture-bridge.md) | Windows decode-output → `wgpu::Texture` import bridge (`WgpuDx12DecodeBridge`) | Proposed — design only |
| [0003](0003-dx12-native-zero-copy-bridge.md) | `WgpuDx12NativeBridge`: same-device D3D12 native encode input, true Zero-Copy | Proposed — design only |
| [0004](0004-wgpu-30-upgrade.md) | Upgrade `wgpu` from 26.x to 30.x | Accepted — hardware-verified |

Template: [`template.md`](template.md)

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../docs/adr/).
