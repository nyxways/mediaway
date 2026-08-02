# ADR-0003: `D3d11SharedDecodeBridge` — D3D11 shared texture → D3D12 open

- **Status**: Accepted — implemented 2026-07-31
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder-windows`

## Context

[`mediaway-wgpu` ADR-0002](../../mediaway-wgpu/adr/0002-decode-to-wgpu-texture-bridge.md)
(**Proposed**, design only) specifies `WgpuDx12DecodeBridge` — the reverse of
the already-shipped `WgpuDx12Bridge` encode direction — importing this crate's
WMF DX11 Zero-Copy decode output (`GpuBufferHandle::DirectX11`, NV12,
[ADR-0001](0001-wmf-h264-dx11-out.md), **Accepted**) into an ordinary
`wgpu::Texture`. That ADR fixes the **public contract** this crate must
satisfy — struct name, method signatures, ownership model (allocate the
shared texture on the caller's own D3D11 device, `CopySubresourceRegion` from
the decoder's original texture, open on D3D12 via `OpenSharedHandle`), and
synchronization approach (`Flush()` + bounded `D3D11_QUERY_EVENT`/`GetData`
poll) — and explicitly defers this crate's own implementation-level decisions
to a companion crate-local ADR here. This ADR is that companion.

Direct mirror-image precedent, opposite direction: `mediaway-encoder-windows`
`D3d12SharedEncodeBridge` (`src/d3d12_share.rs`,
[ADR-0006](../../mediaway-encoder-windows/adr/0006-d3d12-shared-to-d3d11.md),
**Accepted**) allocates a `D3D12_HEAP_FLAG_SHARED` texture on a caller D3D12
device, `CreateSharedHandle`s it, then opens it on a **freshly created**
same-adapter native D3D11 device via `OpenSharedResource1`. This ADR's bridge
differs in two structural ways the encode precedent does not need to solve:

1. **Both devices are caller-owned** — neither is freshly created here (the
   caller already has a live decode-session `ID3D11Device` and a live
   `wgpu`-extracted `ID3D12Device`), so same-adapter validation must be a real
   **two-sided** LUID comparison, not "create D3D11 on the LUID D3D12 already
   reported" (which is correct by construction and needs no comparison).
2. **Only one cross-API open is needed, not two.** The encode bridge creates
   on D3D12 then opens on D3D11 (its D3D12 side is the source of truth).
   Here, the shared texture is created directly on the caller's own D3D11
   device (no re-open needed on that side) and only opened once, on D3D12.

## Decision

> Add `src/d3d11_shared_decode_bridge.rs` (new, `#[cfg(all(windows, feature =
> "video"))]`), declared `mod d3d11_shared_decode_bridge;` +
> `pub use d3d11_shared_decode_bridge::D3d11SharedDecodeBridge;` at the crate
> root — mirroring `mediaway-encoder-windows`'s exact `mod d3d12_share;` /
> `pub use d3d12_share::D3d12SharedEncodeBridge;` placement and its top-level
> `//! Interop: [...]` doc-comment line. No `Cargo.toml` changes: every
> `windows` feature this bridge needs (`Win32_Graphics_Direct3D11`,
> `Win32_Graphics_Direct3D12`, `Win32_Graphics_Dxgi`, `Win32_Foundation`) is
> already enabled for this crate (checked directly against `Cargo.toml`).

### Struct shape (ZCA sketch, no bodies)

```rust
pub struct D3d11SharedDecodeBridge {
    d3d12_resource: ID3D12Resource,
    d3d11_device: ID3D11Device,        // caller's device, AddRef'd at open()
    d3d11_context: ID3D11DeviceContext, // GetImmediateContext(), cached once
    d3d11_texture: ID3D11Texture2D,    // our own shared NV12 texture
    shared_handle: HANDLE,             // needs manual CloseHandle — see Drop
}
```

