# mediaway-decoder-linux — ADRs

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-vaapi-h264-cpu-out.md) | VA-API via `cros-libva`, H.264 CPU-output decode | Accepted |
| [0002](0002-vaapi-h264-p-slice-dpb.md) | VA-API H.264 single-forward-reference P-slice decode (DPB port from `vulkan/dpb.rs`) | Accepted |
| [0003](0003-vaapi-hevc-p-slice-dpb.md) | VA-API HEVC single-forward-reference P-slice decode | Accepted (implemented) |
| [0004](0004-vaapi-vp9-key-frame-and-inter-decode.md) | VA-API VP9 `KEY_FRAME` + general `INTER_FRAME` decode | Accepted (implemented) |
| [0005](0005-vaapi-av1-key-frame-decode.md) | VA-API AV1 `KEY_FRAME`-only decode | Accepted (implemented) |
| [0006](0006-vaapi-dmabuf-zero-copy-output.md) | VA-API DMA-BUF Zero-Copy decode output — new `GpuBufferHandle::DmaBuf` | Accepted (implemented) |

Template: copy from [`mediaway-decoder/adr/template.md`](../../mediaway-decoder/adr/template.md).

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../docs/adr/).
