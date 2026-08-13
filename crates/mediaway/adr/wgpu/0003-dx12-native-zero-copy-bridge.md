# ADR-0003: wgpu → D3D12 native encode Zero-Copy bridge (`WgpuDx12NativeBridge`)

- **Status**: Proposed
- **Date**: 2026-08-13
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway` (`wgpu` module)

## Context

[`WgpuDx12Bridge`](../../src/wgpu/dx12.rs) (ADR-0001) extracts the native `ID3D12Device`/
`ID3D12Resource` behind a `wgpu::Device`/`wgpu::Texture` (via `as_hal`/
`create_texture_from_hal`) and feeds it into
[`mediaway_encoder::windows::D3d12SharedEncodeBridge`](../../../mediaway-encoder/src/windows/d3d12_share.rs)
(ADR-0006 in that crate) — a D3D12-shared-heap → native-D3D11 hop, because WMF hardware
encoder MFTs reject `D3D11On12` and wgpu has no D3D11 backend. That ADR is explicit and
correct about the cost: **`EncodePathClass::GpuCopy`**, one GPU→GPU `CopyResource` plus a
CPU↔GPU `device.poll(PollType::Wait)` stall per frame, because the bridge's shared NT handle
carries no cross-device fence.

`mediaway-encoder` ADR-0008 (this session, companion to this ADR) adds GPU-texture input to
the **native D3D12 video-encode** backend (`D3d12VideoEncoder`, `mediaway-encoder`'s
`windows::d3d12_video_encode` module) for H.264/HEVC. That backend already accepts an
**externally-owned** `ID3D12Device*` via `GpuDeviceHandle::DirectX12` — it never creates its
own device. Combined with `WgpuDx12Bridge::new`'s existing `as_hal::<Dx12>()` extraction
technique, this means a wgpu app can hand the *same* `ID3D12Device*` wgpu already owns
straight to `D3d12VideoEncoder::open`, and then feed it a `wgpu::Texture` that was itself
created on/imported into that exact device — with **no shared heap, no second device, no
GPU→GPU copy, no `poll` stall**. This is a structurally different (and cheaper) shape than
`WgpuDx12Bridge`, not a tuning of it.

## Decision

> Add a **new**, separate type — `WgpuDx12NativeBridge` — in `mediaway::wgpu`, alongside
> (not replacing) `WgpuDx12Bridge`. It targets `mediaway-encoder`'s D3D12-native encoder
> (ADR-0008) instead of `D3d12SharedEncodeBridge`/WMF.

### Why a new type, not an extension of `WgpuDx12Bridge`

The two bridges' ownership shapes are fundamentally different, not parametrizations of one
shape: `WgpuDx12Bridge` **allocates** a second GPU resource (the shared D3D12 texture) and
a second device (native D3D11) every `new()`, and **records a copy** every frame.
`WgpuDx12NativeBridge` allocates nothing beyond a borrowed device-pointer wrapper and records
**no** per-frame command at all — it is a pure re-export of pointers the caller already owns.
Folding both into one type would mean a `copy_frame`-shaped method that sometimes copies and
sometimes doesn't, which works against `caveats-and-clarity.md`'s "defaults must not silently
choose a slow path" rule and this workspace's "name why" convention for costly paths.

### API shape (Stage 1, Windows only, H.264/HEVC)

```rust
/// wgpu DX12 HAL interop → D3D12 **native** video-encode Zero-Copy bridge.
///
/// Unlike [`WgpuDx12Bridge`], this creates no second device, no shared heap,
/// records no GPU→GPU copy, and blocks on no `device.poll`. It hands the
/// exact `ID3D12Device*` wgpu's DX12 backend already owns to
/// `mediaway_encoder`'s native D3D12 video-encode session
/// (`GpuDeviceHandle::DirectX12`, ADR-0008 in that crate), then re-exports a
/// caller's own `wgpu::Texture` — already NV12, already created on/imported
/// into *this same* device — as `GpuBufferHandle::DirectX12` with no copy
/// recorded. Genuine `EncodePathClass::ZeroCopy`.
///
/// **Does not solve BGRA/RGBA input.** Most wgpu apps render into BGRA/RGBA
/// render targets, not NV12 — this bridge has no format-conversion step (see
/// `mediaway-encoder` ADR-0008 § Not designed in this pass). Apps without a
/// native NV12 producer should keep using [`WgpuDx12Bridge`] instead.
pub struct WgpuDx12NativeBridge {
    device_handle: NativeHandle,
}

