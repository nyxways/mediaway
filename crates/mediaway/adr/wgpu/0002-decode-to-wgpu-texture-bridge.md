# ADR-0002: Windows decode-output → `wgpu::Texture` import bridge (`WgpuDx12DecodeBridge`)

- **Status**: Accepted — implemented 2026-07-31
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-wgpu`

## Context

[ADR-0001](0001-dx12-hal-gpucopy-bridge.md) shipped `WgpuDx12Bridge`, a real,
hardware-verified **export** bridge: `wgpu::Texture` (BGRA8, DX12 HAL) →
`mediaway-encoder-windows::D3d12SharedEncodeBridge` (D3D12 shared heap →
native D3D11) → WMF hardware H.264 encode. This ADR designs the **reverse**
direction: Windows hardware **decode** output → an ordinary `wgpu::Texture`,
so an app already rendering/compositing with `wgpu` can display or
post-process a Mediaway-decoded frame without a forced GPU→CPU readback.
Per [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md)'s own
framing ("hand frames to Mediaway encode (or accept decode output back into
wgpu)"), this is the other half of the interop pair ADR-0005 already
anticipated — not a new capability outside that spec's scope.

### Current real state of Windows GPU decode output (verified from source, not memory)

`mediaway-decoder-windows`'s WMF backend (`src/wmf/h264.rs`, `src/wmf/dx11.rs`,
[ADR-0001](../../mediaway-decoder-windows/adr/0001-wmf-h264-dx11-out.md),
**Status: Accepted**) is the only decode path with a real, non-blocked GPU
output today:

- `VideoOutputPreference::ZeroCopyGpu` opens a **hardware** decoder MFT bound
  to a caller-supplied `ID3D11Device` (`GpuDeviceHandle::DirectX11`) via a
  DXGI device manager. `poll_frame` returns
  `VideoFrame { format: PixelFormat::Nv12, storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 { texture, subresource }), .. }`
  — a real `ID3D11Texture2D*` + subresource index, never a silent readback.
  Codec dispatch (`video_subtype` in `src/wmf/codec.rs`) already covers
  H.264/HEVC/AV1/VP9 for this same DX11 path (the struct is named
  `WmfH264Decoder` for historical reasons, but the GPU-output code it shares
  is codec-agnostic).
- **Lifetime contract (already established, this ADR does not change it)**:
  the returned `ID3D11Texture2D` is COM-held by the decoder
  (`GpuFrameHold`/`self.released`) only until the **next** `push_packet`,
  `poll_frame` (that returns a frame), or `flush` call, which recycles the
  surface. Any consumer — including the bridge this ADR designs — must finish
  reading it before the caller drives the decoder again.
- **Not round-trip hardware-verified yet**: per
  [`docs/ai/wiki/platform/windows-decode.md`](../../../docs/ai/wiki/platform/windows-decode.md)
  and `mediaway-decoder-windows/docs/benchmarks.md`, the test machine (RTX
  4090 + Intel UHD 770) currently has **no working Media Foundation decode HW
  MFT for H.264 on either GPU** (`DecodeError::Unsupported` on both), even
  though `ffmpeg`'s `h264_cuvid`/NVDEC decodes the same content fine outside
  MF — an MF-specific driver/registration gap, not an absence of decode
  capability. The DX11 Zero-Copy **code path** is real and exercised by
  `open_dx11_zero_copy_or_skip`-style tests, but those tests currently take
  the graceful-skip branch, so no real pixel content has flowed through this
  path yet.
- **D3D12 native decode is a separate, unrelated, blocked path** —
  `src/d3d12_video_decode.rs` (ADR-0002 in that crate) implements
  `ID3D12VideoDecoder`/`DecodeFrame1` general-GOP H.264 decode with native
  `GpuBufferHandle::DirectX12` output, but is **not wired into any public
  trait** (`mod d3d12_video_decode;`, non-`pub`) and reproduces a **real GPU
  TDR hang** on the test hardware after three real bugs
  were found and fixed — root cause still unresolved, hardware iteration
  **paused** by explicit project-owner decision. It cannot be a v1 dependency
  for this ADR.

**Conclusion**: the only real, buildable, non-blocked decode GPU output to
bridge from today is `GpuBufferHandle::DirectX11` (NV12) out of WMF's DX11
Zero-Copy path — exactly mirroring ADR-0001's own finding that DX12→D3D11 was
the only real bridge available for encode. Symmetrically, this ADR bridges
**D3D11 (decode's native output) → D3D12 (wgpu's only Windows-native
backend) → `wgpu::Texture`** — the reverse resource-sharing primitive of
ADR-0001's D3D12→D3D11 bridge.

### No existing bridge does D3D11 → D3D12 sharing

`mediaway-encoder-windows::D3d12SharedEncodeBridge` ([ADR-0006 in that
crate](../../mediaway-encoder-windows/adr/0006-d3d12-shared-to-d3d11.md))
allocates a `D3D12_HEAP_FLAG_SHARED` resource, `CreateSharedHandle`s it, then
opens it on a **freshly created**, same-adapter native D3D11 device via
`OpenSharedResource1`. That is the opposite direction and does not reuse a
caller-owned D3D11 device (it always makes its own). Nothing in this
workspace today creates a shared **D3D11** texture and opens it as **D3D12**
via `ID3D12Device::OpenSharedHandle` — that primitive must be designed here.

## Decision

> Add **`WgpuDx12DecodeBridge`** — a new, separate type in `mediaway-wgpu`
> (not a method on `WgpuDx12Bridge`) — plus a new companion type,
> **`D3d11SharedDecodeBridge`**, in `mediaway-decoder-windows`, mirroring
> `D3d12SharedEncodeBridge`'s placement (the crate that owns the native side
> being read from hosts the sharing primitive; `mediaway-wgpu` stays a thin
> HAL-extraction + composition layer, never reimplementing D3D11/D3D12
> COM plumbing itself — the same division of labor ADR-0001 already
> established for the encode direction).

### Why a new type, not a `WgpuDx12Bridge` method

- **Opposite direction** (import vs. export) and **opposite native handle
  shape** (`GpuBufferHandle::DirectX11` in vs. out).
- **Different pixel format**: decode output is NV12; `WgpuDx12Bridge::dest`
  is fixed BGRA8 to match WMF's encoder-input requirement. A shared struct
  would need two unrelated `dest` textures, one per direction.
- **Different Mediaway dependency**: this bridges into
  `mediaway-decoder-windows` (new), not `mediaway-encoder-windows`.
- A dedicated type keeps each bridge's field set and invariants honest and
  matches [`docs/spec/api-layers.md`](../../../docs/spec/api-layers.md)'s
  "low-level APIs stay first-class, dedicated entry points" guidance rather
  than overloading one struct with two unrelated resource-sharing shapes.

### `D3d11SharedDecodeBridge` (new, `mediaway-decoder-windows`) — expected shape

This ADR specifies the **contract** this companion type must satisfy so
`mediaway-wgpu` can be designed against it; its own implementation is out of
this crate's ADR authority and needs its **own** crate-local ADR in
`mediaway-decoder-windows/adr/` when built (same boundary ADR-0001 respected
by not describing `D3d12SharedEncodeBridge`'s internals as its own decision).

```rust
pub struct D3d11SharedDecodeBridge { /* .. */ }
impl D3d11SharedDecodeBridge {
    /// Allocate a `D3D11_RESOURCE_MISC_SHARED_NTHANDLE` NV12 texture on
    /// `d3d11_device` (the SAME device the decode session was opened on —
    /// not a freshly created one, unlike the encode-direction bridge) and
    /// open it as an `ID3D12Resource` on `d3d12_device` via
    /// `ID3D12Device::OpenSharedHandle`. Validates same-adapter (`GetAdapterLuid`
    /// on both sides, mirroring `d3d12_share.rs`'s existing check) —
    /// `DecodeError::InvalidInput` on mismatch, never an undefined
    /// cross-adapter open attempt.
    pub fn open(
        d3d11_device: NativeHandle,
        d3d12_device: NativeHandle,
        width: u32,
        height: u32,
    ) -> Result<Self, DecodeError>;

