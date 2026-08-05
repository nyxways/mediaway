# mediaway-ffi ADRs

Crate-local C ABI surface decisions live here.

| ID | Title |
|----|-------|
| [0001](0001-auto-encode-c-abi.md) | Auto-encode → fragmented MP4 C ABI surface (first pass) |
| [0002](0002-gpu-frame-input-c-abi.md) | GPU frame input C ABI — `gpu_device` reachable from C |
| [0003](0003-auto-audio-encode-c-abi.md) | Auto audio encode C ABI — `AudioEncoder` reachable from C |
| [0004](0004-auto-decode-c-abi.md) | Auto video decode C ABI — `AutoDecoder` reachable from C |
| [0005](0005-capture-encode-bridge-c-abi.md) | Capture-to-encode bridge C ABI |
| [0006](0006-audio-decode-c-abi.md) | Opus audio decode C ABI + Opus wired into audio encode C ABI |

Workspace C-FFI policy: [`docs/adr/0004-c-ffi.md`](../../../docs/adr/0004-c-ffi.md).
Workspace packaging: [`docs/adr/0003-crate-packaging.md`](../../../docs/adr/0003-crate-packaging.md).
Precedent: [`mediaway-container-ffi/adr/0001`](../../mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md) (opaque-handle / panic-safety / status-code patterns).
