# mediaway-decoder-linux — roadmap

Linux VA-API decode backend.
Facade: [`mediaway-decoder`](../../mediaway-decoder/docs/roadmap.md).
Platform order: Windows → Web → **Linux** → other. Workspace index:
[`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Workspace member + docs / ADR surface
- [x] `cros-libva` dependency review + ADR ([0001](../adr/0001-vaapi-h264-cpu-out.md)) —
      shares the license/deps review already done for `mediaway-encoder-linux`'s identical
      dependency

### 1 — VA-API H.264 CPU-output decode (this change)

- [x] `Display::open()` DRM render-node auto-detect, honest `DecodeError` on failure
- [x] `vaCreateConfig`/`vaCreateContext`/`vaCreateSurfaces` session (Constrained
      Baseline/Main, `VAEntrypointVLD`)
- [x] Pure-Rust H.264 SPS/PPS/slice-header parsing scoped to this stage (baseline/main
      profile, progressive, `pic_order_cnt_type == 0`) — reuses
      [`mediaway_sw::h264`](../../mediaway-sw/docs/roadmap.md)'s NAL splitting / bit reader
- [x] `vaBeginPicture`/`vaRenderPicture`/`vaEndPicture`/`vaSyncSurface` via
      `cros_libva::Picture` typestate
- [x] CPU NV12 readback (`vaCreateImage`/`vaGetImage`/`vaMapBuffer`, documented cost)
- [x] **IDR pictures only**, single slice per picture, no DPB / reference picture
      management (see ADR-0001 § Scope)

**Zero hardware verification in the authoring session** — see ADR-0001's caveat section.
Compile-verified only (WSL2 Ubuntu 24.04, real `libva-dev` headers). The hardware-gated
test is expected to skip until run on a machine with a real VA-API driver + GPU.

### 2 — Inter-predicted pictures (deferred)

- [ ] P/B-slice decode, reference picture lists, DPB + MMCO / sliding-window marking
- [ ] Multi-slice-per-picture support
- [ ] `pic_order_cnt_type` 1/2
- [ ] Proven CI/`machine_id` cells once run on real VA-API hardware (promote 🆗 → ⚡ where earned)

### 3 — Zero-Copy (deferred)

- [ ] DMA-BUF surface export (`VideoOutputPreference::ZeroCopyGpu`,
      `vaExportSurfaceHandle` / `VADRMPRIMESurfaceDescriptor`)
- [ ] `GpuBufferHandle::Vulkan` interop path

### 4 — Multi-codec (deferred)

- [ ] HEVC / AV1 / VP9 via VA-API
- [ ] Vulkan Video decode as an alternative/complement (own ADR if pursued)

### 5 — Integration (deferred)

- [ ] Demuxer → decode smoke with `mediaway-container`
- [ ] Annex-B vs AVCC extradata policy documented + tested (this crate currently assumes
      Annex-B framing for both `push_packet` and `extra_data`, mirroring
      `mediaway-decoder-windows`'s identical open item)
