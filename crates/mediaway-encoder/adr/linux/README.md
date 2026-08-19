# mediaway-encoder-linux — ADRs

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-vaapi-cros-libva-h264-cpu-upload.md) | VA-API via `cros-libva`, H.264 CPU-upload encode | Accepted |
| [0002](0002-vaapi-h264-p-frame-gop.md) | VA-API H.264 single-forward-reference P-frame GOP (port from `vulkan/h264_gop.rs`) | Accepted (implemented) |
| [0003](0003-vaapi-av1-key-frame-and-inter-gop.md) | VA-API AV1 `KEY_FRAME` + single-forward-reference `INTER_FRAME` GOP encode | Proposed (blocked on a `cros-libva` packed-header gap) |
| [0004](0004-vaapi-vp9-key-frame-and-inter-gop.md) | VA-API VP9 `KEY_FRAME` + single-forward-reference `INTER_FRAME` GOP encode | Accepted (implemented, not blocked unlike AV1) |

Template: copy from [`mediaway-encoder/adr/template.md`](../../mediaway-encoder/adr/template.md).

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../docs/adr/).
