# GPU framework interop

Canonical: [`docs/spec/gpu-interop.md`](../../../spec/gpu-interop.md) · ADR-0005.

- Rust: `mediaway-wgpu` (2026-07-29) — real, hardware-tested `wgpu` HAL interop.
  Windows: `WgpuDx12Bridge` reaches past `wgpu`'s own API via `Device::as_hal`/
  `create_texture_from_hal` (`unsafe` escape hatches, not a stabilized public
  contract) to recover the native `ID3D12Device`/`ID3D12Resource` `wgpu`'s
  DX12 backend holds, then bridges into `mediaway-encoder-windows`'s existing
  `D3d12SharedEncodeBridge` (D3D12 shared heap → native D3D11 → WMF). **Path
  class: `GpuCopy`, not Zero-Copy** — `wgpu` has no D3D11 backend and WMF
  rejects `D3D11On12`, so one GPU→GPU copy + a CPU↔GPU sync stall per frame is
  the real cost. `cargo test -p mediaway-wgpu` passes end-to-end on an
  RTX 4090 (currently via the graceful-skip path —
  same pre-existing HW/driver limitation the underlying WMF bridge test
  already hits on its own). See `mediaway-wgpu/adr/0001`.
- Real bug caught only by compiling: `wgpu-hal` 26.x pins its own `windows`
  crate dependency to 0.58, incompatible as a Rust *type* with this
  workspace's ordinary `windows = "0.62"` even though both wrap the same COM
  interface — bridge only raw pointer bits (`NativeHandle`) across that
  version boundary, never a typed COM object.
- `mediaway-encoder-vulkan` (2026-07-29) — real, hardware-verified
  `VK_KHR_video_queue` capability probe (`ash`). Confirmed:
  NVIDIA RTX 4090 advertises H.264+H.265 encode on queue family 4; Intel UHD
  770's Windows Vulkan driver advertises none. Real bug found+fixed:
  `VK_KHR_video_queue` is a *device* extension, not instance — an early draft
  tried enabling it in `InstanceCreateInfo` and every driver correctly
  rejected it. **Encode is now real and reusable, not just a probe**:
  `VulkanVideoEncoder` implements `mediaway_encoder::VideoEncoder`
  (H.264 + HEVC, CPU-upload, all-intra), hardware-verified on the same RTX
  4090 — session/images/buffers/command pool persist across `push_frame`
  calls, and a `VK_QUERY_TYPE_VIDEO_ENCODE_FEEDBACK_KHR` query gives
  byte-exact packets (not the whole zero-padded destination buffer). **Not**
  yet a Zero-Copy path — CPU-upload only, external-memory GPU import still
  deferred. Two more real bugs found only on hardware: a `Drop` field-order
  bug that destroyed the Vulkan instance before its device (violates a real
  Vulkan ordering rule, crashed with `STATUS_ACCESS_VIOLATION`), and a
  capabilities query that chained H.264's struct while querying an HEVC
  profile (driver rejected the call outright). Also: HEVC's
  `picture_access_granularity` is `32x32` on this driver, not `16x16` like
  H.264 — the two must never be assumed equal. See
  `mediaway-encoder-vulkan/adr/0001`.
- Reverse direction (decode → `wgpu::Texture`), Windows: **implemented on
  both sides**, `mediaway-wgpu` ADR-0002 (`WgpuDx12DecodeBridge`,
  **Accepted**, 2026-07-31) plus its companion `D3d11SharedDecodeBridge` in
  `mediaway-decoder-windows` (crate-local ADR-0003, **Accepted**,
  2026-07-31). `D3d11SharedDecodeBridge` creates the shared NV12 texture
  directly on the caller's own D3D11 decode device (no re-create, unlike the
  encode-direction bridge), validates same-adapter via a real **two-sided**
  LUID comparison (both devices are caller-owned here, unlike the encode
  bridge which only creates one side), and adds a same-device `GetDevice()`
  guard on the source decode texture beyond what the wgpu-side ADR's
  contract literally asked for. `WgpuDx12DecodeBridge` (`mediaway-wgpu`,
  `src/dx12_decode.rs`) mirrors `WgpuDx12Bridge`'s `as_hal` +
  `create_texture_from_hal` technique in reverse, wraps the bridge's shared
  D3D12 resource **once** at `new()` time (single-buffered, reused
  destination texture — same footgun class as `WgpuDx12Bridge::dest`, sharper
  here since decode output is often sampled across multiple render frames).
  Same `GpuCopy` cost class as the encode direction (D3D11→D3D11 copy +
  CPU↔GPU query-poll stall, no fence hand-off in v1).
  `wgpu::TextureFormat::NV12` confirmed to exist exactly as designed in the
  pinned `wgpu-types 26.0.0` source this workspace resolves to, gated by
  `Features::TEXTURE_FORMAT_NV12` (native-only, DX12 + Vulkan) — required for
  a caller's later `create_view` on the returned texture, not for
  `WgpuDx12DecodeBridge::new`'s own `create_texture_from_hal` wrap (which
  bypasses that validation, same as the encode bridge's BGRA8 wrap).
  Hardware-verified this session beyond a graceful skip:
  `WgpuDx12DecodeBridge::new` genuinely succeeded end to end (HAL extraction
  → `D3d11SharedDecodeBridge::open` → `create_texture_from_hal`) against a
  real wgpu DX12 device + same-adapter `ID3D11Device` pair
  (`tests/dx12_decode_smoke.rs`) — also confirms `D3D11_BIND_SHADER_RESOURCE`-only
  is sufficient for `wgpu` to accept the shared resource (ADR-0003's own
  residual risk #5, resolved positively). Still unverified: a full decode →
  `copy_from_decoded` → `import_decoded_texture` round trip with real pixel
  content, and per-plane (`TextureAspect::Plane0`/`Plane1`) view/sampling —
  both blocked on the lack of a working H.264 decode HW MFT in testing so
  far. See `mediaway-wgpu/adr/0002`,
  `mediaway-decoder-windows/adr/0003`.
- Browser: WebGPU; native C/C++: Dawn has **zero** video-encode code in its
  entire repo (confirmed via a direct code search) — no HAL-style escape
  hatch exists there the way `wgpu` has one; encode lives entirely in the
  separate WebCodecs spec.
- Other languages: same OS/WebGPU **handles** via FFI — not a fake wgpu object
- Cores do not depend on wgpu
- Encode intake (compatible / adapt / bridge): [backend-preference](../encode/backend-preference.md) · ADR-0004
- Web: no `GPUTexture` → `VideoFrame` ctor — canvas-source path, honesty label: [web-gpu-frame](../encode/web-gpu-frame.md)
