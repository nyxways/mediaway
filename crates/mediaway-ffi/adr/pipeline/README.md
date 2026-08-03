# mediaway-ffi ADRs

Crate-local C ABI surface decisions live here.

| ID | Title |
|----|-------|
| [0001](0001-auto-encode-c-abi.md) | Auto-encode → fragmented MP4 C ABI surface (first pass) |
| [0002](0002-gpu-frame-input-c-abi.md) | GPU frame input C ABI — `gpu_device` reachable from C |

Workspace C-FFI policy: [`docs/adr/0004-c-ffi.md`](../../../docs/adr/0004-c-ffi.md).
Workspace packaging: [`docs/adr/0003-crate-packaging.md`](../../../docs/adr/0003-crate-packaging.md).
Precedent: [`mediaway-container-ffi/adr/0001`](../../mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md) (opaque-handle / panic-safety / status-code patterns).
