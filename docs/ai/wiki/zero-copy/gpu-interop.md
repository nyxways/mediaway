# GPU framework interop

Canonical: [`docs/spec/gpu-interop.md`](../../../spec/gpu-interop.md) · ADR-0005.

- Rust: `mediaway::wgpu` (2026-07-29) — real, hardware-tested `wgpu` HAL interop.
  Windows: `WgpuDx12Bridge` reaches past `wgpu`'s own API via `Device::as_hal`/
  `create_texture_from_hal` (`unsafe` escape hatches, not a stabilized public
  contract) to recover the native `ID3D12Device`/`ID3D12Resource` `wgpu`'s
  DX12 backend holds, then bridges into `mediaway-encoder::windows`'s existing
  `D3d12SharedEncodeBridge` (D3D12 shared heap → native D3D11 → WMF).
  `copy_frame`: **`GpuCopy`**, one GPU→GPU copy + CPU↔GPU sync stall/frame
  (`wgpu` has no D3D11 backend, WMF rejects `D3D11On12`). **`render_target`
  + `handle` (2026-08-20): genuine `ZeroCopy`** — render directly into the
  bridge's own shared texture (now `RENDER_ATTACHMENT`-capable) instead of
  copying a separate one in; only cost left is `handle`'s untargeted
  `poll(Wait)` stall. **`from_external_shared_resource`**: import a
  caller-owned already-shared D3D12 resource instead of allocating one
  (`mediaway-encoder` ADR-0011's `open_with_resource`). All hardware-verified
  on the reference RTX 4090. See
  `crates/mediaway/adr/wgpu/0001-dx12-hal-gpucopy-bridge.md`,
  `0005-render-target-and-external-shared-resource.md`.
- `wgpu` bumped 26.x → 30.x (2026-08-18), real-hardware re-verified — 6 breaking API changes
  fixed, plus resolved the 26.x-era `windows`-crate 0.58/0.62 straddle bug as a side effect. See
  `crates/mediaway/adr/wgpu/0004-wgpu-30-upgrade.md`.
- `mediaway-encoder::vulkan` (2026-07-29) — real, hardware-verified
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
  `crates/mediaway-encoder/adr/vulkan/0001-vulkan-video-encode-ash-probe.md`.
- Reverse direction (decode → `wgpu::Texture`), Windows: **implemented on
  both sides**, `mediaway::wgpu` ADR-0002 (`WgpuDx12DecodeBridge`,
  **Accepted**, 2026-07-31) plus its companion `D3d11SharedDecodeBridge` in
  `mediaway-decoder::windows` (crate-local ADR-0003, **Accepted**,
  2026-07-31). `D3d11SharedDecodeBridge` creates the shared NV12 texture
  directly on the caller's own D3D11 decode device (no re-create, unlike the
  encode-direction bridge), validates same-adapter via a real **two-sided**
  LUID comparison (both devices are caller-owned here, unlike the encode
  bridge which only creates one side), and adds a same-device `GetDevice()`
  guard on the source decode texture beyond what the wgpu-side ADR's
  contract literally asked for. `WgpuDx12DecodeBridge` (`mediaway::wgpu`,
  `src/wgpu/dx12_decode.rs`) mirrors `WgpuDx12Bridge`'s `as_hal` +
  `create_texture_from_hal` technique in reverse, wraps the bridge's shared
  D3D12 resource **once** at `new()` time (single-buffered, reused
  destination texture — same footgun class as `WgpuDx12Bridge::dest`, sharper
  here since decode output is often sampled across multiple render frames).
  Same `GpuCopy` cost class as the encode direction (D3D11→D3D11 copy +
  CPU↔GPU query-poll stall, no fence hand-off in v1).
  `wgpu::TextureFormat::NV12` confirmed to exist exactly as designed in the
  pinned `wgpu-types` source this workspace resolves to, gated by
  `Features::TEXTURE_FORMAT_NV12` (native-only, DX12 + Vulkan) — required for
  a caller's later `create_view` on the returned texture, not for
  `WgpuDx12DecodeBridge::new`'s own `create_texture_from_hal` wrap (which
  bypasses that validation, same as the encode bridge's BGRA8 wrap).
  Hardware-verified: `WgpuDx12DecodeBridge::new` succeeds end to end (HAL
  extraction → `D3d11SharedDecodeBridge::open` → `create_texture_from_hal`)
  against a real wgpu DX12 device + same-adapter `ID3D11Device` pair
  (`tests/wgpu/dx12_decode_smoke.rs`) — confirms `D3D11_BIND_SHADER_RESOURCE`-only
  is sufficient for `wgpu` to accept the shared resource (ADR-0003 residual
  risk #5, resolved positively). **Full pixel round trip now hardware-verified
  too** (2026-08-05, `tests/wgpu/dx12_decode_pixel_roundtrip.rs`): a stand-in
  D3D11 NV12 texture (real decoder output has the same shape the bridge cares
  about, not who produced it) written with a known pattern →
  `import_decoded_texture` → byte-exact readback, 6144/6144 bytes matching on
  an RTX 4090. Real bug found (26.0.6-era, not re-checked against 30.x): `wgpu-hal`'s DX12
  backend (`calc_subresource_for_copy`) had no match arm for `FormatAspects::PLANE_0`/`PLANE_1`
  — `unreachable!()` panics on **any** `copy_texture_to_buffer`/`copy_texture_to_texture` (even
  `TextureAspect::All`) against a multi-planar (NV12) texture. Worked around in the test via the
  reverse hop (`ID3D12Device::CreateSharedHandle` → `ID3D11Device1::OpenSharedResource1` → D3D11
  staging `Map`) instead of `wgpu`'s own copy API — `create_view`/per-plane sampling remains
  genuinely unverified. Also found: both `tests/wgpu/*.rs` files had **never actually
  compiled** — not wired into `Cargo.toml` (`[[test]]` needed) and referencing a stale
  `mediaway_wgpu` extern crate from before the ADR-0021 crate merge — both fixed. See
  `crates/mediaway/adr/wgpu/0002-decode-to-wgpu-texture-bridge.md`,
  `crates/mediaway-decoder/adr/windows/0003-d3d11-shared-decode-bridge.md`.
- Browser: WebGPU; native C/C++: Dawn has **zero** video-encode code in its
  entire repo (confirmed via a direct code search) — no HAL-style escape
  hatch exists there the way `wgpu` has one; encode lives entirely in the
  separate WebCodecs spec.
- Linux VA-API DMA-BUF Zero-Copy: **implemented, no real hardware verification** —
  `adr/linux/0003-*.md`; new `GpuBufferHandle::DmaBuf(Box<..>)` (`Copy`-removal: zero call sites); no `mediaway::wgpu` consumer yet.
- Other languages: same OS/WebGPU **handles** via FFI — not a fake wgpu object
- Cores do not depend on wgpu
- Encode intake (compatible / adapt / bridge): [backend-preference](../encode/backend-preference.md) · ADR-0004
- Web: no `GPUTexture` → `VideoFrame` ctor — canvas-source path, honesty label: [web-gpu-frame](../encode/web-gpu-frame.md)