    /// `CopySubresourceRegion` from the decoder's `(texture, subresource)`
    /// into this bridge's own shared D3D11 texture, on the SAME D3D11
    /// device/context the decode texture already lives on (no cross-device
    /// concern for this step — see § Synchronization). Then blocks until the
    /// GPU copy retires (§ Synchronization) before returning.
    pub fn copy_from_decoded(
        &self,
        texture: NativeHandle,
        subresource: u32,
    ) -> Result<(), DecodeError>;

    /// Opaque `ID3D12Resource*` for `mediaway-wgpu` to HAL-wrap.
    pub fn d3d12_resource_handle(&self) -> Result<NativeHandle, DecodeError>;
}
```

### `WgpuDx12DecodeBridge` (this crate) — public surface

```rust
pub struct WgpuDx12DecodeBridge { /* .. */ }
impl WgpuDx12DecodeBridge {
    /// Extract the native `ID3D12Device*` behind `device` (must be wgpu's
    /// DX12 backend), open a `D3d11SharedDecodeBridge` sized `width`x`height`
    /// bridging from `d3d11_device` (the SAME device the caller opened its
    /// decode session on), and wrap its shared D3D12 resource once as an
    /// owned `wgpu::Texture` (`create_texture_from_hal`, NV12 format).
    pub fn new(
        device: &wgpu::Device,
        d3d11_device: GpuDeviceHandle,
        width: u32,
        height: u32,
    ) -> Result<Self, WgpuInteropError>;

