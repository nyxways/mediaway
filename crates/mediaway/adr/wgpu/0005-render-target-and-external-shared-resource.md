# ADR-0005: `WgpuDx12Bridge` — render-direct Zero-Copy + external shared resource import

- **Status**: Accepted — hardware-verified 2026-08-20
- **Date**: 2026-08-20
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway` (`wgpu` module)

## Context

[`WgpuDx12Bridge`](../../src/wgpu/dx12.rs) (ADR-0001, hardware-verified) copies a caller's
own `wgpu::Texture` into the bridge's internally-allocated shared D3D12 texture every frame
(`copy_frame`: `copy_texture_to_texture` + `device.poll(Wait)` stall), then hands the D3D11
view of that shared texture to WMF. Labeled `EncodePathClass::GpuCopy`, correctly — a real
GPU→GPU copy happens.

That copy exists only because the caller's source texture and the bridge's shared texture are
two separate GPU allocations. Nothing about WMF/D3D11 requires this: `D3d12SharedEncodeBridge`
(`mediaway-encoder` ADR-0006) already allocates its shared texture with
`D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET` — it just never exposes that texture as a
render-attachment-capable `wgpu::Texture` a caller could render into directly. If a caller
renders straight into the bridge's own shared texture instead of a separate one, `copy_frame`'s
`copy_texture_to_texture` step disappears entirely — the only remaining cost is the
CPU↔GPU sync wait already documented (no cross-device fence exists yet, ADR-0001), which is a
sync cost, not a payload copy, and so satisfies [Zero-Copy marks](../../../../docs/ai/wiki/zero-copy/marks.md)'s
"no payload memcpy" bar.

This is a different, smaller change than [ADR-0003](0003-dx12-native-zero-copy-bridge.md)'s
`WgpuDx12NativeBridge` (still Proposed, blocked on `mediaway-encoder` ADR-0008's
not-yet-public `D3d12VideoEncoder`, NV12-only). ADR-0003 removes the shared-heap/second-device
hop entirely by targeting the native D3D12 encoder instead of WMF. This ADR keeps targeting
WMF via the existing `D3d12SharedEncodeBridge` — same destination, same D3D11 hop — and only
removes the *redundant intra-bridge copy* into that shared texture. Both are legitimate,
independent paths to Zero-Copy for different encoder targets; this one is unblocked today.

A second, related gap: `WgpuDx12Bridge::new` always **allocates** its own shared D3D12
resource. Some callers (e.g. a texture already produced by another sharing primitive, such as
[`D3d11SharedDecodeBridge`](../../../mediaway-decoder/adr/windows/0003-d3d11-shared-decode-bridge.md)'s
D3D12 output, or an app's own pre-existing shared render target) already have a live,
`D3D12_HEAP_FLAG_SHARED`-allocated `ID3D12Resource*` and should not need the bridge to
allocate a second one just to reach WMF.

## Decision

> Extend `WgpuDx12Bridge` (not a new type — see § Why not a new type) with two additions,
> alongside the existing `new`/`copy_frame` (both unchanged):
>
> 1. `render_target(&self) -> &wgpu::Texture` — the bridge's shared texture, now allocated
>    with `RENDER_ATTACHMENT | COPY_DST` usage (was `COPY_DST`-only; the underlying D3D12
>    resource already permits render-target use via `ALLOW_RENDER_TARGET`, so this is a wgpu
>    validation-flag correction, not a new allocation).
> 2. `handle(&self, device: &wgpu::Device) -> Result<GpuBufferHandle, WgpuInteropError>` — the
>    Zero-Copy counterpart of `copy_frame`: no `copy_texture_to_texture`, just
>    `device.poll(PollType::Wait { submission_index: None, .. })` (wait for *all* outstanding
>    work, since the caller's own render pass submission index isn't known to the bridge) then
>    `bridge.as_dx11_handle()`.
> 3. `from_external_shared_resource(device: &wgpu::Device, resource: NativeHandle, width: u32,
>    height: u32) -> Result<Self, WgpuInteropError>` — wraps a caller-owned
>    `D3D12_HEAP_FLAG_SHARED` resource (already living on `device`'s own extracted
>    `ID3D12Device`) instead of allocating one. Companion addition in `mediaway-encoder`:
>    `D3d12SharedEncodeBridge::open_with_resource(d3d12_device: NativeHandle, d3d12_resource:
>    NativeHandle) -> Result<Self, EncodeError>` (extracted from `open`'s existing tail —
>    `CreateSharedHandle` → `EnumAdapterByLuid` → `D3D11CreateDevice` → `OpenSharedResource1` —
>    skipping only `CreateCommittedResource`). See that crate's own ADR-0011.

### Why not a new type

Unlike ADR-0003's `WgpuDx12NativeBridge` (different ownership shape entirely — borrows a
device pointer, allocates nothing), these two additions **reuse the exact same shared texture
and D3D11-open machinery** `new()`/`copy_frame` already established — `render_target`/`handle`
are a second way to *fill* the same `dest` texture (render into it vs. copy into it), not a
different resource-sharing primitive. `from_external_shared_resource` only changes where
`dest`'s backing resource came from (imported vs. allocated); every other field and method is
identical. Splitting these into a separate type would duplicate the entire struct and
`wrap_bridge_resource` helper for a difference of one allocation step.

### Caller contract for `render_target`/`handle`

- Render directly into `render_target()`'s texture (as a `RENDER_ATTACHMENT` view), submit,
  then call `handle(device)` — never `copy_frame` in the same frame (the two are alternative
  ways to reach a handle, not composable per-frame).
- `render_target()` returns the **same** underlying allocation on every call, exactly like
  `copy_frame`'s destination today — single-buffered, not a frame queue. Holding a view across
  the next frame's render observes that next frame's content.
- `handle`'s `device.poll(Wait)` with no submission index waits for **all** of `device`'s
  outstanding GPU work, not just the caller's most recent submission — coarser than
  `copy_frame`'s targeted `WaitForSubmissionIndex`-shaped wait, and worth documenting as a
  minor trade-off (a caller with other unrelated pending GPU work on the same `device` pays a
  longer stall). Fixing this would require the caller to pass their own submission index in;
  deferred as unnecessary complexity for v1 (see § Alternatives).

### `EncodePathClass` labeling

`render_target` + `handle` together are **`EncodePathClass::ZeroCopy`**, not `GpuCopy` — no
payload copy occurs. The CPU↔GPU stall is documented per `caveats-and-clarity.md`'s "pipeline
stalls" caveat category, same as `copy_frame`'s stall is today; a stall caveat does not
disqualify a path from the Zero-Copy label (only a payload `memcpy` does, per
[zero-copy/marks.md](../../../../docs/ai/wiki/zero-copy/marks.md)).

## Alternatives Considered

| Alternative | Why not |
|---|---|
| New `WgpuDx12ZeroCopyBridge` type instead of extending `WgpuDx12Bridge` | Would duplicate the entire struct/`wrap_bridge_resource` helper for a one-allocation-step difference — see § Why not a new type. |
| Pass the caller's submission index into `handle` for a targeted wait | Requires threading a `wgpu::SubmissionIndex` out of the caller's own render-pass submit call into this API — real, but adds a parameter for a stall-duration optimization, not a correctness fix; deferred, flagged as a documented trade-off instead. |
| Have `from_external_shared_resource` also accept a raw `d3d12_device: NativeHandle` param | Redundant and a footgun — the imported `resource` must already live on the *same* device `device: &wgpu::Device` resolves to for `create_texture_from_hal`'s "created from this device's internal handle" contract to hold; extracting the device handle internally (same as `new()`) removes the chance of passing a mismatched pair. |

## Consequences

### Positive

- `WgpuDx12Bridge` callers who control their own render pipeline can reach genuine
  `EncodePathClass::ZeroCopy` into WMF with no `mediaway-encoder` ADR-0008/`D3d12VideoEncoder`
  dependency (unlike ADR-0003) and no NV12 requirement — stays BGRA, matching WMF's existing
  Zero-Copy DX11 input format.
- `copy_frame` is untouched — existing callers with an already-rendered, separately-owned
  texture keep working exactly as ADR-0001 shipped them.
- `from_external_shared_resource` opens the door for composing with other sharing primitives
  (e.g. a future decode→re-encode pipeline reusing `D3d11SharedDecodeBridge`'s output) without
  a redundant second shared allocation.

### Negative / Trade-offs

- `handle`'s untargeted `poll(Wait)` is coarser than `copy_frame`'s submission-indexed wait —
  documented, not fixed, this pass.
- Three ways to reach a `GpuBufferHandle` now exist on one type (`copy_frame`,
  `render_target`+`handle`, and the external-resource constructor composing with the same
  pair) — mitigated by keeping `copy_frame` and `render_target`/`handle` mutually exclusive per
  frame (documented, not enforced by the type system) rather than adding runtime state tracking
  which path was used.
- `render_target`'s `RENDER_ATTACHMENT` usage flag addition changes `wrap_bridge_resource`'s
  `texture_desc` for **every** caller, including existing `copy_frame`-only ones — a strictly
  additive capability (the underlying D3D12 resource already allows it), not expected to break
  anything, but worth calling out as a shared-helper change.

## Addendum (2026-08-20): implementation + hardware verification

Implemented exactly per § Decision. `wrap_bridge_resource`'s `texture_desc.usage` gained
`RENDER_ATTACHMENT` alongside the existing `COPY_DST` — a validation-flag change only, the
underlying D3D12 resource already allowed render-target use.

**Hardware-verified on the reference RTX 4090** (`cargo test -p mediaway --test
dx12_render_target_smoke -- --nocapture`): a real render pass clearing `render_target()`'s
texture, followed by `handle()` (no `copy_texture_to_texture` recorded), produced a valid
`GpuBufferHandle::DirectX11`. `from_external_shared_resource` was verified by extracting the
first bridge's own shared resource via `wgpu::Texture::as_hal::<Dx12>()` (the same escape-hatch
idiom `mediaway` ADR-0002's addendum already used for test-only readback) and re-sharing it on a
second, independent `WgpuDx12Bridge` instance — both `gpu_device_handle()` and `handle()`
succeeded, confirming `CreateSharedHandle` can be called a second time on an already-shared
resource to mint an independent NT handle to the same underlying allocation.

`mediaway-encoder` ADR-0011's `D3d12SharedEncodeBridge::open_with_resource` was verified
separately and directly (not just indirectly through the `mediaway` bridge): a new
`d3d12_shared_bridge_open_with_resource_or_skip` hardware test allocates its own
`D3D12_HEAP_FLAG_SHARED` resource with `CreateCommittedResource` (mirroring `open`'s own
allocation shape) and passes it to `open_with_resource` directly — passed on the same RTX 4090,
alongside the pre-existing `d3d12_shared_bridge_open_or_skip` (`open`, unaffected by the
`open`/`from_resource` refactor — same 2 tests, same pass result, confirmed via `cargo test -p
mediaway-encoder --lib -- d3d12_shared_bridge`).

`cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --check` both clean
on `mediaway` and `mediaway-encoder`. Two real clippy findings fixed along the way (not
mentioned as residual risk, both mechanical): `from_resource` took `ID3D12Device` by value
where a `&ID3D12Device` reference sufficed (`needless_pass_by_value`), and a doc comment needed
`` `AddRef` `` backticks (`doc_markdown`).

## References

- [`WgpuDx12Bridge` ADR-0001](0001-dx12-hal-gpucopy-bridge.md) — the bridge this ADR extends.
- [`WgpuDx12NativeBridge` ADR-0003](0003-dx12-native-zero-copy-bridge.md) — the other,
  independent path to Zero-Copy (native D3D12 encoder, still blocked); not superseded by this
  ADR, a different destination.
- `mediaway-encoder` [ADR-0006](../../../mediaway-encoder/adr/windows/0006-d3d12-shared-to-d3d11.md) —
  `D3d12SharedEncodeBridge`, extended here (`open_with_resource`) by that crate's own ADR-0011.
- [`docs/spec/gpu-interop.md`](../../../../docs/spec/gpu-interop.md),
  [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md),
  [`docs/ai/wiki/zero-copy/marks.md`](../../../../docs/ai/wiki/zero-copy/marks.md).

ADRs are **English**. Numbering is local to this `adr/` folder.
