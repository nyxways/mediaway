# mediaway-encoder-nvenc — roadmap

NVIDIA NVENC direct vendor encode backend (`Backend::Nvenc` — see `mediaway-encoder` ADR-0004).
Facade: [`mediaway-encoder`](../../mediaway-encoder/docs/roadmap.md).
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Workspace member + docs / ADR surface
- [x] Bindings research + ADR ([0001](../adr/0001-nvenc-vendor-backend.md))

### 1 — H.264 CPU-upload (this change)

- [x] Private, internally-owned D3D11 hardware device (`NV_ENC_DEVICE_TYPE_DIRECTX`) —
      never exposed to callers; satisfies NVENC's session device requirement without the
      caller supplying a `GpuDeviceHandle`
- [x] `Session::open_dx` → `get_encode_preset_config_ex` (H.264, P3 preset, `HighQuality`
      tuning) → `init_encoder` (`enable_ptd = true`, automatic GOP/picture-type decisions)
- [x] CPU NV12 upload (`upload_cpu_nv12`, documented cost) into a private D3D11 staging
      texture, `CopyResource`'d into a GPU-resident texture registered once with NVENC
      (`register_resource_dx11`) — **not** NVENC's native `NvEncCreateInputBuffer`/lock
      host-memory path (see ADR-0001 addendum: real bug found there)
- [x] `encode_picture` / bitstream lock per pushed frame (synchronous, one NVENC bitstream
      buffer reused across frames — no deeper pipelining this stage, mirrors
      `mediaway-encoder-linux`'s VA-API backend)
- [x] Keyframe detection via Annex-B NAL scan (`contains_idr_nal`) — no separate extradata
      path; SPS/PPS ride inline before each IDR (mirrors the VA-API backend's convention)

**Hardware-verified 2026-07-29 on a real NVIDIA GeForce RTX 4090** (driver 32.0.15.9579) —
see [ADR-0001](../adr/0001-nvenc-vendor-backend.md) 2026-07-29 addendum. Not a
compile-only / simulated result: `dx11::video_tests::nvenc_open_and_encode_or_skip_without_hw`
opens a real session and encodes 5 synthetic NV12 frames, asserting real Annex-B output with
an inline SPS and a leading IDR slice.

### 2 — GOP / rate control tuning (deferred)

- [ ] Expose bitrate/preset/tuning knobs beyond the fixed P3/`HighQuality` default
- [ ] Proven multi-GPU / driver-version matrix, `machine_id` bench cells

### 3 — Zero-Copy (deferred — the genuinely large part per ADR-0001's size estimate)

- [ ] D3D11 Zero-Copy input (`VideoInputPreference::ZeroCopyGpu`, caller-supplied
      `GpuBufferHandle::DirectX11` texture via `register_resource_dx11` directly, no private
      upload textures)
- [ ] D3D12 Zero-Copy input (fence-based `NV_ENC_INPUT_RESOURCE_D3D12` — needs struct/fence
      support beyond what the `nvenc` crate's safe layer exposes today; see ADR-0001)

### 4 — Multi-codec (this change) + Linux (deferred)

- [x] HEVC CPU-upload — same D3D11 staging-texture path as H.264, codec selected via
      `NV_ENC_CODEC_HEVC_GUID`; keyframe detection via a 2-byte-NAL-header-aware Annex-B scan
      (`contains_hevc_idr_nal`, HEVC's NAL header differs from H.264's 1-byte header)
- [x] AV1 CPU-upload — same session shape, `NV_ENC_CODEC_AV1_GUID`; AV1 has no NAL/Annex-B
      framing at all (OBU-based), so keyframe detection instead scans for an
      `OBU_SEQUENCE_HEADER` via a small `leb128`-aware OBU walker
      (`contains_av1_sequence_header_obu` / `read_leb128`)
- [ ] Linux (`libnvidia-encode.so.1`, CUDA device type) — same crate, new `#[cfg]` arm
- [x] `auto` wiring: `mediaway-encoder-windows`'s `AutoVideoEncoder::open` opens NVENC
      directly for `BackendSelection::Explicit(Backend::Nvenc)`, and tries it (then
      `QuickSync`) ahead of `Os` CPU upload for `BackendSelection::AutoHardwareOnly` —
      never reached by plain `Auto` (see `mediaway-encoder` ADR-0004's 2026-07-31 addendum)

**Hardware-verified 2026-07-29 on the same reference RTX 4090** (driver 32.0.15.9579) — see
[ADR-0001](../adr/0001-nvenc-vendor-backend.md) 2026-07-29 (HEVC/AV1) addendum. Both codecs
worked through the `nvenc` crate's existing generic session/encoder API with no bindings
fork/extension needed (only the codec GUID differs at the call sites) — not a bindings gap,
not a driver/hardware gap. Real output confirmed: HEVC Annex-B with an inline VPS NAL (type
32) on the first packet; AV1 OBU stream with a genuine
temporal-delimiter → sequence-header → frame OBU shape on the first packet. NVENC still has
no VP9 encoder at all (VP9 is decode-only) — `validate()` rejects it; not a gap this axis will
ever close.