    /// Copy `frame`'s decode-output GPU texture into this bridge's shared
    /// texture and return it as a `wgpu::Texture`.
    ///
    /// Validates `frame.width`/`frame.height` match `new()`'s size,
    /// `frame.format == PixelFormat::Nv12`, and
    /// `frame.storage` is `VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 { .. })`.
    /// `frame.pts`/`frame.duration` are **not** carried into the returned
    /// `wgpu::Texture` — wgpu has no timestamp concept; callers who need
    /// timing must track it out of band, keyed off the same `frame`.
    pub fn import_decoded_texture(
        &self,
        frame: &mediaway_common::VideoFrame,
    ) -> Result<wgpu::Texture, WgpuInteropError>;
}

/// NV12, matching `PixelFormat::Nv12`'s Windows decode output — see
/// `wgpu::Features::TEXTURE_FORMAT_NV12` requirement below (§ Residual risk).
pub const DECODE_BRIDGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::NV12;
```

`WgpuInteropError` gains two `#[non_exhaustive]`-additive variants: a new
`DecodeBridge(#[from] mediaway_decoder::DecodeError)` (parallel to the
existing `Bridge(#[from] EncodeError)`) and `AdapterMismatch` (the D3D11/D3D12
LUID check failed) rather than overloading `InvalidInput` with a
semantically different failure.

### Ownership — `create_texture_from_hal` takes ownership; the copy-based design sidesteps double-use

`Device::create_texture_from_hal` moves the passed HAL texture in; the
resulting `wgpu::Texture` owns its lifetime from that point (same convention
already relied on and hardware-verified by `WgpuDx12Bridge`'s
`wrap_bridge_resource` — this is **not** newly assumed here). Because
`WgpuDx12DecodeBridge` copies into its **own** persistently-owned D3D12
resource — wrapped **once**, at `new()` time, exactly like
`WgpuDx12Bridge::dest` — rather than HAL-wrapping the decoder's original
resource directly, the decoder's `GpuBufferHandle::DirectX11 { texture, subresource }`
is **never** passed to `create_texture_from_hal` at all. It is only read as a
`CopySubresourceRegion` **source**, on the same D3D11 device/context that
already owns it, inside `D3d11SharedDecodeBridge::copy_from_decoded`.

