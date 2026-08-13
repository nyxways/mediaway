# ADR-0008: D3D12 native video encode — GPU-texture input (H.264/HEVC)

- **Status**: Proposed
- **Date**: 2026-08-13
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`windows::d3d12_video_encode`)

## Context

[`D3d12VideoEncoder`](../../src/windows/d3d12_video_encode.rs) (ADR-0007) drives the native
`ID3D12VideoDevice3`/`ID3D12VideoEncoder` pipeline (H.264, HEVC, AV1) but is **CPU-upload
only**: `open()` unconditionally creates a session-owned `input_texture: ID3D12Resource`
(DEFAULT-heap NV12, `D3D12_RESOURCE_FLAG_NONE`) and every `push_frame` copies caller bytes
into it via an `upload_buffer` (UPLOAD heap) + `CopyTextureRegion`. ADR-0007 explicitly named
this a deferred follow-up: *"Deferred: Zero-Copy GPU input
(`VideoInputPreference::ZeroCopyGpu`) …"*.

Separately, `mediaway-encoder-windows`'s WMF path already has two GPU-input options:
`GpuBufferHandle::DirectX11` Zero-Copy (ADR-0003) and `D3d12SharedEncodeBridge` (ADR-0006,
`GpuCopy` — D3D12 shared-heap → native D3D11). ADR-0006's own context section names "wgpu
DX12" as a motivating scenario, and `mediaway::wgpu`'s `WgpuDx12Bridge` (ADR-0001 in that
crate) targets exactly that bridge today, honestly labeled `EncodePathClass::GpuCopy` (one
GPU→GPU copy + a CPU↔GPU `device.poll(Wait)` stall per frame) because wgpu has no D3D11
backend and WMF rejects `D3D11On12`.

