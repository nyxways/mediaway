# mediaway-device-ffi ADRs

Crate-local C ABI surface decisions live here.

| ID | Title |
|----|-------|
| [0001](0001-capture-c-abi.md) | Camera + microphone capture C ABI surface (first pass) |
| [0002](0002-callback-event-delivery.md) | Callback-based event delivery over the C ABI — `DeviceHotplug` as the first case |
| [0003](0003-gpu-handle-c-abi.md) | GPU device/buffer handles across the C ABI — unblocking Screen capture |
| [0004](0004-domain-feature-split.md) | Per-domain Cargo feature split (camera / desktop / audio / hotplug) |

Workspace C-FFI policy: [`docs/adr/0004-c-ffi.md`](../../../docs/adr/0004-c-ffi.md).
Workspace packaging: [`docs/adr/0003-crate-packaging.md`](../../../docs/adr/0003-crate-packaging.md).
