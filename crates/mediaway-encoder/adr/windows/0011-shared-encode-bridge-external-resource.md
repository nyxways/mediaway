# ADR-0011: `D3d12SharedEncodeBridge::open_with_resource` — caller-owned shared resource

- **Status**: Accepted — hardware-verified 2026-08-20
- **Date**: 2026-08-20
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`windows` module)

## Context

[`D3d12SharedEncodeBridge::open`](../../src/windows/d3d12_share.rs) (ADR-0006) always
allocates its own `D3D12_HEAP_FLAG_SHARED` texture via `CreateCommittedResource` before
`CreateSharedHandle`/`OpenSharedResource1`. `mediaway` ADR-0005 (companion, same session) needs
a variant that skips allocation and instead shares a resource the **caller** already created
with the same flags (`D3D12_HEAP_FLAG_SHARED` + `D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET`) —
enabling `WgpuDx12Bridge::from_external_shared_resource` to import a pre-existing shared
texture (e.g. one produced by another sharing primitive) instead of forcing a second,
redundant shared allocation.

`CreateSharedHandle` is a method on `ID3D12Device`, not on the resource that created it — any
live `ID3D12Resource*` allocated with the `SHARED` heap flag can be passed to it by whichever
device owns it, regardless of who called `CreateCommittedResource`. Everything after that step
in `open` (`EnumAdapterByLuid`, `D3D11CreateDevice`, `OpenSharedResource1`) is already
resource-shape-agnostic — it only needs the shared handle, not the resource's provenance.

## Decision

> Refactor `open`'s body into two parts: a new private `from_resource(d3d12: ID3D12Device,
> resource: ID3D12Resource) -> Result<Self, EncodeError>` covering everything from
> `CreateSharedHandle` onward (unchanged code, only moved), and a new private
> `device_from_handle(d3d12_device: NativeHandle) -> Result<ID3D12Device, EncodeError>`
> covering the existing borrow+clone (also unchanged, only extracted). `open` becomes:
> `device_from_handle` → `CreateCommittedResource` (unchanged) → `from_resource`. Add:
>
> ```rust
> impl D3d12SharedEncodeBridge {
>     /// Share a caller-owned `ID3D12Resource*` instead of allocating one — `resource` must
>     /// already be `D3D12_HEAP_FLAG_SHARED`-allocated (`ALLOW_RENDER_TARGET` recommended for
>     /// render-target use) on `d3d12_device`. No `width`/`height` parameter: unlike `open`,
>     /// this does not allocate, so the resource's own dimensions apply.
>     ///
>     /// # Errors
>     /// [`EncodeError::InvalidInput`] for a null device/resource pointer, [`EncodeError::Backend`]
>     /// on D3D/DXGI failure (including a `resource` that was not actually shared-heap-allocated —
>     /// `CreateSharedHandle` itself will fail for a non-shared resource).
>     pub fn open_with_resource(
>         d3d12_device: NativeHandle,
>         d3d12_resource: NativeHandle,
>     ) -> Result<Self, EncodeError>;
> }
> ```
>
> Borrow+clone `d3d12_resource` via `ID3D12Resource::from_raw_borrowed` + `.clone()` (COM
> AddRef), mirroring `device_from_handle`'s existing pattern exactly (`// clone: COM AddRef,
> bridge owns a reference for CreateSharedHandle/the struct's lifetime`).

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Take `width`/`height` on `open_with_resource` for symmetry with `open` | Not needed — this constructor never calls `CreateCommittedResource`, so there is no `D3D12_RESOURCE_DESC` to build from them. Adding unused parameters just to match `open`'s signature would be dead input, not real symmetry. |
| Have the caller pass an already-opened `IDXGIResource1`/shared handle directly, skip `CreateSharedHandle` | Would require the caller to know the DXGI plumbing this bridge already encapsulates (`CreateSharedHandle`'s exact flags) — worse ergonomics than accepting a plain `ID3D12Resource*` and doing that step here, same as `open` already does. |
| Validate `resource`'s `D3D12_RESOURCE_DESC` (dimensions/format/flags) before sharing | `ID3D12Resource::GetDesc()` exists and could catch a non-BGRA/non-shared resource early with a clearer error — deferred: `CreateSharedHandle` already fails cleanly (`EncodeError::Backend`) for a non-shared resource, and this crate has no existing call site for `GetDesc()` to model the check on (same "don't guess an unverified signature" caution ADR-0003 in `mediaway-decoder-windows` applied to `IDXGIAdapter::GetDesc()`). Revisit if a confusing failure mode shows up in practice. |

## Consequences

### Positive

- `WgpuDx12Bridge::from_external_shared_resource` (this session's companion `mediaway` ADR-0005)
  has a real, minimal-diff dependency to build against — no new `unsafe` technique beyond the
  borrow/clone pattern every other method in this file already uses.
- `open`'s own behavior and public signature are completely unchanged — this is a pure
  extraction + addition.

### Negative / Trade-offs

- No validation that `resource` was actually allocated with the `SHARED` heap flag before
  `CreateSharedHandle` is attempted — an honest, encapsulated failure (`EncodeError::Backend`)
  rather than a panic, but not as specific an error as a pre-check could give (see
  § Alternatives).
- `open_with_resource` trusts the caller that `resource` lives on `d3d12_device` — no
  cross-check exists (or is practical: D3D12 has no "which device owns this resource" query
  analogous to D3D11's `GetDevice()`). A mismatched pair fails at `CreateSharedHandle` with
  `EncodeError::Backend`, not a more specific error.

## Addendum (2026-08-20): implementation + hardware verification

Implemented exactly per § Decision — `open`'s existing tail moved verbatim into `from_resource`,
`device_from_handle` extracted verbatim from `open`'s existing borrow/clone. Hardware-verified
on the reference RTX 4090: the pre-existing `d3d12_shared_bridge_open_or_skip` test (exercising
`open`) still passes unchanged, and a new `d3d12_shared_bridge_open_with_resource_or_skip` test
(allocates its own `D3D12_HEAP_FLAG_SHARED` resource via `CreateCommittedResource`, mirroring
`open`'s own allocation shape, then calls `open_with_resource` directly) also passes — both
`d3d11_texture_handle()`/`d3d12_resource_handle()` succeed. `cargo clippy --all-targets
--all-features -- -D warnings` and `cargo fmt --check` clean.

One real, mechanical clippy finding: `from_resource(d3d12: ID3D12Device, ..)` took the device by
value where a `&ID3D12Device` reference sufficed (`needless_pass_by_value`) — fixed to
`from_resource(d3d12: &ID3D12Device, ..)`.

## References

- [ADR-0006](0006-d3d12-shared-to-d3d11.md) — `D3d12SharedEncodeBridge::open`, refactored (not
  behaviorally changed) by this ADR.
- `mediaway` [ADR-0005](../../../mediaway/adr/wgpu/0005-render-target-and-external-shared-resource.md) —
  the consumer this addition exists for.
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md).

ADRs are **English**. Numbering is local to this `adr/` folder.