**Key fact this ADR's investigation confirmed**: `D3d12VideoEncoder::open` already takes an
**externally-owned** `ID3D12Device*` via `config.gpu_device: GpuDeviceHandle::DirectX12` — it
never creates its own device (`setup::device_from_handle` just `AddRef`s the caller's
device). This means a caller that already owns the exact `ID3D12Device` this session opens on
(e.g. a wgpu app's own DX12 device, extracted via `wgpu::Device::as_hal`) can, in principle,
hand this encoder an `ID3D12Resource` it also created on that same device with **zero
cross-device sharing machinery at all** — no shared heap, no second D3D11 device, no
GPU→GPU copy, no fence/poll stall. This is a materially different (and cheaper) shape than
ADR-0006's `D3d12SharedEncodeBridge`, which exists specifically to hop between two
*different* devices/APIs (D3D12 → native D3D11).

### Real input-resource contract (ground-truthed, not guessed)

Confirmed directly from the official D3D12 video-encode spec already cached in this repo
(`local/standards/d3d12-video-encoding-h264-hevc/`, registry id
`d3d12-video-encoding-h264-hevc` — the same doc ADR-0007's 2026-08-06 addendum used) and from
this crate's own already-hardware-verified `ops.rs`/`setup.rs` code (not memory-guessed
`windows`-crate signatures — this workspace's local `windows` crate source tree isn't cached
under the usual registry path on this machine, but the exact structs/fields below are already
compiling and hardware-verified in this repo today, which is stronger grounding than a fresh
source read):

- `D3D12_VIDEO_ENCODER_ENCODEFRAME_INPUT_ARGUMENTS::pInputFrame` is `ManuallyDrop<Option<ID3D12Resource>>`
  — a non-owning ABI "lend a pointer for this call" field, exactly what
  [`util::borrow_resource`](../../src/windows/d3d12_video_encode/util.rs) already builds from
  any `&ID3D12Resource`, whether backend-owned (`&self.input_texture`) or truly borrowed
  (`ID3D12Resource::from_raw_borrowed`, the same technique `setup::device_from_handle` already
  uses for the session's device — stopping short of `.clone()`/`AddRef` when only a
  call-duration borrow is needed).
- Spec § "EncodeFrame Structures" states plainly: *"The input frame passed to
  ID3D12VideoEncodeCommandList2::EncodeFrame is a D3D12 resource that can be consumed by other
  portions of the pipeline and **must not have the
  `D3D12_RESOURCE_FLAG_VIDEO_ENCODE_REFERENCE_ONLY` flag**."* — i.e. any ordinary D3D12
  texture works as encode input; only the *reconstructed-picture pool*
  ([`setup::ReconPool`](../../src/windows/d3d12_video_encode/setup.rs), ADR-0007's 2026-08-06
  addendum) needs that flag. No other flag/heap-type requirement is documented for the input
  frame specifically.
- Resource **state** contract: this backend already transitions `input_texture`
  `COMMON → VIDEO_ENCODE_READ → COMMON` around every `EncodeFrame` call (see `ops.rs`). D3D12
  resources are only valid for command-list recording issued via **their own owning device**
  — an `ID3D12Resource*` created by a different `ID3D12Device` object (even on the same
  physical adapter) cannot legally appear in this session's barriers/`EncodeFrame` call at
  all. This is a hard OS/API constraint, not a driver quirk.

## Decision

> Extend [`D3d12VideoEncoder`](../../src/windows/d3d12_video_encode.rs) to accept a
> caller-owned `ID3D12Resource` (via `VideoFrameStorage::Gpu(GpuBufferHandle::DirectX12 {
> resource })`) as the per-frame encode input, gated by the **already-existing**
> `VideoInputPreference::ZeroCopyGpu` (today's `default()` — currently rejected outright by
> `validate_common`). No new `VideoInputPreference` variant is needed; the type-level plumbing
> (`VideoFrameStorage::Gpu`, `GpuBufferHandle::DirectX12`, `GpuDeviceHandle::DirectX12`) already
> exists in `mediaway-common` and is unused by this module today.

### Scope this stage

- **H.264 + HEVC only.** AV1 stays CPU-upload-only and GPU-input `+ CodecKind::Av1` is
  rejected (`EncodeError::Unsupported`) — D3D12 native AV1 encode is not even decodable yet on
  the CPU-upload path (ADR-0007's 2026-08-07 addenda, still-open root cause), so adding GPU
  input to it would add complexity to an already-broken codec with nothing to verify against.
- **Same-`ID3D12Device` only, not merely same-adapter.** The pushed resource's owning device
  (`ID3D12Resource::GetDevice()`) must be the exact COM object identity of
  `config.gpu_device`'s device (`Interface::as_raw` pointer-equal) — not a LUID/adapter
  comparison. This is stricter but also simpler than
  `mediaway-decoder-windows::D3d11SharedDecodeBridge`'s two-sided *same-adapter* LUID check
  (ADR-0003 in that crate): D3D12 offers no cross-device import without an explicit shared
  handle, so "same device" is the only workable contract here, and it is cheap to enforce.
- **Caller keeps ownership; this backend never `Release`s the pushed resource.** Every
  `push_frame` call **borrows** the resource for the synchronous duration of that call only
  (mirrors `GpuBufferHandle::DirectX11`'s existing WMF contract, ADR-0003 in this crate) — no
  `AddRef`/`Release` pair per frame at all (see § ZCA shape).
- **State contract**: the pushed resource must be `D3D12_RESOURCE_STATE_COMMON` when
  `push_frame` is called; this backend transitions it to `VIDEO_ENCODE_READ` and back to
  `COMMON` before returning — byte-identical contract to the CPU-upload path's internal
  texture, so callers don't need new state-machine knowledge.
- **CPU-upload path is unchanged and remains the default fallback** for `CpuUploadOk` sessions
  — this ADR adds a sibling input path, it does not touch `upload_and_copy`/`read_packet`'s
  existing behavior.
- **`GpuBufferHandle::DirectX12` has no subresource field** (unlike `DirectX11`'s
  `{ texture, subresource }`) — matches `D3D12_VIDEO_ENCODER_ENCODEFRAME_INPUT_ARGUMENTS::
  InputFrameSubresource`, which this backend already always passes as `0` (the driver
  addresses NV12's luma/chroma planes internally from that one subresource index; see
  `ops.rs`'s existing CPU-upload copy, which separately targets subresource `0`/`1` only for
  the *staging copy*, not for `EncodeFrame` itself).

### Not designed in this pass (explicitly out of scope)

- **No GPU format-conversion helper.** This ADR only covers a caller that already has an
  NV12-format D3D12 texture on the matching device. Most real GPU-rendering apps (including
  typical wgpu apps) render BGRA/RGBA, not NV12 — this ADR does **not** add a BGRA→NV12
  compute/render conversion pass. Those apps should keep using the existing
  `WgpuDx12Bridge` → `D3d12SharedEncodeBridge` → WMF BGRA Zero-Copy path (ADR-0005 in this
  crate already makes WMF's own HW MFT accept BGRA directly with **no** conversion step on
  the WMF side — only the D3D12→D3D11 hop is `GpuCopy`). Adding a convert step would also
  need a new `EncodePathClass` taxonomy entry (today's `auto.rs` enum is `ZeroCopy | GpuCopy |
  CpuUpload | Readback | Software` — a GPU-resident *conversion* doesn't cleanly fit any of
  those); that is a separate, cross-cutting decision, not folded into this ADR.
- **Not wired into `auto`/`WindowsVideoEncoder` public API.** `D3d12VideoEncoder` stays
  `pub(crate)` for now, same as ADR-0007 left it — this ADR only extends the module's own
  capability; the "later integration pass" ADR-0007 already named (`auto.rs` path-class
  wiring) is unchanged and still pending.

## ZCA shape

```rust
/// Where per-frame NV12 pixels come from — decided once at `open()` from
/// `VideoEncoderConfig::input`. A closed enum (not two `Option` fields),
/// matching this module's existing `GopStructure` convention: the active
/// variant always matches `config.input`, so "exactly one input path is
/// active" is a type-level fact, not a runtime-checked invariant.
enum InputResources {
    /// Backend-owned DEFAULT-heap NV12 texture + UPLOAD-heap staging buffer
    /// (today's only path, unchanged).
    CpuUpload { texture: ID3D12Resource, upload_buffer: ID3D12Resource },
    /// No backend-owned input texture at all — every `push_frame` borrows a
    /// caller-owned resource instead. Saves ~1.5x-frame-size DEFAULT+UPLOAD
    /// heap VRAM for Zero-Copy-only sessions (the CPU-upload fields are never
    /// allocated).
    ExternalGpu,
}
```

`D3d12VideoEncoder` struct changes: replace the current `input_texture: ID3D12Resource,
upload_buffer: ID3D12Resource` fields with `input: InputResources`; add `device: ID3D12Device`
(currently a local in `open()`, dropped implicitly at the end of the function — promoting it
to a session field is the only new persistent ownership this ADR adds, one extra `// clone:`
comment at `open()` time, needed so every later `push_frame` can validate external-resource
device identity).

```rust
// setup.rs — new helpers

/// Borrow (no `AddRef`) a caller-owned `ID3D12Resource*` for exactly the
/// duration of the caller's own stack frame holding `raw`. Mirrors
/// `device_from_handle` stopping short of `.clone()`: zero COM refcount
/// traffic per pushed frame, matching `GpuBufferHandle::DirectX12`'s "caller
/// guarantees liveness for at least this call" contract.
pub(super) fn borrow_external_resource(
    raw: &*mut core::ffi::c_void,
) -> Result<&ID3D12Resource, EncodeError>;

/// Validate `resource` is a legal Zero-Copy input for `device`/`width`/
/// `height`: same `ID3D12Device` object identity (`GetDevice()` +
/// `Interface::as_raw` pointer compare — not a LUID/adapter check), NV12
/// format, exact resolution, single mip/array slice, and **not** flagged
/// `VIDEO_ENCODE_REFERENCE_ONLY` (spec-required exclusion — see § Context).
pub(super) fn validate_zero_copy_input(
    resource: &ID3D12Resource,
    device: &ID3D12Device,
    width: u32,
    height: u32,
) -> Result<(), EncodeError>;
```

```rust
// ops.rs — encode_frame_h264 / encode_frame_hevc gain one parameter

pub(super) fn encode_frame_h264(
    &mut self,
    pts: i64,
    duration: u64,
    gop: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264,
    decision: Option<super::gop::FrameDecision>,
    input: &ID3D12Resource,   // NEW — replaces every prior `&self.input_texture` use
) -> Result<Packet, EncodeError>;
```

`push_frame` dispatches on `(&self.input, &frame.storage)`:

- `(CpuUpload { .. }, VideoFrameStorage::Cpu { data })` → today's `upload_and_copy` path,
  unchanged, then encode with the session's own `texture`.
- `(ExternalGpu, VideoFrameStorage::Gpu(GpuBufferHandle::DirectX12 { resource }))` → borrow via
  `setup::borrow_external_resource`, validate via `setup::validate_zero_copy_input`, encode
  with the borrowed reference directly — **no `upload_and_copy` call, no copy queue work, no
  extra fence wait** beyond the encode queue's own existing `signal_and_wait`.
- Any other combination (CPU frame on a `ZeroCopyGpu` session, GPU frame on a `CpuUploadOk`
  session, wrong `GpuBufferHandle` variant) → `EncodeError::Unsupported`, matching this
  crate's existing mismatch convention (WMF ADR-0003).

### Ownership / `unsafe` boundary

- `setup::device_from_handle` (existing) — unchanged, still `AddRef`s once at `open()`.
- `setup::borrow_external_resource` — **new** `unsafe` block, `// SAFETY:` names the
  `GpuBufferHandle::DirectX12` liveness contract (caller keeps the resource alive for at least
  the synchronous `push_frame` call) and that no ownership transfer/`AddRef` happens.
- `setup::validate_zero_copy_input` — `resource.GetDevice::<ID3D12Device>()` is a real COM
  call that *does* `AddRef` internally (per `IUnknown::QueryInterface`/`GetDevice` semantics);
  the returned owned `ID3D12Device` is compared by raw pointer then dropped (releases
  immediately) — a real, but tiny (one atomic increment/decrement + pointer compare) per-frame
  cost, worth documenting honestly per `caveats-and-clarity.md`, negligible next to the
  `EncodeFrame` GPU work itself.
- No new `unsafe` at the `push_frame`/`ops.rs` call sites beyond what already exists for every
  other D3D12 call in this module (`EncodeFrame`, `ResourceBarrier`, …) — `input: &ID3D12Resource`
  flows into the same `borrow_resource`/barrier helpers unchanged.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| New `VideoInputPreference::ZeroCopyGpuD3d12` variant (parallel to `ZeroCopyGpu`) | `ZeroCopyGpu` already exists and is unused by this backend — adding a second, D3D12-specific variant would fragment the facade's cross-platform config for no real benefit; every other Windows GPU-input path (WMF DX11 ADR-0003) already reuses the same `ZeroCopyGpu` value. |
| Same-**adapter** (LUID) check instead of same-**device** | D3D12 (unlike D3D11-cross-device scenarios) has no legal way to record commands against a resource from a different `ID3D12Device`, even same adapter — a LUID check would be strictly weaker than the real constraint and would let an invalid resource reach `EncodeFrame`, surfacing as an opaque driver failure instead of a clean `EncodeError::InvalidInput`. |
| Take ownership (`AddRef`) of the pushed resource every `push_frame` | Unnecessary refcount churn on a genuine hot path (once per encoded frame) when a pure borrow, scoped to the synchronous call, is sufficient and already the pattern this module uses for its own internal resources via `borrow_resource`. |
| Add a GPU BGRA→NV12 conversion pass in this same ADR | Scope creep — needs a new shader, a new `EncodePathClass` taxonomy decision, and is a materially different (and separately reviewable) design; deferred, see § Not designed in this pass. |
| Wire `D3d12VideoEncoder` into `auto`/`WindowsVideoEncoder` as part of this change | Independent decision already named as future work by ADR-0007; bundling it here would conflate "can this module accept GPU input" with "how does the cross-backend policy layer choose it." |

## Consequences

### Positive

- Closes ADR-0007's explicitly-named "Deferred: Zero-Copy GPU input" gap for H.264/HEVC.
- A caller that already owns the matching `ID3D12Device` (e.g. `mediaway::wgpu`'s
  `WgpuDx12Bridge`-style `as_hal` extraction) can reach genuine `EncodePathClass::ZeroCopy` —
  no shared heap, no second device, no GPU→GPU copy, no `poll(Wait)` stall — a strictly
  cheaper path than ADR-0006's `D3d12SharedEncodeBridge` for NV12-capable callers.
- Zero-Copy-only sessions skip allocating the CPU-upload DEFAULT/UPLOAD-heap textures
  entirely (`InputResources::ExternalGpu` carries no VRAM of its own).

### Negative / Trade-offs

- Adds a real, if small, per-frame validation cost (`GetDevice()` + desc/flag checks) that the
  CPU-upload path doesn't pay — a deliberate correctness trade, not a hot-path regression
  (dominated by the encode itself).
- Does not solve the common "app renders BGRA/RGBA, needs NV12" case — callers without a
  native NV12 producer gain nothing from this ADR and must keep using the `GpuCopy` WMF path
  or write their own conversion step.
- Introduces a second real ownership/lifetime contract callers must get right (same-device,
  `COMMON` state on entry, no `VIDEO_ENCODE_REFERENCE_ONLY` flag) — misuse surfaces as
  `EncodeError::InvalidInput`, not a panic, but is a new footgun class for this module.
- `D3d12VideoEncoder` still isn't part of the public API — this capability is unreachable by
  outside callers until the still-pending `auto`/`WindowsVideoEncoder` integration pass lands.

## References

- ADR-0007 (D3D12 native video encode, CPU-upload) — this ADR's direct predecessor; "Deferred"
  section named this exact gap.
- ADR-0006 (`D3d12SharedEncodeBridge`, `GpuCopy`) — the alternative path this ADR's Zero-Copy
  path is cheaper than, for NV12-capable callers only.
- ADR-0003 (DX11 Zero-Copy input) — the borrow-not-own `GpuBufferHandle` contract precedent
  this ADR mirrors for D3D12.
- ADR-0005 (BGRA DXGI input) — why BGRA-source callers may prefer the WMF path even after
  this ADR ships.
- `local/standards/d3d12-video-encoding-h264-hevc/d3d12_video_encoding_h264_hevc.md` (registry
  id `d3d12-video-encoding-h264-hevc`) — source of the `pInputFrame`/`VIDEO_ENCODE_REFERENCE_ONLY`
  exclusion contract quoted in § Context.
- `mediaway-decoder-windows` ADR-0003 (`D3d11SharedDecodeBridge`) — precedent for a
  `GetDevice()`-based same-device guard on an externally supplied texture (that ADR's is
  same-*adapter* via LUID since it bridges two caller-owned devices; this ADR's is same-*device*
  identity since D3D12 has no cross-device resource sharing without an explicit shared handle).
- [`docs/spec/gpu-interop.md`](../../../../docs/spec/gpu-interop.md) · ADR-0005 (workspace)
- [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md) · ADR-0009 (workspace)
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) · ADR-0006 (workspace)
- Companion ADR: `crates/mediaway/adr/wgpu/0003-dx12-native-zero-copy-bridge.md` (the
  `mediaway::wgpu` consumer of this capability)
- Companion ADR: [ADR-0009](0009-native-capture-shared-handle-zero-copy.md) — investigates the
  non-wgpu caller (`mediaway-device` capture feeding this encoder directly); finds the same
  BGRA→NV12 gap named above blocks that caller too, plus new cross-device fence needs.

ADRs are **English**. Numbering is local to this `adr/` folder.
