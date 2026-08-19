# mediaway-encoder::apple — ADRs

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-videotoolbox-h264-cpu-upload.md) | `VideoToolbox` `VTCompressionSession` via `objc2`, H.264 CPU-upload encode | Accepted |
| [0002](0002-videotoolbox-hevc-encode.md) | `VideoToolbox` encode HEVC addition; VP9/AV1 permanent non-support | Accepted |
| [0003](0003-videotoolbox-metal-zero-copy-encode.md) | `VideoToolbox` encode Zero-Copy input (`GpuBufferHandle::Metal`) | Accepted |
| [0004](0004-audiotoolbox-aac-encode.md) | `AudioToolbox` `AudioConverter` AAC encode | Accepted |
| [0005](0005-audiotoolbox-opus-encode.md) | `AudioToolbox` `AudioConverter` Opus encode | Accepted |
| [0006](0006-videotoolbox-prores-encode.md) | `VideoToolbox` ProRes encode; ProRes RAW permanent non-support | Accepted |

Template: copy from [`mediaway-encoder/adr/template.md`](../template.md).

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../../docs/adr/).
