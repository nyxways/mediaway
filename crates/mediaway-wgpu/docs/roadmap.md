# mediaway-wgpu — roadmap

## Stage 1 — Windows DX12 → WMF `GpuCopy` (this crate, as authored)

- [x] `WgpuDx12Bridge::new` — `wgpu::Device::as_hal::<Dx12>()` → native
  `ID3D12Device*` → `mediaway_encoder_windows::D3d12SharedEncodeBridge::open`.
- [x] `WgpuDx12Bridge::copy_frame` — wrap the bridge's shared D3D12 resource as
  a `wgpu::Texture` (`create_texture_from_hal`), `copy_texture_to_texture`
  from a caller-owned `wgpu::Texture`, `device.poll(Wait)`, return
  `GpuBufferHandle::DirectX11`.
- [x] `tests/dx12_encode_smoke.rs` — real `wgpu::Instance`/adapter/device,
  synthetic BGRA texture, full encode round trip, Annex-B SPS/IDR check.
  Skips gracefully without DX12/HW MFT hardware.
- [x] **Compile-verify + run** — hardware-verified same-day follow-up
  (ADR-0001 § "Verification update"): found + fixed three real bugs (a
  `windows`-crate 0.58-vs-0.62 version mismatch against `wgpu_hal::dx12`,
  a wrong `PollType::Wait` shape, a wrong `Texture::texture_from_raw` path),
  then confirmed `cargo test -p mediaway-wgpu --all-features` passes — the
  smoke test currently skips at `WindowsVideoEncoder::open`
  (`no HW H.264 MFT for BGRA DXGI input`), cross-checked as a pre-existing,
  already-known hardware/driver limitation shared by
  `mediaway-encoder-windows`'s own `auto_open_gpu_copy_via_d3d12_bridge_or_skip`
  test on the same test hardware, not a defect in this bridge.
- [x] `cargo deny check advisories licenses bans sources` — `advisories ok,
  bans ok, licenses ok, sources ok` with `ash`/`wgpu`/`windows-hal-interop`
  all in the graph.
- [ ] Benchmark `copy_frame`'s GpuCopy path against `zc_wmf_h264_dx11` /
  `D3d12SharedEncodeBridge`'s own numbers before any README ⚡ mention, per
  `docs/conventions/benchmarking.md`'s "same path class for headlines" rule.

## Stage 2 — HEVC / AV1 / VP9 over the same bridge (deferred)

`WindowsVideoEncoder::open` already supports all four codecs over
`GpuBufferHandle::DirectX11` (`mediaway-encoder-windows` ADR-0004); no new
bridge code needed, only test coverage.

## Stage 3 — true Zero-Copy (deferred, needs a sibling crate to mature first)

Vulkan-backend route: force `wgpu` onto its Vulkan backend on Windows
(`Backends::VULKAN`), extract `VkDevice`/`VkImage` via `as_hal::<Vulkan>`,
import via `VK_KHR_external_memory_win32` into a **Vulkan Video encode
session**. Blocked today: `mediaway-encoder-vulkan` is Stage 0 (capability
probe only, no encode session) — see that crate's own roadmap. Revisit once
it has a real `VideoEncoder` implementation.

## Stage 4 — non-Windows backends (not started)

`WgpuMetalBridge` (macOS/iOS, `Backends::METAL` → `VideoToolbox`),
`WgpuVulkanBridge` (Linux, `Backends::VULKAN` → VA-API surface import or a
mature `mediaway-encoder-vulkan`). Neither started; both need a real,
hardware-tested backend crate to bridge into first, same lesson as Stage 3.

## Stage 5 — Windows decode-output → `wgpu::Texture` import (implemented, construction hardware-verified)

Reverse direction of Stage 1: `mediaway-decoder-windows`'s WMF DX11
Zero-Copy decode output (`GpuBufferHandle::DirectX11`, NV12) → an ordinary
`wgpu::Texture`, via `mediaway-decoder-windows::D3d11SharedDecodeBridge`
(D3D11 shared texture → `ID3D12Device::OpenSharedHandle`, ADR-0003 there,
**Accepted**) plus `WgpuDx12DecodeBridge` (`src/dx12_decode.rs`) in this
crate. See [ADR-0002](../adr/0002-decode-to-wgpu-texture-bridge.md) —
**Accepted**, implemented 2026-07-31.

- [x] `D3d11SharedDecodeBridge` in `mediaway-decoder-windows` (prerequisite,
  own crate-local ADR-0003, hardware-verified `open`/`d3d12_resource_handle`)
- [x] `WgpuDx12DecodeBridge::new` / `import_decoded_texture` in this crate
- [x] Real hardware smoke test — `tests/dx12_decode_smoke.rs`, mirrors
  `tests/dx12_encode_smoke.rs`'s graceful-skip shape. **Construction-only**
  (no decode HW MFT available in testing so far to produce real
  decoded content, so no `import_decoded_texture` round trip yet) — and on
  this session's run it did not skip: a real wgpu DX12 device +
  same-explicit-adapter `ID3D11Device` pair opened, and
  `WgpuDx12DecodeBridge::new` genuinely succeeded end to end (HAL
  extraction, `D3d11SharedDecodeBridge::open`, `create_texture_from_hal`).
- [x] `wgpu::TextureFormat::NV12` requirements confirmed directly against
  the pinned `wgpu-types 26.0.0` / `wgpu 26.0.1` / `wgpu-hal 26.0.6` source
  this workspace's `Cargo.lock` resolves `wgpu = "26.0"` to — exists exactly
  as ADR-0002 assumed, gated by `Features::TEXTURE_FORMAT_NV12` (native-only,
  DX12 + Vulkan). See ADR-0002's implementation addendum.
- [ ] Real decode → `copy_from_decoded` → `import_decoded_texture` round
  trip with actual pixel content, and `create_view`
  (`TextureAspect::Plane0`/`Plane1`) sampling of the imported texture — both
  still blocked on the lack of a working H.264 decode HW MFT in testing so
  far (unchanged from ADR-0002 § Context).

## Cross-cutting

- [`docs/adr/`](../../../docs/adr) — none yet; this crate's decisions are
  crate-local ([`adr/`](../adr/)).
- Platform order: Windows → Web → Linux → other
  ([`docs/roadmap.md`](../../../docs/roadmap.md)).