Consequences, stated explicitly per
[`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md):

- The decoder's own `mediaway-decoder-windows` ADR-0001 lifetime contract
  ("texture lifetime until next `push_packet`/`poll_frame`/`flush` recycles
  it") is **untouched and sufficient** — `import_decoded_texture` must simply
  be called before the caller's next decoder call, exactly as already
  documented there; this ADR adds no new lifetime rule on that side.
- The source `GpuBufferHandle::DirectX11` handle **cannot be "double-used"**
  as a wgpu texture — it is consumed by a same-device GPU copy, not by
  ownership transfer. This is a stronger, simpler guarantee than a raw
  hal-wrap would give, at the cost of a real per-frame copy (see below).
- **`WgpuDx12DecodeBridge`'s returned `wgpu::Texture` is the SAME underlying
  GPU allocation on every call**, reused and overwritten each
  `import_decoded_texture` — identical in spirit to `WgpuDx12Bridge::dest`,
  but a materially sharper footgun here because decode output is often
  **sampled across multiple render frames** (unlike encode's immediate
  push-and-forget). Holding a `wgpu::Texture` returned from one call while
  calling `import_decoded_texture` again observes the **second** frame's
  content, not a stable snapshot. Rustdoc on `import_decoded_texture` must
  say so explicitly; callers needing two live decoded frames simultaneously
  (cross-fade, double-buffering) must open two `WgpuDx12DecodeBridge`
  instances or copy out of the returned texture into their own persistent
  one — this bridge is a single-buffered staging resource, not a frame queue.

### Synchronization — v1 is a documented CPU↔GPU stall, not a raw pointer copy

Two hand-offs exist:

1. **Decode MFT → decoder's own D3D11 texture.** Already synchronized by
   WMF/the driver before `poll_frame` returns a sample — unrelated to this
   ADR, unchanged.
2. **This bridge's `CopySubresourceRegion` (D3D11 → D3D11, same device) → a
   later `wgpu`-recorded GPU command that reads the shared texture via its
   D3D12 view.** This is the new hand-off this ADR introduces, and it is
   **not** GPU-side ordered by default: the shared NT handle carries no
   fence, and D3D12 has no automatic knowledge of when a D3D11 command on a
   different API surface retires — same root issue ADR-0001 already
   documented for the opposite direction ("the bridge's shared NT handle
   carries no cross-device fence yet").

**v1 decision**: `D3d11SharedDecodeBridge::copy_from_decoded` calls
`ID3D11DeviceContext::Flush()` after the copy, then polls an
`ID3D11Query` (`D3D11_QUERY_EVENT`) via `GetData` with a bounded timeout
(same defensive poll-loop shape `mediaway-decoder-windows`'s own
`dx11.rs::wait_need_input` already uses) until the GPU copy is confirmed
retired, **before returning control** to `WgpuDx12DecodeBridge`. Any
wgpu-recorded command submitted afterward is therefore guaranteed to see a
fully-written texture — the CPU already knows the D3D11 copy is done before
any D3D12 command referencing the shared resource is ever submitted. This is
the same **cost class** (`GpuCopy` + CPU↔GPU stall) ADR-0001 already ships
and documents for the encode direction, not a regression, and it is
**achievable in v1 without a compiler pass** the same way ADR-0001's
`device.poll(PollType::WaitForSubmissionIndex)` was designed and later
verified correct.

**Deferred to a later stage**: true GPU-side fence chaining
(`ID3D11Device5::CreateFence` → shared handle → `ID3D12Device::OpenSharedHandle`
as an `ID3D12Fence` → `ID3D12CommandQueue::Wait`) would let wgpu's own queue
wait on the D3D11 copy without a CPU stall. This is real, documented D3D11.3/
D3D12 API surface, but a materially larger `unsafe`/fence-lifecycle design to
get right without a compiler this pass — **not attempted in v1**, tracked as
future work (see `docs/roadmap.md` update below), the same honest
"defer rather than guess a bigger unsafe surface" call ADR-0001 made for
Vulkan-backend true Zero-Copy.

**Labeling**: `import_decoded_texture` is documented as `GpuCopy` class (one
D3D11→D3D11 copy + a CPU↔GPU stall), never Zero-Copy — per
`caveats-and-clarity.md` and `benchmarking.md`'s "never present a copy/
readback path as Zero-Copy" rule, matching `WgpuDx12Bridge::copy_frame`'s
existing honesty bar exactly.

### Scope — Windows/D3D12 only, v1

Matches where the only real, non-blocked decode GPU output
(`GpuBufferHandle::DirectX11`) and the only real `wgpu`-into-Mediaway bridge
(`WgpuDx12Bridge`, DX12) both already live. Vulkan/Linux decode
(`mediaway-decoder-vulkan`: H.264 general-GOP hardware-verified, HEVC GPU
output still all-zero per that crate's own roadmap) and macOS/iOS
(VideoToolbox `CVPixelBuffer`) are explicitly **not** designed here — same
"needs a mature sibling backend crate first" deferral ADR-0001 already
applied to Stage 3/4 of the encode direction.

### Residual risk (honest, unverified-until-compiled — same posture as ADR-0001)

1. **`wgpu::TextureFormat::NV12` exact identifier and requirements.** A web
   search this session (not a direct docs.rs fetch pinned to 26.0.0, unlike
   ADR-0001's own verification) found `Features::TEXTURE_FORMAT_NV12` gates a
   native-only multi-planar NV12 texture format in current wgpu, requiring
   per-plane texture views (`TextureAspect::Plane0`/`Plane1`) for sampling,
   and that "creation of textures of format NV12 is a native-only feature" —
   consistent with using `create_texture_from_hal` to **import** an
   already-existing native NV12 resource (bypassing wgpu's own creation-path
   validation), but the caller's `wgpu::Device` must still have requested
   `Features::TEXTURE_FORMAT_NV12` at `request_device` time for the format to
   be considered valid when later creating a view. **Not independently
   confirmed against the pinned wgpu 26.0.0 docs.rs page this session** — the
   first real implementation pass must verify this exactly as ADR-0001's
   follow-up caught three real signature mistakes only once compiled.
2. **`ID3D12Device::OpenSharedHandle`'s exact `windows`-crate signature** at
   the pinned `windows-hal-interop = "=0.58.0"` version this crate already
   depends on for DX12 HAL interop — not fetched/confirmed this session.
3. **`ID3D11Device5::CreateFence`/shared-fence availability** is noted only
   as future-stage context above, not relied on for v1 — no risk to this
   ADR's actual decision.

**Concrete next step for whoever picks this up**: file the companion
`D3d11SharedDecodeBridge` crate-local ADR in
`mediaway-decoder-windows/adr/`, implement both types, then run
`cargo check -p mediaway-decoder-windows -p mediaway-wgpu --all-features`
(Windows target) before anything else — the residual-risk list above is the
first place to look on failure — followed by a real hardware smoke test
mirroring `tests/dx12_encode_smoke.rs`'s shape (open a decode session, decode
one real frame, import it, assert non-trivial pixel content via a
`copy_texture_to_buffer` readback in the **test only**, never in the library
path). Given no working H.264 decode HW MFT has been available in testing so
far (see § Context), that test should be written to skip gracefully
the same way `open_dx11_zero_copy_or_skip` already does, and cross-checked
against whether HEVC/AV1/VP9 HW decoder MFTs fare any better on that same
machine before concluding decode-side GPU output is fully blocked there.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Add `import_decoded_texture` as a method on the existing `WgpuDx12Bridge` | Different direction, different fixed pixel format (NV12 vs BGRA8), different Mediaway dependency crate — would force one struct to hold two unrelated `dest` textures and two unrelated backend handles; worse for `api-layers.md`'s "dedicated first-class entry points" guidance. |
| Implement the D3D11→D3D12 sharing plumbing directly inside `mediaway-wgpu` (add D3D11 `windows` features here) instead of a new `mediaway-decoder-windows` type | Breaks the established division of labor ADR-0001 set (reuse the platform crate's own resource-sharing bridge, keep `mediaway-wgpu` a thin composition/HAL-extraction layer). Would also make the new sharing primitive un-reusable by any future non-wgpu D3D12 consumer of decode output (e.g., a future D3D12 Video Encode backend wanting Zero-Copy-ish decode→transcode), unlike placing it in the decoder crate. |
| Hal-wrap the decoder's **original** `ID3D11Texture2D` directly (skip the extra copy) | The decoder's internal DXGI surface pool is not created with `D3D11_RESOURCE_MISC_SHARED_NTHANDLE` (MF's own internal allocator, not caller-controlled) — there is no NT handle to open on D3D12 without first copying into a caller-allocated shared texture, mirroring exactly why the encode-direction bridge (ADR-0006) also copies rather than sharing the app's original texture directly. |
| Full GPU-side fence hand-off in v1 (`ID3D11Fence`/`ID3D12Fence` shared handle) | Real API, but a materially larger, unverified `unsafe` surface to design without a compiler this pass; the CPU-stall v1 is the same honest trade-off ADR-0001 already made and shipped successfully for the opposite direction. Tracked as later-stage work. |
| Design against the D3D12 native decode path (ADR-0002 in `mediaway-decoder-windows`) instead of WMF DX11 | That path is unregistered, not wired to any public trait, and reproduces a real unresolved GPU TDR hang on the test hardware — explicitly paused. Not a viable v1 dependency. |

## Consequences

### Positive

- Closes the other half of the `wgpu` interop pair `docs/spec/gpu-interop.md`
  already names ("accept decode output back into wgpu"), symmetric to the
  already-shipped encode direction.
- Reuses the exact `create_texture_from_hal` wrapping technique and
  CPU-stall synchronization pattern already hardware-verified by ADR-0001,
  minimizing genuinely new `unsafe` surface to the new
  D3D11-shared-texture-creation + `OpenSharedHandle` primitive.
- Keeps the copy-based design's ownership story simple and safe: the
  decoder's existing lifetime contract needs no changes, and the source
  handle is provably never retained past the copy.

### Negative / Trade-offs

- `GpuCopy`, not Zero-Copy, same as the encode direction — one extra
  D3D11→D3D11 copy plus a CPU↔GPU stall per imported frame.
- Requires a **new, not-yet-built** companion type in
  `mediaway-decoder-windows` (`D3d11SharedDecodeBridge`) with its own
  crate-local ADR before this crate's `WgpuDx12DecodeBridge` can be
  implemented — this ADR is a design contract against that not-yet-existing
  surface, same honesty posture ADR-0001 used for
  `D3d12SharedEncodeBridge` before it existed... except here the dependency
  genuinely does not exist yet at all (ADR-0001's dependency already shipped
  by the time it was written).
- The reused, single-buffered `dest` texture is a sharper footgun for decode
  (frequently sampled across multiple render frames) than it was for encode
  (immediate push-and-forget) — must be documented prominently, not just
  noted in passing.
- No real hardware round trip is possible today (no working H.264 decode HW
  MFT available in testing so far) — verification will
  have to happen on different hardware, or wait for that gap to close, or be
  limited to the graceful-skip path the same way `WgpuDx12Bridge`'s own test
  currently is.
- `wgpu::TextureFormat::NV12`'s exact requirements were not confirmed against
  a pinned docs.rs page this session (§ Residual risk #1) — real risk of a
  signature/feature-flag mismatch on first compile, same class of risk
  ADR-0001 disclosed and then found three real instances of.

## References

- [ADR-0001](0001-dx12-hal-gpucopy-bridge.md) — this ADR's direct template
  (encode direction, same crate)
- `mediaway-decoder-windows` [ADR-0001](../../mediaway-decoder-windows/adr/0001-wmf-h264-dx11-out.md)
  (WMF DX11 Zero-Copy decode output — the source this ADR bridges from)
- `mediaway-decoder-windows` [ADR-0002](../../mediaway-decoder-windows/adr/0002-d3d12-native-video-decode.md)
  (D3D12 native decode — checked and found paused/TDR-blocked, not usable)
- `mediaway-encoder-windows` [ADR-0006](../../mediaway-encoder-windows/adr/0006-d3d12-shared-to-d3d11.md)
  (`D3d12SharedEncodeBridge` — the opposite-direction sharing primitive this
  ADR's `D3d11SharedDecodeBridge` mirrors)
- [`docs/ai/wiki/platform/windows-decode.md`](../../../docs/ai/wiki/platform/windows-decode.md),
  [`docs/ai/wiki/zero-copy/gpu-interop.md`](../../../docs/ai/wiki/zero-copy/gpu-interop.md)
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md) · [`docs/adr/0005-gpu-interop.md`](../../../docs/adr/0005-gpu-interop.md)
- [`docs/spec/api-layers.md`](../../../docs/spec/api-layers.md), [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md)
- [`docs/conventions/benchmarking.md`](../../../docs/conventions/benchmarking.md) (`GpuCopy` vs Zero-Copy labeling)

## Addendum (2026-07-31): implementation findings

`src/dx12_decode.rs` implemented per § Decision against the now-real, hardware-verified
`D3d11SharedDecodeBridge` ([`mediaway-decoder-windows` ADR-0003](../../mediaway-decoder-windows/adr/0003-d3d11-shared-decode-bridge.md),
**Accepted**) — that companion type's public signatures (`open(d3d11_device: NativeHandle,
d3d12_device: NativeHandle, width: u32, height: u32) -> Result<Self, DecodeError>`,
`copy_from_decoded(&self, texture: NativeHandle, subresource: u32) -> Result<(), DecodeError>`,
`d3d12_resource_handle(&self) -> Result<NativeHandle, DecodeError>`) matched this ADR's
assumed shape exactly — no signature drift to adapt to.

**§ Residual risk #1 (`wgpu::TextureFormat::NV12`) — fully resolved by direct source
inspection, not just compiled.** Checked directly against this workspace's pinned
`Cargo.lock` resolution (`wgpu-types 26.0.0`, `wgpu 26.0.1`, `wgpu-hal 26.0.6` — the exact
versions `wgpu = "26.0"` resolves to here), not a web search:

- `wgpu::TextureFormat::NV12` exists exactly as assumed (`wgpu-types-26.0.0/src/lib.rs:2157`):
  two planes, luminance viewable as `R8Unorm`, chrominance viewable as `Rg8Unorm` at half
  width/height, width/height must be even.
- `wgpu::Features::TEXTURE_FORMAT_NV12` (`wgpu-types-26.0.0/src/features.rs:986`) gates it —
  "native only", DX12 + Vulkan — exactly as assumed. The doc comment on `NV12` states the
  feature "must be enabled to use this texture format", confirmed to mean **texture
  *creation*** through wgpu's own API (`wgpu-26.0.1/src/api/device.rs`'s ordinary
  `create_texture`) and later `create_view` calls — **not** `create_texture_from_hal`, which
  bypasses that validation (same "escape hatch" property `WgpuDx12Bridge`'s BGRA8 wrap already
  relies on for its own format). This crate's `WgpuDx12DecodeBridge::new` therefore compiles
  and wraps the resource regardless of whether the caller's `wgpu::Device` requested the
  feature — but a caller who skips requesting it will fail later, at their own `create_view`
  call on the returned texture, not here. Documented on [`DECODE_BRIDGE_FORMAT`] and the
  crate's device-feature requirements.

No other residual-risk item from § Residual risk required a design change — `ID3D12Device::OpenSharedHandle`'s
exact signature (§ Residual risk #2) was already resolved by `mediaway-decoder-windows`
ADR-0003's own addendum (out-param form, not return-value), consumed here as-is since
`mediaway-wgpu` never calls it directly (only `D3d11SharedDecodeBridge` does).

**`WgpuInteropError::AdapterMismatch`**: added exactly as this ADR specifies, but not
currently constructed by `WgpuDx12DecodeBridge::new` — `D3d11SharedDecodeBridge::open`'s own
two-sided LUID check (ADR-0003 § Decision step 3) already folds an adapter mismatch into
`DecodeError::InvalidInput`, surfaced here through the new `WgpuInteropError::DecodeBridge`
variant instead. `AdapterMismatch` is kept as a distinct, documented, `#[non_exhaustive]`
variant for API-contract parity and future use (see its own rustdoc in `src/error.rs`), the
same "declared ahead of use" precedent `GpuBufferHandle`'s own variants already set in
`mediaway-common`.

**Hardware-verified this session, beyond a graceful skip** — the new
`tests/dx12_decode_smoke.rs::wgpu_dx12_decode_bridge_constructs_on_same_adapter_or_skip` test
opened a real wgpu DX12 device (`Features::TEXTURE_FORMAT_NV12` confirmed supported and
requested) and a real `ID3D11Device` on the same explicit DXGI adapter
(`enumerate_adapters(DX12)[0]` matched against `EnumAdapters1(0)`, mirroring
`D3d11SharedDecodeBridge`'s own `open_same_adapter_or_skip` test), then called
`WgpuDx12DecodeBridge::new` end to end — HAL extraction, `D3d11SharedDecodeBridge::open`
(shared-texture creation + `OpenSharedHandle`), and `create_texture_from_hal` all **genuinely
succeeded** on an RTX 4090, printing `wgpu dx12 decode bridge:
construction ok (same explicit adapter)`, not a skip message. This also resolves
`mediaway-decoder-windows` ADR-0003 § Residual risk #5 in the positive: `D3D11_BIND_SHADER_RESOURCE`-only
on the D3D11 side **is** sufficient for `wgpu`'s `create_texture_from_hal` to accept the
opened D3D12 resource — at least for construction; per-plane view creation and sampling are
still unverified (see below).

**Still unverified** (unchanged from this ADR's own § Context and § Consequences, and
inherited from ADR-0003 § Residual risk #7): a full decode → `copy_from_decoded` →
`import_decoded_texture` round trip with real pixel content, and `create_view` /
`TextureAspect::Plane0`/`Plane1` sampling of the imported texture — no working
H.264 decode HW MFT has been available in testing so far to produce a real
`GpuBufferHandle::DirectX11` decode output to feed in. `cargo check`, `cargo clippy --all-features
-- -D warnings`, `cargo fmt --check`, and `cargo test` (2 integration tests + the pre-existing
encode smoke test, all passing) all ran clean on this Windows host.

ADRs are **English**. Numbering is local to this `adr/` folder.
