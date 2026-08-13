# ADR-0009: Native (non-wgpu) capture-to-D3D12-encoder Zero-Copy — investigation

- **Status**: Proposed (investigation; no code written)
- **Date**: 2026-08-13
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`windows::d3d12_video_encode`), companion findings in
  `mediaway-device` (`windows_desktop`, `windows_camera`)

## Context

ADR-0008 (this folder) and `mediaway/adr/wgpu/0003` design GPU-input Zero-Copy for the D3D12
native encoder assuming a **wgpu app** as the caller. This ADR investigates a different,
arguably more fundamental caller: `mediaway-device`'s own Windows capture feeding
`mediaway-encoder`'s D3D12 native encoder directly, with **no wgpu anywhere in the graph**.

### Ground-truthed capture facts

- **Screen** (`windows_desktop::WindowsScreenCapture`, DXGI Desktop Duplication): D3D11-based
  (`IDXGIOutputDuplication`, `ID3D11Texture2D`). Produces `GpuBufferHandle::DirectX11 {
  texture, subresource: 0 }`, format `DXGI_FORMAT_B8G8R8A8_UNORM` (BGRA8) —
  `windows_desktop/dxgi_shared.rs::attach_consumer`. As of `mediaway-device-windows` ADR-0006,
  **every** session (including a lone consumer) already pays one `CopyResource` per frame from
  the OS-owned duplication surface into a **per-consumer-owned** `ID3D11Texture2D`
  (`MiscFlags: 0` today — not shareable).
- **Camera** (`windows_camera::WindowsCameraCapture`, Media Foundation `IMFSourceReader`): **CPU
  only today.** Frames are `VideoFrameStorage::Cpu`; the module doc already names a DX11
  Zero-Copy follow-up as unimplemented. No `GpuBufferHandle` is produced at all yet — out of
  scope for this ADR until that follow-up lands.
- `mediaway-device`'s Windows feature set enables only `Win32_Graphics_Direct3D11` — **no**
  `Direct3D12` dependency anywhere in that crate today. Any D3D12-side `OpenSharedHandle` must
  therefore happen in a consumer (`mediaway-encoder`), not in `mediaway-device`, preserving that
  crate's existing D3D12-agnostic boundary.

### Is true (no-`CopyResource`) D3D11↔D3D12 interop real?

Yes — confirmed, not guessed. A D3D11 texture created with `D3D11_RESOURCE_MISC_SHARED_NTHANDLE`
and exported via `IDXGIResource1::CreateSharedHandle` (or `ID3D11Resource1`) can be opened by a
**different** `ID3D12Device` via `ID3D12Device::OpenSharedHandle` as a genuine `ID3D12Resource`
referencing the **same physical GPU allocation** — a cross-API *view*, not a copy. This is the
same NT-shared-handle mechanism `d3d12_share.rs`'s `D3d12SharedEncodeBridge` (ADR-0006) already
uses today, just in the opposite direction (that bridge is D3D12→D3D11; this would be D3D11→D3D12).

**The catch — cross-device synchronization, not memory movement.** D3D11 and D3D12 are
independent command queues once a resource crosses devices; there is no implicit ordering
guarantee (unlike same-device D3D11↔D3D11, where the immediate context serializes submission
order, which is *why* today's capture→WMF `GpuBufferHandle::DirectX11` path needs no fence at
all when both sides share one device). `IDXGIKeyedMutex` does not solve this for a D3D12
consumer (no native D3D12 keyed-mutex acquire/release). The correct mechanism is a **shared
monitored fence** (`ID3D11Device5::CreateFence` / `ID3D12Device::CreateFence`, each exportable
via `CreateSharedHandle`) — GPU-side signal/wait, no CPU stall — or, more crudely, the same
CPU-side stall `WgpuDx12Bridge` already pays (`device.poll(Wait)`-equivalent: `Flush` + fence
event wait) before handing the resource to the D3D12 encoder. Either way, this is **new,
undesigned plumbing** — nothing in this codebase does D3D11→D3D12 cross-device fencing today.

### The format wall applies here too — this is the key finding

ADR-0008 explicitly defers BGRA→NV12 GPU conversion (§ "Not designed in this pass") because the
D3D12 native encoder's Zero-Copy input path only accepts NV12. Screen capture's texture is
**BGRA8**, not NV12 — identical to the wgpu-app case. So even after building the shared-handle +
fence machinery above, a capture→D3D12-native-encoder Zero-Copy path hits the **exact same
missing conversion step** as `WgpuDx12NativeBridge`. This is not a smaller problem than the wgpu
path — it shares the same blocking gap, and additionally needs new cross-device sync plumbing
the wgpu path's WMF fallback (`WgpuDx12Bridge`) never needed either, because that fallback stays
BGRA end-to-end.

### What already works today, with zero new code

Capture (`GpuBufferHandle::DirectX11`, BGRA8) feeding `mediaway-encoder-windows`'s **existing**
WMF path (ADR-0001/ADR-0003/ADR-0005 in this folder) is **already genuine same-device Zero-Copy**
today, provided the app passes the same `ID3D11Device` instance as both capture's and the
encoder's `gpu_device` — WMF's hardware MFT accepts BGRA directly (ADR-0005), no shared handle,
no fence, no `CopyResource` beyond ADR-0006's own already-documented per-consumer copy. This is
the cheapest real "native capture → HW encode" story in the workspace right now, and needs no
new design.

## Decision

