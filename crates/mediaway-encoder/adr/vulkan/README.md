# mediaway-encoder-vulkan — ADRs

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-vulkan-video-encode-ash-probe.md) | Vulkan Video via `vulkanalia` (migrated from `ash`); crate placement; probe/session/`VideoEncoder`/AV1 scope | Accepted — H.264/HEVC hardware-verified; AV1 implemented, driver-blocked |
| [0002](0002-vulkan-gop-rate-control.md) | Multi-frame GOP (P-frames only, no B-frames) + CBR rate control for H.264/HEVC | Accepted — H.264 + HEVC GOP hardware-verified; CBR stays H.264-only |

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../docs/adr/).