`d3d12_device` (the caller's `ID3D12Device*`) is **not** stored — after
`OpenSharedHandle` returns, the bridge never needs it again (unlike
`D3d12SharedEncodeBridge`, which keeps `d3d11_device` because it *created*
that device and other accessors return handles off it). Smaller struct, one
fewer AddRef held for the bridge's lifetime.

### `open` — call sequence

1. `width == 0 || height == 0` → `DecodeError::InvalidInput` (mirrors
   `D3d12SharedEncodeBridge::open`'s own zero-size check).
2. Borrow + `.clone()` (COM AddRef) both `d3d11_device: NativeHandle` and
   `d3d12_device: NativeHandle` via `ID3D11Device::from_raw_borrowed` /
   `ID3D12Device::from_raw_borrowed`, exactly
   `dx11.rs::device_from_handle`'s established pattern
   (`// clone: COM AddRef for session-owned device handle`). `d3d12_device`'s
   clone is **local to `open`** — not retained in the struct.
3. **Same-adapter validation (two-sided, new relative to the encode
   precedent — see § Context):**
   - D3D12 side: `d3d12_device.GetAdapterLuid()` (real `ID3D12Device` method,
     already used by `d3d12_share.rs`).
   - D3D11 side: `d3d11_device.cast::<IDXGIDevice>()` (safe — proven,
     compiling code today in `mediaway-device-windows/src/dxgi.rs:65`) →
     `unsafe { dxgi_device.GetAdapter() }` (proven at `dxgi.rs:66`, same
     crate) → `unsafe { adapter.GetDesc() }` → `.AdapterLuid`. The
     `GetDesc()` call itself has **no existing precedent in this workspace**
     (flagged in § Residual risk).
   - Compare `LUID { LowPart, HighPart }` field-by-field (not relying on a
     `PartialEq` impl that may or may not exist on the `windows`-crate
     `LUID` type). Mismatch → `DecodeError::InvalidInput`, never an
     undefined cross-adapter open attempt (this is the sibling ADR's
     explicit ask).
4. `d3d11_device.CreateTexture2D(&desc, None, Some(&mut texture))` — NV12,
   `Width`/`Height` as given, `MipLevels: 1`, `ArraySize: 1`,
   `SampleDesc: { Count: 1, Quality: 0 }`, `Usage: D3D11_USAGE_DEFAULT`,
   `BindFlags: D3D11_BIND_SHADER_RESOURCE`, `MiscFlags:
   D3D11_RESOURCE_MISC_SHARED | D3D11_RESOURCE_MISC_SHARED_NTHANDLE`,
   `CPUAccessFlags: 0`. `None` initial data — first real write is step 6's
   `CopySubresourceRegion`, at `copy_from_decoded` time, not `open` time.
5. `.cast::<IDXGIResource1>()` (safe) →
   `unsafe { resource1.CreateSharedHandle(None, DXGI_SHARED_RESOURCE_READ.0 |
   DXGI_SHARED_RESOURCE_WRITE.0, PCWSTR::null()) }` → `shared_handle`.
6. `unsafe { d3d12_device.OpenSharedHandle::<ID3D12Resource>(shared_handle) }`
   → `d3d12_resource`. Must run **after** step 3's LUID check, never before —
   opening a shared handle cross-adapter is undefined, not just slow.
7. `unsafe { d3d11_device.GetImmediateContext() }` → `d3d11_context`
   (cached; avoids re-fetching it on every `copy_from_decoded` call).
8. Assemble `Self { .. }`.

### `copy_from_decoded` — call sequence

1. Borrow `texture: NativeHandle` as `ID3D11Texture2D` via
   `from_raw_borrowed` (same borrowed-pointer convention as
   `dx11.rs::device_from_handle` and `d3d12_video_decode`'s
   `setup::device_from_handle`) → `DecodeError::InvalidInput` if the wrap
   fails (defensive; cannot actually happen given `NativeHandle`'s
   `NonZeroUsize` backing, same non-panicking style `to_native_handle`
   already uses for its own "never expected in practice" case).
2. **New runtime check beyond the sibling ADR's literal text, within its
   spirit ("never silently overwrite/readback", `caveats-and-clarity.md`):**
   `unsafe { borrowed_texture.GetDevice(&mut owning_device) }`
   (`ID3D11DeviceChild::GetDevice` — a real, well-documented, void-returning
   D3D11 method; **exact `windows`-crate out-param signature not verified
   this session**, § Residual risk), then compare
   `Interface::as_raw(&owning_device) == Interface::as_raw(&self.d3d11_device)`.
   Mismatch → `DecodeError::InvalidInput`. Without this check, a caller
   passing a texture from the wrong D3D11 device would hit undefined
   `CopySubresourceRegion` behavior with no runtime signal at all — the
   `texture: NativeHandle` parameter carries no type-level device tag, so
   this is the only cheap way to catch it. Cost: one extra COM call per
   frame, not a GPU stall — negligible next to the query poll below.
3. `unsafe { self.d3d11_context.CopySubresourceRegion(&self.d3d11_texture, 0,
   0, 0, 0, &borrowed_texture, subresource, None) }` — full-region copy
   (`None` box), destination mip 0 / array slice 0 (the bridge's texture is
   always `MipLevels: 1, ArraySize: 1`). Source subresource, dimensions, and
   NV12-ness are trusted from the caller/decode contract — neither this
   bridge nor the D3D11 API can independently re-verify pixel format from a
   bare `NativeHandle`; this matches the same trust boundary
   `D3d12SharedEncodeBridge`'s own `CopyResource`-based callers already
   accept (documented, not silently assumed).
4. `unsafe { self.d3d11_device.CreateQuery(&D3D11_QUERY_DESC { Query:
   D3D11_QUERY_EVENT, MiscFlags: 0 }, Some(&mut query)) }` — a **fresh**
   query object per call, not one cached in the struct (see § Alternatives —
   deliberate simplicity choice, not an oversight).
5. `unsafe { self.d3d11_context.End(&query) }` immediately after the copy,
   so the query's retirement covers exactly this `CopySubresourceRegion`.
6. `unsafe { self.d3d11_context.Flush() }` — queries alone do not force
   submission; `Flush()` is required before the GPU will ever retire the
   query (same reasoning the sibling ADR's § Synchronization already states).
7. Poll `unsafe { self.d3d11_context.GetData(&query, Some(ptr), size, 0) }`
   in a loop **identical in shape** to `dx11.rs::wait_need_input`: 500 ms
   deadline, 1 ms `std::thread::sleep` between polls, `S_OK` (data ready) =
   done, deadline exceeded → `DecodeError::Backend` (no new error variant,
   same choice `wait_need_input` already made for its own timeout).
8. Return `Ok(())`.

### `d3d12_resource_handle`

`to_native_handle(&self.d3d12_resource)` — the exact same free function
`d3d12_share.rs` already defines (`NativeHandle::new(Interface::as_raw(obj)
as usize).ok_or(<Error>::Backend)`), copied into this module with
`DecodeError` substituted for `EncodeError`. Not extracted into a shared
crate for a two-line, two-call-site helper — no ADR-worthy abstraction here.

### `Drop`

Mirrors `D3d12SharedEncodeBridge::drop` exactly: only `shared_handle: HANDLE`
needs manual cleanup (`CloseHandle`, guarded by `!shared_handle.is_invalid()`)
— it is a raw NT handle, not a COM object. `d3d12_resource`, `d3d11_device`,
`d3d11_context`, `d3d11_texture` are all `windows`-crate COM wrapper types;
their own `Drop` impls already call `Release` — no manual handling needed,
same conclusion the encode-direction precedent already reached for its own
COM fields.

## `DecodeError` — reused as-is, zero new variants

`mediaway-decoder::DecodeError` (`crates/mediaway-decoder/src/error.rs`) is
`#[non_exhaustive]` with `Unsupported`, `NoBackend`, `InvalidInput`,
`Backend`, `Closed`. Every failure this bridge can produce maps cleanly onto
the existing two the sibling ADR explicitly anticipated:

| Failure | Variant | Precedent |
|---|---|---|
| `width`/`height` == 0 | `InvalidInput` | `D3d12SharedEncodeBridge::open`'s own zero-size check (`EncodeError::InvalidInput`) |
| Adapter LUID mismatch | `InvalidInput` | Sibling ADR's explicit ask, verbatim |
| Cross-device texture passed to `copy_from_decoded` | `InvalidInput` | New check (§ Decision), same variant class — a caller input problem, not a backend failure |
| `from_raw_borrowed` wrap failure | `InvalidInput` | `dx11.rs::device_from_handle`'s own choice for the analogous case |
| Any D3D11/D3D12/DXGI API call failure (`CreateTexture2D`, `CreateSharedHandle`, `OpenSharedHandle`, `CopySubresourceRegion`, `CreateQuery`, `GetAdapter`, `GetDesc`, …) | `Backend` | Every existing `unsafe` call site in `dx11.rs`/`d3d12_share.rs` maps backend failures to `Backend` |
| Query poll deadline exceeded | `Backend` | `dx11.rs::wait_need_input`'s own timeout choice, verbatim |
| Live COM interface yields a null raw pointer (theoretical) | `Backend` | `to_native_handle`'s own "not expected in practice" framing |

`Unsupported`, `NoBackend`, `Closed` do not apply — this bridge has no
codec/output-preference branching, no backend-selection step, and no
open/closed session lifecycle of its own. **No new `DecodeError` variant
proposed** — resolves cleanly without touching the shared facade type, unlike
the still-open `mediaway-decoder-windows` ADR-0002 DPB-backpressure question.

## `NativeHandle` — reused as-is, no new representation

Public signatures use `mediaway_common::NativeHandle` exactly as the sibling
ADR's contract specifies and exactly as `D3d12SharedEncodeBridge`'s own
public API already does (`open(d3d12_device: NativeHandle, ...)`,
`d3d12_resource_handle() -> Result<NativeHandle, EncodeError>`). Both crates
depend on the **same** workspace-pinned `windows = "0.62"`
(`mediaway-decoder-windows/Cargo.toml` and `mediaway-encoder-windows/Cargo.toml`
both use `windows = { workspace = true, .. }`) — unlike the
`mediaway-wgpu`/`wgpu-hal` boundary (pinned to a *different*, 0.58, `windows`
version), there is **no cross-version COM-type incompatibility** to bridge
here. `NativeHandle` is still the right shape regardless (opaque bits across
a crate boundary, per `docs/spec/gpu-interop.md`), just for a simpler reason
in this specific case: it lets `mediaway-wgpu` depend on this crate without
either crate re-exporting `windows`-crate COM types in its own public API.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Cache one `ID3D11Query` in the struct, reused across `copy_from_decoded` calls | Avoids a small per-call query-object allocation, but adds a state machine (must track "is the previous query's `GetData` already resolved before reusing it?"). A fresh query per call is simpler, self-contained, and the allocation cost is negligible next to the CPU↔GPU stall the poll itself already costs — per AGENTS.md "simplicity first". Revisit only if profiling shows query creation is measurable, which is unlikely relative to the stall. |
| Trust the caller's "same D3D11 device" contract with no runtime check (matches the sibling ADR's literal text, which does not demand this check) | `texture: NativeHandle` carries no type-level device tag; silently accepting a cross-device texture would be undefined `CopySubresourceRegion` behavior with zero caller-visible signal — the exact class of footgun `caveats-and-clarity.md` and this workspace's "never silently wrong" convention exist to prevent. `GetDevice()` + pointer comparison is cheap (one COM call, no GPU stall) and closes a real gap without changing the public signature. |
| `BindFlags: D3D11_BIND_SHADER_RESOURCE \| D3D11_BIND_RENDER_TARGET` (mirroring `d3d12_share.rs`'s own hardware finding: "bare shared heaps fail `OpenSharedResource1`") | That finding was for a **BGRA** D3D12-created texture opened on D3D11, opposite direction and format. Whether an NV12 D3D11 texture needs `RENDER_TARGET` binding to open cleanly on D3D12 (NV12 RTV support is driver/feature-level dependent) is **not established** by that precedent or independently confirmed this session — chose the narrower `SHADER_RESOURCE`-only flag set for v1 and flagged the alternative explicitly as a residual risk rather than copying a flag combination proven for a different format/direction. |
| Full two-sided COM ownership (clone/AddRef the source decode texture in `copy_from_decoded`, hold it past the call) | Not needed — the copy is synchronous from the caller's perspective (the method already blocks until the GPU copy retires before returning, § Decision step 7), so there is no reason to extend the source texture's lifetime past this one call. Matches the sibling ADR's explicit claim that the source handle is "provably never retained past the copy" — a stored/cloned reference would contradict that claim. |
| GPU-side fence hand-off (`ID3D11Device5::CreateFence` → shared → `ID3D12Fence`) instead of the CPU-stall poll loop | Already decided (not re-opened here) by the sibling ADR's own § Synchronization as explicit future work, not a v1 dependency. This ADR inherits that decision rather than re-deciding it. |

## Consequences

### Positive

- Closes the sibling `mediaway-wgpu` ADR-0002's stated dependency gap —
  `WgpuDx12DecodeBridge` now has a fully specified, review-ready contract to
  implement against.
- Reuses every established convention this crate and its direct
  encode-side sibling already use (`NativeHandle` borrow/clone pattern,
  `to_native_handle` helper shape, `DecodeError` variant mapping, the
  `wait_need_input` poll-loop shape, the `mod X; pub use X::Y;` crate-root
  re-export pattern) — minimal genuinely new surface: NV12 shared-texture
  creation + the two-sided LUID check + the new cross-device `GetDevice()`
  guard.
- No `Cargo.toml` changes, no new `mediaway-common` facade changes, no new
  `DecodeError` variant — smallest possible cross-crate blast radius for a
  new GPU-interop primitive.
- The new cross-device `GetDevice()` check closes a real silent-UB gap the
  sibling ADR's contract did not explicitly demand, at negligible extra cost.

### Negative / Trade-offs

- `GpuCopy`, not Zero-Copy — one `CopySubresourceRegion` (D3D11→D3D11, same
  device) plus a CPU↔GPU stall per imported frame, exactly the cost class
  the sibling ADR already labels and budgets for.
- A fresh `ID3D11Query` per `copy_from_decoded` call is a small, deliberate,
  unoptimized allocation on what may become a per-frame hot path — acceptable
  next to the stall itself, but a real (documented) opportunity if this path
  is ever profiled as hot.
- Several exact `windows`-crate 0.62 signatures used here are **not**
  independently confirmed against a pinned docs.rs page this session (see
  below) — same honesty posture, and same class of risk, the sibling ADR
  already disclosed for its own unresolved items.
- `IDXGIAdapter::GetDesc()` has **no existing call site anywhere in this
  workspace** to ground it against, unlike `IDXGIDevice::GetAdapter()`
  (proven, `mediaway-device-windows/src/dxgi.rs`) — the single most
  "new to this workspace" API call this ADR relies on.

## Residual risk (honest, unverified-until-compiled — same posture as the sibling ADR)

1. **`IDXGIResource1::CreateSharedHandle`'s exact `windows`-crate 0.62
   parameter types** (the `DXGI_SHARED_RESOURCE_*` flags — raw `u32` bitwise
   ORed vs. a newtype the crate wraps and expects `BitOr`-composed; the
   security-attributes parameter's exact `Option<...>` shape). Corroborated
   only via web search of Microsoft Learn's prose this session, **not** a
   direct fetch of the pinned windows-rs 0.62 docs.rs page.
2. **`ID3D12Device::OpenSharedHandle`'s exact generic signature**
   (`OpenSharedHandle::<T: Interface>(&self, handle: HANDLE) -> Result<T>`
   per a web search this session) — same caveat as #1, not independently
   fetched from the pinned docs.rs page.
3. **`ID3D11DeviceChild::GetDevice`'s exact `windows`-crate out-param
   signature** — not fetched or web-searched this session at all (this
   check is this ADR's own addition, beyond the sibling ADR's literal
   contract — § Alternatives). Real risk the actual binding shape (owned
   out-param pointer vs. return value) differs from what § Decision assumes.
4. **`D3D11_RESOURCE_MISC_SHARED_NTHANDLE`'s required companion flag.**
   Microsoft Learn text found this session states MiscFlags must include
   `D3D11_RESOURCE_MISC_SHARED_NTHANDLE` **and** either
   `D3D11_RESOURCE_MISC_SHARED` or `D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX`
   — this ADR chose the plain `SHARED` combination (no keyed mutex needed,
   since synchronization is via the query/flush poll, not a keyed mutex).
   Not independently verified against a primary/pinned source this session.
5. **Whether `D3D11_BIND_SHADER_RESOURCE`-only is sufficient** for the
   resulting `ID3D12Resource` to later support `wgpu`'s `create_texture_from_hal`
   + SRV-view creation for NV12 (needed by `WgpuDx12DecodeBridge`) — flagged
   in § Alternatives as a real open question, not resolved here; that
   crate's own ADR-0002 § Residual risk #1 already flags the `wgpu`-side
   half of this same NV12-format uncertainty.
6. **`IDXGIAdapter::GetDesc()`'s exact `windows`-crate signature and the
   `LUID`-carrying `DXGI_ADAPTER_DESC` field name/layout** — well-documented
   Win32 API, but zero existing call sites in this workspace to ground it
   against (unlike `IDXGIDevice::GetAdapter()`, which is proven, compiling
   code in `mediaway-device-windows/src/dxgi.rs`).
7. **No real hardware round trip is possible on the available test hardware
   today** — inherited from the sibling ADR's own already-disclosed finding
   (no working H.264 decode HW MFT available, see ADR-0001's own
   `open_dx11_zero_copy_or_skip` graceful-skip test). This bridge's own
   future smoke test must skip the same way.

**Concrete next step for whoever picks this up**: implement
`src/d3d11_shared_decode_bridge.rs` per § Decision, then
`cargo check -p mediaway-decoder-windows --all-features` (Windows target) —
the residual-risk list above is the first place to look on failure — followed
by a hardware-gated smoke test mirroring
`d3d12_shared_bridge_open_or_skip`'s shape (open a bridge against a real
`D3D11CreateDevice`/`D3D12CreateDevice` pair on the same adapter, assert
`d3d12_resource_handle()` succeeds; skip gracefully, `eprintln!`, on any
missing capability) before attempting a real decode → bridge → readback
round trip.

## References

- [`mediaway-wgpu` ADR-0002](../../mediaway-wgpu/adr/0002-decode-to-wgpu-texture-bridge.md)
  — the design contract this ADR implements against (public signatures,
  ownership model, synchronization approach — not re-decided here).
- [ADR-0001](0001-wmf-h264-dx11-out.md) — this crate's WMF DX11 Zero-Copy
  decode output; the source `GpuBufferHandle::DirectX11` this bridge reads
  from. Lifetime contract unchanged.
- [ADR-0002](0002-d3d12-native-video-decode.md) — checked and confirmed
  irrelevant here: unregistered, TDR-blocked, unrelated D3D12 **native
  decode** path (bitstream parsing), not a source this GPU-interop bridge
  reads from.
- `mediaway-encoder-windows` [ADR-0006](../../mediaway-encoder-windows/adr/0006-d3d12-shared-to-d3d11.md)
  + `src/d3d12_share.rs` — the opposite-direction sharing primitive this
  ADR mirrors (`to_native_handle`, `Drop`/`CloseHandle` pattern, LUID/adapter
  plumbing, zero-size `InvalidInput` check).
- `src/wmf/dx11.rs::wait_need_input` — the poll-loop shape (deadline +
  sleep granularity) `copy_from_decoded`'s query poll reuses.
- `mediaway-device-windows/src/dxgi.rs` — proven, compiling precedent for
  `IDXGIDevice::cast()`/`GetAdapter()` in this exact workspace/`windows`
  version.
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md),
  [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md).
- [`docs/conventions/benchmarking.md`](../../../docs/conventions/benchmarking.md)
  (`GpuCopy` vs Zero-Copy labeling).

## Addendum (2026-07-31): implementation findings

`src/d3d11_shared_decode_bridge.rs` implemented per § Decision, verified against a real
`cargo check`/`clippy`/`fmt`/`test` pass (Windows host, this session). Real `windows`-crate
0.62.2 signatures for each § Residual risk item, checked directly against the crate's vendored
source (`windows-0.62.2/src/Windows/Win32/...`), not just web search:

1. **`IDXGIResource1::CreateSharedHandle`** — matched the ADR's assumed shape exactly:
   `unsafe fn CreateSharedHandle<P2>(&self, pattributes: Option<*const SECURITY_ATTRIBUTES>,
   dwaccess: u32, lpname: P2) -> Result<HANDLE>` (`P2: Param<PCWSTR>`). Compiled as written
   (`None, DXGI_SHARED_RESOURCE_READ.0 | DXGI_SHARED_RESOURCE_WRITE.0, PCWSTR::null()`), gated
   behind `Win32_Security` (already enabled).
2. **`ID3D12Device::OpenSharedHandle`** — **needed a real fix**. Actual signature is
   `unsafe fn OpenSharedHandle<T>(&self, nthandle: HANDLE, result__: *mut Option<T>) ->
   Result<()>` — an out-param pattern (like `CreateTexture2D`), not the assumed
   `OpenSharedHandle::<T>(&self, handle) -> Result<T>` return-value form. Fixed to
   `let mut resource: Option<ID3D12Resource> = None; d3d12_device.OpenSharedHandle(shared_handle,
   &raw mut resource)?; resource.ok_or(Backend)?`.
3. **`ID3D11DeviceChild::GetDevice`** — **needed a real fix, in the easier direction than
   assumed**. Actual signature is a clean safe-shaped wrapper,
   `unsafe fn GetDevice(&self) -> Result<ID3D11Device>` (the ADR assumed a void-returning
   out-param call). `ID3D11Texture2D` reaches it via its `Deref` chain
   (`Texture2D → Resource → DeviceChild`), so `borrowed_texture.GetDevice()` resolves directly
   with no explicit `.cast()`.
4. **`D3D11_RESOURCE_MISC_SHARED_NTHANDLE` companion flag** — the ADR's flag choice (`SHARED |
   SHARED_NTHANDLE`, no keyed mutex) needed no change, but the flags are `D3D11_RESOURCE_MISC_FLAG`
   (`i32`-backed) while `D3D11_TEXTURE2D_DESC::MiscFlags` is `u32` — needed an explicit `.0 as u32`
   cast on each flag before bitwise-OR-ing (same cast pattern the existing `BindFlags` test code
   in this crate's own `lib.rs` already uses for `D3D11_BIND_SHADER_RESOURCE`).
5. **`D3D11_BIND_SHADER_RESOURCE`-only sufficiency** — not a compile-time question; still
   unresolved (real hardware / `wgpu` interop question, not addressable by `cargo check`). Left
   as documented in § Decision / § Alternatives, unchanged.
6. **`IDXGIAdapter::GetDesc()`** — matched the ADR's assumed shape exactly:
   `unsafe fn GetDesc(&self) -> Result<DXGI_ADAPTER_DESC>`, with `.AdapterLuid: LUID` on the
   result. Compiled as written, no fix needed.

**One real correctness finding beyond the ADR's own flagged list**: `ID3D11DeviceContext::GetData`'s
generated wrapper returns `windows_core::Result<()>`, and `HRESULT::is_ok()` is defined as
`self.0 >= 0` — which means **S_FALSE (query not yet ready) also converts to `Ok(())`**, same as
S_OK (query ready). A poll loop checking only `.is_ok()` would treat "not ready yet" as "done"
and return prematurely with stale/undefined texture contents. Fixed by following the ADR's
literal call shape (`GetData(&query, Some(ptr), size, 0)`, not `None`/`0`) and checking the
actual `BOOL` written to `ptr` (`done != 0`) in addition to `.is_ok()`, resetting `done = 0`
before each poll iteration since a not-yet-ready `GetData` call may leave it unwritten.

**Hardware-verified this session** (not just a graceful skip): `open_same_adapter_or_skip`
opened a real `ID3D11Device` + `ID3D12Device` pair on the same explicit adapter
(`IDXGIFactory1::EnumAdapters1(0)`) and printed `d3d11 shared decode bridge ok` —
`D3d11SharedDecodeBridge::open` and `d3d12_resource_handle()` both genuinely succeeded on the
primary adapter. `copy_from_decoded` itself remains unverified against a real decode
output (§ Residual risk 7, inherited, unchanged) — there is still no working H.264
decode HW MFT available.

`cargo fmt -p mediaway-decoder-windows -- --check` flags pre-existing drift across most of this
crate's other files (unrelated to this change, not touched here) — the two new files
(`d3d11_shared_decode_bridge.rs`, `d3d11_shared_decode_bridge_tests.rs`) are clean against the
installed `rustfmt 1.9.0-stable`.

ADRs are **English**. Numbering is local to this `adr/` folder.