impl WgpuDx12NativeBridge {
    /// Extract the native `ID3D12Device*` behind `device` (must be wgpu's
    /// DX12 backend). Allocates nothing; borrows nothing past this call
    /// (`Interface::as_raw`'s bits are copied out, not the guard itself).
    ///
    /// # Errors
    ///
    /// [`WgpuInteropError::HalUnavailable`] when `device` is not wgpu's DX12
    /// HAL backend.
    pub fn new(device: &wgpu::Device) -> Result<Self, WgpuInteropError>;

    /// [`GpuDeviceHandle::DirectX12`] for
    /// [`mediaway_encoder::VideoEncoderConfig::gpu_device`].
    #[must_use]
    pub fn gpu_device_handle(&self) -> GpuDeviceHandle;

    /// Re-export `texture` as [`GpuBufferHandle::DirectX12`] — **no copy, no
    /// compute pass, no `device.poll` stall**. `texture` must already be
    /// NV12-format, single mip/array-slice, exact session resolution, and
    /// created on/imported into the same `wgpu::Device` passed to
    /// [`Self::new`] (`mediaway-encoder` ADR-0008 validates same-device
    /// identity and rejects a mismatch as `EncodeError::InvalidInput`, not a
    /// panic).
    ///
    /// Only reads `texture`'s raw HAL pointer — never calls wgpu's own
    /// `copy_texture_to_texture`/`write_texture` on it, sidestepping the
    /// confirmed `wgpu-hal` 26.0.6 DX12 backend bug where those calls
    /// `unreachable!()`-panic on any multi-planar (NV12) texture
    /// (`calc_subresource_for_copy`, found during the decode-direction bridge
    /// work — see [`mediaway::wgpu` ADR-0002](0002-decode-to-wgpu-texture-bridge.md)).
    ///
    /// # Errors
    ///
    /// [`WgpuInteropError::HalUnavailable`] when `texture` has no DX12 HAL
    /// backing. [`WgpuInteropError::InvalidInput`] for a null raw pointer.
    pub fn frame_handle(&self, texture: &wgpu::Texture) -> Result<GpuBufferHandle, WgpuInteropError>;
}
```

Composition mirrors `WgpuDx12Bridge`'s existing contract (`docs/spec/api-layers.md`,
"convenience is composition only"): this type does not open or drive a `VideoEncoder` itself
— callers compose it with `mediaway_encoder::windows::D3d12VideoEncoder::open` (once ADR-0008
lands and, separately, once a later pass makes that type reachable outside its own crate — see
§ Residual dependency below).

### How the caller actually populates the NV12 texture (open question, not solved here)

wgpu's own `TextureFormat::NV12` surface is fragile on the pinned `wgpu-hal` 26.0.6 DX12
backend: `wgpu::TextureFormat::NV12` exists (`Features::TEXTURE_FORMAT_NV12`, native-only) but
its `copy_texture_to_texture`/`copy_texture_to_buffer` paths are **confirmed to panic**
(`unreachable!()` in `calc_subresource_for_copy`) for any multi-planar texture, found during
the decode-direction bridge's own work (ADR-0002 in this crate). `frame_handle` itself never
triggers that bug (it only reads a raw pointer), but this ADR does **not** design *how* a
caller fills the NV12 texture in the first place — realistic options (raw HAL-recorded
commands, a compute shader with per-plane `create_view` writes, or a caller that already
produces NV12 outside wgpu's own texture APIs and only wraps it via `create_texture_from_hal`
for the bridge to read) are left to the caller/a future pass, not designed here.

### Fallback strategy — `WgpuDx12Bridge`/`GpuCopy` stays, is not deprecated

This bridge is additive. `WgpuDx12Bridge` remains the right choice when any of:

- The caller's source texture is BGRA/RGBA (no NV12 producer) — WMF's HW MFT accepts BGRA
  directly with no conversion step on its side (`mediaway-encoder` ADR-0005), so `GpuCopy` +
  WMF-BGRA is simpler than writing a custom NV12 conversion pass.
- The target codec is AV1 — `mediaway-encoder` ADR-0008 explicitly excludes AV1 from D3D12
  native GPU input this stage (the CPU-upload AV1 path isn't decodable yet either, ADR-0007's
  still-open bug in that crate).
- The machine's driver lacks `ID3D12VideoDevice3` H.264/HEVC support (`D3D12_FEATURE_VIDEO_
  ENCODER_CODEC` unsupported) but does have a WMF hardware MFT — `WgpuDx12NativeBridge` has no
  fallback of its own; callers needing resilience across both cases should attempt
  `WgpuDx12NativeBridge` first and fall back to `WgpuDx12Bridge` on `open()` failure (an
  `auto`-style policy layer for `mediaway::wgpu` itself is future work, not designed here).

### Residual dependency: `D3d12VideoEncoder` is not yet public

`mediaway-encoder`'s `D3d12VideoEncoder` is `pub(crate)` today (ADR-0007's own "not wired into
the public API yet" note, unchanged by companion ADR-0008). `WgpuDx12NativeBridge` as designed
here only needs `GpuDeviceHandle`/`GpuBufferHandle` (already public in `mediaway-common`) —
it does not need to name `D3d12VideoEncoder` itself. But an **app** cannot actually open a
D3D12-native encode session with this bridge's output until `mediaway-encoder` makes that type
(or an `auto`-routed equivalent) reachable. This ADR's `frame_handle`/`gpu_device_handle` shape
is still worth landing ahead of that (matches ADR-0008's own "capability first, wiring later"
staging), but is **not independently useful** until it does.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Extend `WgpuDx12Bridge` with a second mode/method instead of a new type | Ownership shapes differ fundamentally (allocates+copies vs. borrows+re-exports) — see § Why a new type. |
| Add an optional GPU BGRA→NV12 conversion pass to this same bridge | Scope creep matching `mediaway-encoder` ADR-0008's own deferral; needs a shader + a new path-class taxonomy decision, better reviewed separately. |
| Force wgpu onto Vulkan backend + bridge to `mediaway-encoder-vulkan`'s external-memory import instead | `mediaway-encoder-vulkan` has no external-memory GPU-import encode session yet (CPU-upload only per its own ADR-0001) — no consumer to bridge into today, same reasoning `WgpuDx12Bridge` ADR-0001 already used to rule this out. |
| Do nothing until `D3d12VideoEncoder` is `pub` | Bundling the wgpu-side design with the encoder-side `pub`/`auto`-wiring decision would block progress on this ADR behind an unrelated, larger decision; the two are independently reviewable (see § Residual dependency). |

## Consequences

### Positive

- A wgpu app with a native NV12 producer reaches genuine `EncodePathClass::ZeroCopy` into
  `mediaway-encoder`'s D3D12-native H.264/HEVC encoder — no shared heap, no second device, no
  copy, no stall; strictly cheaper than `WgpuDx12Bridge`'s existing `GpuCopy` path for that
  case.
- `WgpuDx12Bridge` is untouched — existing BGRA/AV1/older-driver callers keep working exactly
  as ADR-0001 shipped them.
- Small, focused surface (`new`/`gpu_device_handle`/`frame_handle`) — no new `unsafe` technique
  beyond `WgpuDx12Bridge`'s already-established `as_hal` pointer-extraction pattern.

### Negative / Trade-offs

- Does not solve the common BGRA/RGBA-source case — see § How the caller actually populates.
- Depends on `mediaway-encoder` ADR-0008 landing first (and, for real end-to-end use, on that
  crate's still-pending `D3d12VideoEncoder` public-API wiring) — not independently shippable
  as a complete story, only as a reviewable slice.
- Introduces a second, narrower bridge type for callers to choose between — a documentation/
  discoverability cost (`docs/ai/wiki/zero-copy/gpu-interop.md` will need a clear "which bridge
  when" table once this lands, not written yet since this is still Proposed).

## References

- [`WgpuDx12Bridge` ADR-0001](0001-dx12-hal-gpucopy-bridge.md) (this crate) — the existing
  `GpuCopy` bridge this ADR sits alongside, and the source of the `as_hal` extraction
  technique this ADR reuses.
- [`WgpuDx12DecodeBridge` ADR-0002](0002-decode-to-wgpu-texture-bridge.md) (this crate) —
  source of the confirmed `wgpu-hal` 26.0.6 DX12 `calc_subresource_for_copy` NV12 panic this
  ADR's `frame_handle` doc warns about.
- `mediaway-encoder` [ADR-0008](../../../mediaway-encoder/adr/windows/0008-d3d12-native-encode-gpu-input.md)
  (companion, same session) — the encoder-side capability this bridge targets.
- `mediaway-encoder` [ADR-0007](../../../mediaway-encoder/adr/windows/0007-d3d12-native-video-encode.md) —
  D3D12 native encoder base (CPU-upload), including AV1's still-open decodability bug.
- `mediaway-encoder` [ADR-0006](../../../mediaway-encoder/adr/windows/0006-d3d12-shared-to-d3d11.md) /
  [ADR-0005](../../../mediaway-encoder/adr/windows/0005-bgra-dxgi-input.md) — why the existing
  `GpuCopy`/WMF-BGRA path remains the right fallback for BGRA-source callers.
- [`docs/spec/gpu-interop.md`](../../../../docs/spec/gpu-interop.md) · ADR-0005 (workspace)
- [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md) · ADR-0009 (workspace)
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) · ADR-0006 (workspace)

ADRs are **English**. Numbering is local to this `adr/` folder.
