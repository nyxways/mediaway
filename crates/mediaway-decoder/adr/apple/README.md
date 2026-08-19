# mediaway-decoder-apple — ADRs

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-videotoolbox-h264-cpu-out.md) | `VideoToolbox` `VTDecompressionSession` via `objc2`, H.264 CPU-output decode | Accepted |
| [0002](0002-videotoolbox-hevc-vp9-av1-decode.md) | `VideoToolbox` decode multicodec expansion — HEVC, VP9, AV1 | Accepted |
| [0003](0003-videotoolbox-metal-zero-copy-decode.md) | `VideoToolbox` decode Zero-Copy output (`GpuBufferHandle::Metal`) | Accepted |
| [0004](0004-audiotoolbox-aac-decode.md) | `AudioToolbox` `AudioConverter` AAC decode | Accepted |
| [0005](0005-audiotoolbox-opus-decode.md) | `AudioToolbox` `AudioConverter` Opus decode | Accepted |
| [0006](0006-videotoolbox-prores-decode.md) | `VideoToolbox` ProRes decode; ProRes RAW permanent non-support | Accepted |

Template: copy from [`mediaway-decoder/adr/template.md`](../template.md).

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../../docs/adr/).
