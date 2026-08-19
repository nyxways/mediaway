# mediaway-decoder-vulkan — ADRs

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-vulkan-video-decode.md) | Vulkan Video decode via `vulkanalia`; crate placement; H.264/HEVC/AV1 + general-GOP scope | Accepted — H.264 general-GOP + HEVC IDR-only hardware-verified; AV1 design superseded in scope by 0002 |
| [0002](0002-av1-decode-keyframe-first.md) | AV1 decode (`VK_KHR_video_decode_av1`) — KEY_FRAME-only first increment | Accepted — implemented and hardware-verified (first attempt, no driver-maturity wall) |

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../docs/adr/).
