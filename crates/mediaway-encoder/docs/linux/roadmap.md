# mediaway-encoder-linux — roadmap

Linux VA-API encode backend.
Facade: [`mediaway-encoder`](../../mediaway-encoder/docs/roadmap.md).
Platform order: Windows → Web → **Linux** → other. Workspace index:
[`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Workspace member + docs / ADR surface
- [x] `cros-libva` dependency review + ADR ([0001](../adr/0001-vaapi-cros-libva-h264-cpu-upload.md))

### 1 — VA-API H.264 CPU-upload (this change)

- [x] `Display::open()` DRM render-node auto-detect, honest `EncodeError` on failure
- [x] `vaCreateConfig`/`vaCreateContext`/`vaCreateSurfaces` session (Constrained Baseline,
      `VAEntrypointEncSlice`, `VA_RC_CQP`)
- [x] CPU NV12 upload (`upload_cpu_nv12`, documented cost — `Image::create_from` + `vaPutImage`)
- [x] `vaRenderPicture`/`vaEndPicture`/`vaSyncSurface` via `cros_libva::Picture` typestate
- [x] Encoded bitstream readback (`EncCodedBuffer` + `MappedCodedBuffer`, `vaMapBuffer`)
- [x] Every frame independent IDR (no GOP / P-frames / reference management this stage)

**Zero hardware verification in the authoring session** — see ADR-0001's caveat section.
Compile-verified only (WSL2 Ubuntu 24.04, real `libva-dev` 1.20.0 headers). The hardware-gated
test is expected to skip until run on a machine with a real VA-API driver + GPU.

### 2 — GOP / rate control (deferred)

- [ ] P-frame reference picture management, `intra_period`/`ip_period` > 1
- [ ] VBR/CBR rate control (`VAEncMiscParameterRateControl`/`FrameRate`)
- [ ] Proven CI/`machine_id` cells once run on real VA-API hardware (promote 🆗 → ⚡ where earned)

### 3 — Zero-Copy (implemented, WSL2 + Windows compile/clippy/test-verified)

- [x] DMA-BUF surface import (`VideoInputPreference::ZeroCopyGpu`,
      `VASurfaceAttribExternalBuffers` / `VADRMPRIMESurfaceDescriptor`) —
      [ADR-0006](../../adr/linux/0006-vaapi-dmabuf-zero-copy-input.md); **not**
      `GpuBufferHandle::Vulkan` (corrected — reuses the decoder's `GpuBufferHandle::DmaBuf`
      variant instead)
- [ ] Real VA-API hardware verification (decode-export → encode-import round trip) — same
      standing gap as every other VA-API path in this crate

### 4 — Multi-codec (deferred)

- [ ] HEVC / AV1 / VP9 via VA-API
- [ ] Vulkan Video encode as an alternative/complement (own ADR if pursued)