> **Do not build the D3D11(capture)→D3D12(native-encoder) shared-handle bridge yet.** It is
> real and buildable, but (a) is blocked on the same undesigned BGRA→NV12 conversion step ADR-
> 0008 already deferred, and (b) adds a new cross-device fence/sync requirement neither existing
> Windows bridge (`D3d12SharedEncodeBridge`, WMF-DirectX11) needs. Recommend the BGRA→NV12
> conversion gap be resolved **once**, as its own reviewable ADR, since it blocks *both* this
> native path and the wgpu path identically — solving it once unblocks both callers, rather than
> designing two parallel, largely-duplicate bridges around the same missing piece.
>
> Until then, capture-owning apps that want HW-encoded screen/window video should keep using the
> already-shipped capture→WMF-DirectX11 same-device Zero-Copy path — no gap, no fence, no
> conversion needed for BGRA.

### If/when built (sketch only, not scheduled)

- `mediaway-device`: change screen capture's per-consumer texture creation
  (`windows_desktop/dxgi_shared.rs::attach_consumer`) to optionally request
  `D3D11_RESOURCE_MISC_SHARED_NTHANDLE`, gated by a new opt-in on `DesktopVideoCaptureConfig`
  (not a default — most consumers don't need cross-device export). Export the NT handle once at
  attach time (stable for the texture's lifetime, **not** re-exported per frame) as
  `GpuBufferHandle::DirectXShared`. No `Direct3D12` dependency added to `mediaway-device`.
- `mediaway-encoder`: a new, small bridge (mirrors `D3d12SharedEncodeBridge` reversed) opens that
  shared handle via `ID3D12Device::OpenSharedHandle` once per session, **plus** new shared-fence
  plumbing (signal in the capture driver thread after its `CopyResource`, wait before
  `EncodeFrame`) — this fence work does not exist anywhere in this crate today and is the
  single largest new-design item, independent of the BGRA→NV12 question.
- Still requires the deferred BGRA→NV12 conversion (ADR-0008 § Not designed in this pass) before
  any of the above is useful end-to-end.

## Alternatives Considered

| Alternative | Why not (yet) |
|---|---|
| Build the shared-handle bridge now, accept a CPU stall (à la `WgpuDx12Bridge`) instead of a fence | Would produce a working but `GpuCopy`-cost-equivalent path (a stall, not a copy) that is *strictly worse* than today's already-Zero-Copy same-device WMF path for the same BGRA input — no honest win to ship. |
| Add camera's DX11 Zero-Copy follow-up first, then design this bridge for both capture sources at once | Camera's own Zero-Copy follow-up is unscoped, unimplemented work (module-doc-named only) — bundling would block this investigation's conclusion behind a second undesigned feature. |
| Solve BGRA→NV12 conversion inside this ADR | Scope creep matching ADR-0008's own deferral reasoning — needs a shader/compute pass and a new `EncodePathClass` taxonomy decision; better reviewed as its own ADR shared by both this path and the wgpu path. |

## Consequences

### Positive
- Clarifies that "native path Zero-Copy screen-record with HW encode" **already exists** today
  (capture → WMF-DirectX11, same device) — no new work needed for that specific, common case.
- Identifies the BGRA→NV12 conversion gap as the single shared blocker for *both* the wgpu
  bridge (ADR-0003 in `mediaway`) and this native path, avoiding duplicate design effort.
- Confirms `mediaway-device`'s D3D12-agnostic crate boundary should be preserved: any future
  `OpenSharedHandle` belongs in `mediaway-encoder`, not `mediaway-device`.

### Negative / Trade-offs
- Answers "not yet" rather than shipping a capability — the D3D12-native-encoder-specific
  capabilities (whatever motivated ADR-0007/0008, e.g. GOP/rate-control surface) remain
  unreachable from native capture until the BGRA→NV12 gap and the new fence plumbing both land.
- Cross-device D3D11→D3D12 sync (shared fence) is confirmed necessary but is real, new,
  undesigned surface area — larger than either existing bridge's `unsafe` footprint.

## References

- `mediaway-encoder` ADR-0008 (this folder) — the wgpu-caller-framed GPU-input design; shares
  the BGRA→NV12 blocker this ADR identifies.
- `mediaway-encoder` ADR-0006 (`d3d12_share.rs`, `D3d12SharedEncodeBridge`) — the existing
  opposite-direction (D3D12→D3D11) shared-handle precedent this ADR's sketch mirrors.
- `mediaway-encoder` ADR-0005 (BGRA DXGI input) — why capture→WMF already works Zero-Copy today
  with no conversion step.
- `mediaway` `adr/wgpu/0003-dx12-native-zero-copy-bridge.md` — companion investigation for the
  wgpu-app caller; same BGRA→NV12 blocker, different caller population.
- `mediaway-device-windows` ADR-0006 (`dxgi_shared.rs`) — source of the "every consumer already
  pays one `CopyResource`" fact this ADR relies on; also source of the per-consumer texture's
  current `MiscFlags: 0` (not shareable) fact.
- `mediaway-device` `windows_camera/capture.rs` module doc — source of "camera capture is
  CPU-only today, DX11 Zero-Copy is a named but unimplemented follow-up."
- [`docs/spec/gpu-interop.md`](../../../../docs/spec/gpu-interop.md) · ADR-0005 (workspace)
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) · ADR-0006 (workspace)

ADRs are **English**. Numbering is local to this `adr/` folder.
