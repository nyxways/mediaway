# Zero-Copy marks (README ⚡)

Canonical human table: root [`README.md`](../../../../README.md#codec-support).

| Claim | Means |
|-------|--------|
| **⚡** | No payload `memcpy` on the hot handoff |
| GPU ⚡ | `GpuBufferHandle` / DXGI / WebGPU / … stays on device |
| CPU ⚡ | Shared/borrowed PCM or `Bytes` share (audio, mapped buffers) |
| Not ⚡ | New `Vec` copy, readback, upload, cross-API `copy_*` |

Bench labels: `zc` covers **both** GPU and shared-CPU Zero-Copy ([benchmarking](../meta/benchmarking.md)).  
GPU framework adapters: [gpu-interop](gpu-interop.md). Handles: [handles](handles.md).
