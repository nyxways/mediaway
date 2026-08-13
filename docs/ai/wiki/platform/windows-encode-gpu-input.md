# Windows D3D12 native encoder — GPU-input Zero-Copy design (Proposed, no code)

Split out of [`windows-encode-d3d12.md`](windows-encode-d3d12.md) (100-line limit). Two
companion ADRs (2026-08-13, both `Proposed`, no implementation yet) design how the D3D12
native encoder (`d3d12_video_encode`, CPU-upload only today) could accept a GPU texture
directly, for two different caller populations.

## Two callers, one shared blocker

- **wgpu-app caller**
  ([`mediaway-encoder` ADR-0008](../../../../crates/mediaway-encoder/adr/windows/0008-d3d12-native-encode-gpu-input.md)
  +
  [`mediaway` `adr/wgpu/0003`](../../../../crates/mediaway/adr/wgpu/0003-dx12-native-zero-copy-bridge.md)):
  a wgpu app extracts its own `ID3D12Device`/`ID3D12Resource` via `as_hal` and hands them
  straight to the encoder — no shared heap, no second device, no copy, no `poll` stall, since
  the encoder already accepts an externally-owned `ID3D12Device`.
- **Native (non-wgpu) caller**
  ([`mediaway-encoder` ADR-0009](../../../../crates/mediaway-encoder/adr/windows/0009-native-capture-shared-handle-zero-copy.md)):
  `mediaway-device`'s own Windows screen capture (DXGI Desktop Duplication) feeding the D3D12
  encoder directly, with no wgpu anywhere.

**Both are blocked on the same missing piece**: the encoder's Zero-Copy input path only
accepts NV12; screen capture and typical wgpu render targets are BGRA. No BGRA→NV12 GPU
conversion pass exists yet — deliberately deferred in both ADRs as its own, separately
reviewable design (shader + a new `EncodePathClass` taxonomy entry).

## Ground-truthed capture facts (ADR-0009)

- **Screen capture** (`windows_desktop::WindowsScreenCapture`, DXGI Desktop Duplication):
  D3D11-based, produces `GpuBufferHandle::DirectX11 { texture, subresource: 0 }`, BGRA8
  (`DXGI_FORMAT_B8G8R8A8_UNORM`). Since `mediaway-device-windows` ADR-0006 (shared
  duplication), **every** session — even a lone consumer — already pays one `CopyResource`
  per frame into a per-consumer `ID3D11Texture2D` (`MiscFlags: 0`, not shareable today).
- **Camera capture** (`windows_camera::WindowsCameraCapture`, Media Foundation): **CPU-only
  today** — no `GpuBufferHandle` produced at all; its own module doc names a DX11 Zero-Copy
  follow-up as unimplemented. Out of scope until that lands.
- `mediaway-device` has no `Direct3D12` Windows feature enabled at all — any future
  `ID3D12Device::OpenSharedHandle` call belongs in `mediaway-encoder`, not `mediaway-device`.

## Already-shipped Zero-Copy, no new design needed

Capture's `GpuBufferHandle::DirectX11` (BGRA8) feeding `mediaway-encoder-windows`'s existing
WMF path is **already genuine same-device Zero-Copy today** (WMF's HW MFT accepts BGRA
directly, ADR-0005 in that crate) — no shared handle, no fence, no conversion. This remains
the cheapest real "capture → HW encode" path in the workspace.

## True D3D11↔D3D12 zero-copy interop is real, but needs a new fence

`IDXGIResource1::CreateSharedHandle` (D3D11, `D3D11_RESOURCE_MISC_SHARED_NTHANDLE`) +
`ID3D12Device::OpenSharedHandle` genuinely open the **same physical GPU allocation** across
devices — no `CopyResource`. But cross-device consumption has no implicit ordering guarantee
(same-device D3D11 immediate-context ordering, which is why capture→WMF needs no fence today,
does not carry over). `IDXGIKeyedMutex` does not work for a D3D12 consumer. Real synchronization
needs a **shared monitored fence** (`ID3D11Device5`/`ID3D12Device::CreateFence`, exportable via
`CreateSharedHandle`) or the same CPU `poll(Wait)`-equivalent stall `WgpuDx12Bridge` already
pays — genuinely new plumbing, not present anywhere in this codebase yet.

## Verdict (ADR-0009)

Do not build the shared-handle bridge yet — solve the shared BGRA→NV12 conversion gap first
(as its own ADR), since it blocks both callers identically; building either bridge ahead of
that would ship no reachable capability.
