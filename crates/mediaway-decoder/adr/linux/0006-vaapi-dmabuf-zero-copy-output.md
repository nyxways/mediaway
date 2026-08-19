# ADR-0006: VA-API DMA-BUF Zero-Copy decode output

- **Status**: Accepted (implemented — WSL2 + Windows compile/clippy/test-verified; zero real
  VA-API hardware verification, see § Open questions)
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (`src/linux/vaapi/`)

## Context

[ADR-0001](0001-vaapi-h264-cpu-out.md) scoped this backend to CPU NV12 readback only and
explicitly deferred Zero-Copy. Confirmed still true in the current tree:
`crates/mediaway-decoder/src/linux/vaapi/h264.rs:77-80`:

```rust
if config.output != VideoOutputPreference::CpuFramesOk {
    // Zero-Copy DMA-BUF export deferred — see ADR-0001 § Scope.
    return Err(DecodeError::Unsupported);
}
```

`VideoOutputPreference::ZeroCopyGpu` is rejected at `open()` time, before any per-frame code
runs. The roadmap (`crates/mediaway-decoder/docs/linux/roadmap.md` § Stage 3) names the goal as
"`GpuBufferHandle::Vulkan` interop path" — **this ADR corrects that**: VA-API's native
Zero-Copy mechanism is **DMA-BUF** (a DRM/GEM buffer-sharing primitive), not a `VkImage`. VA-API
never hands out a `VkImage`; it hands out a POSIX file descriptor plus plane-layout metadata that
a *consumer* (Vulkan, EGL, wgpu) must separately *import* as its own image. `GpuBufferHandle::
Vulkan { image, memory }` (`crates/mediaway-common/src/gpu.rs:78-83`) cannot represent this —
those fields are documented as "opaque `VkImage`"/"device/memory cookie", not an fd + DRM format
modifier + per-plane pitch/offset. A new variant is required. `crates/mediaway-decoder/src/
vulkan/zero_copy.rs` is this workspace's only existing precedent for building a
`GpuBufferHandle` for a real decode backend — read in full and used as the shape/lifecycle
template below, even though the underlying OS mechanism is unrelated.

### `cros-libva` already wraps the real VA-API DMA-BUF export call, safely

Confirmed directly in the vendored source
(`cros-libva-0.0.13/src/surface.rs:349-407`, `Surface<D>::export_prime`):

```rust
pub fn export_prime(&self) -> Result<DrmPrimeSurfaceDescriptor, VaError> {
    let mut desc: bindings::VADRMPRIMESurfaceDescriptor = Default::default();
    va_check(unsafe {
        bindings::vaExportSurfaceHandle(
            self.display.handle(),
            self.id(),
            bindings::VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2,
            bindings::VA_EXPORT_SURFACE_READ_ONLY | bindings::VA_EXPORT_SURFACE_COMPOSED_LAYERS,
            &mut desc as *mut _ as *mut c_void,
        )
    })?;
    // ... converts desc.objects[0..num_objects] to `DrmPrimeSurfaceDescriptorObject { fd:
    // OwnedFd, size: u32, drm_format_modifier: u64 }` and desc.layers[0..num_layers] to
    // `DrmPrimeSurfaceDescriptorLayer { drm_format, num_planes, object_index: [u8; 4],
    // offset: [u32; 4], pitch: [u32; 4] }`.
}
```

This is a **method on `Surface<D>` for any `D`** — it does not require the surface to have been
*created* as DMA-BUF-backed. This backend's existing driver-allocated `Surface<()>` pool
(`h264.rs:45`, `Vec<Option<Surface<()>>>`) can call `export_prime()` on any pool entry as-is —
**no change to surface creation is needed for the output/export direction**. `objects[i].fd` is
already wrapped `unsafe { OwnedFd::from_raw_fd(o.fd) }` (`surface.rs:375`) — every `unsafe` FFI
call for this path is already inside `cros-libva`, not this crate, matching ADR-0001's existing
"no `unsafe` block written in this crate" posture.

`export_prime()` hardcodes `VA_EXPORT_SURFACE_COMPOSED_LAYERS` (request the driver to compose as
few DRM objects as possible), not `SEPARATE_LAYERS`. Real-world driver reports collected during
this investigation (Intel iHD/Mesa `hwcontext_vaapi.c` DMA-BUF import code, VLC's
`interop_vaapi.c`) describe the common NV12 case as **one DRM object, one layer, two planes**
(`num_layers = 2` is also reported by some call sites for the non-composed case — the exact
number is driver-dependent and **unconfirmed on real hardware for this workspace**, see Open
questions). `VADRMPRIMESurfaceDescriptor` itself allows up to 4 objects / 4 layers / 4 planes per
layer (confirmed via the upstream header, see § Cited source below) — this ADR scopes the new
type to the NV12 case actually used by this backend (see § Decision).

### fd ownership is a real, cited constraint — not this crate's convention to invent

Fetched `va/va_drmcommon.h` (upstream `intel/libva`, the canonical libva header
`VADRMPRIMESurfaceDescriptor` doc-comments trace to) directly:

> "Backend driver will not close the file descriptor. Application should handle the release of
> the fd."

This is authoritative for the **export** direction: every `fd` in a `DrmPrimeSurfaceDescriptorObject` is a **caller-owned**, driver-duplicated descriptor (matches `cros-libva` wrapping it in
`OwnedFd`, whose `Drop` calls `close(2)`). A leaked fd here is a real, fast resource-exhaustion
bug class (default `RLIMIT_NOFILE` is typically ~1024; a naive "export a fresh fd every decoded
frame, never close it" implementation would exhaust the process's fd table in seconds at 30 fps).
This is qualitatively different from every existing `GpuBufferHandle` variant: `ID3D11Texture2D*`
/ `VkImage` / `AHardwareBuffer*` pointers do not need an explicit "close" call from a Zero-Copy
consumer — they die with their parent device/pool. A DMA-BUF fd does.

## Decision

> **Implemented** (originally proposed as design-only; a follow-up pass in this same PR
> implemented and WSL2/Windows-verified it — see § Test plan for the real commands run). A
> Zero-Copy decode output path for
> `VideoOutputPreference::ZeroCopyGpu`, scoped to NV12 (this backend's only pixel format), that:
>
> 1. Adds `GpuBufferHandle::DmaBuf(Box<DmaBufDescriptor>)` to `mediaway-common` (new type,
>    boxed — see § Common-crate change).
> 2. Calls `Surface::export_prime()` on the DPB-pool surface backing a decoded picture, on
>    demand, only when the caller requested `ZeroCopyGpu` — never eagerly for `CpuFramesOk`
>    callers (no new cost on the existing CPU path).
> 3. Reintroduces DPB-slot **outstanding-handle tracking**, dropped by
>    [ADR-0002](0002-vaapi-h264-p-slice-dpb.md) specifically because no Zero-Copy handle existed
>    yet — see § DPB changes.
> 4. Adds one new codec-agnostic module, `linux/vaapi/dmabuf.rs`, callable from any future VA-API
>    codec backend in this crate (today, only `h264.rs`) — see § Module layout.

### Common-crate change: `GpuBufferHandle::DmaBuf`

`crates/mediaway-common/src/gpu.rs` currently declares 7 variants, all small (≤2 `NativeHandle`
fields or a `u64`), all `Copy` via the enum-wide `#[derive(Debug, Clone, Copy, PartialEq, Eq,
Hash)]` (`gpu.rs:47`). A faithful DMA-BUF descriptor (fd + DRM fourcc + modifier + width/height +
up to 2 NV12 planes, each with object index/offset/pitch) is **4-5x larger** than the biggest
existing variant — if added as a flat inline variant, Rust's enum layout (one tag + the largest
variant's size) would inflate `GpuBufferHandle`'s `size_of` for **every platform**, including
Windows/Apple/Android/Web builds that never construct a `DmaBuf` value, since `GpuBufferHandle`
is embedded by value in `VideoFrameStorage::Gpu` → `VideoFrame` → every pipeline hot path
workspace-wide. Decision: **box the payload**.

```rust
// mediaway-common/src/gpu.rs — sketch, not implemented this pass.

/// One plane's byte layout within a [`DmaBufDescriptor`]'s referenced DRM object(s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DmaBufPlane {
    /// Which `DmaBufDescriptor` object this plane's bytes live in — `0` or `1`
    /// (this type caps object count at 2; see field docs on `DmaBufDescriptor`).
    pub object_index: u8,
    pub offset: u32,
    /// Row pitch (stride) in bytes.
    pub pitch: u32,
}

/// Linux DRM/GEM DMA-BUF surface — VA-API's native Zero-Copy export shape
/// (`vaExportSurfaceHandle` + `VADRMPRIMESurfaceDescriptor`), scoped to the ≤2-plane,
/// ≤2-object case this workspace's NV12 pipelines produce (see the owning ADR's "why
/// scoped, not general" note before adding a 3rd/4th plane or object).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DmaBufDescriptor {
    /// Primary DMA-BUF fd (DRM object 0 bits, offset by `+1` so fd `0` still round-trips
    /// through `NativeHandle`'s non-zero representation — same convention
    /// `vulkan::zero_copy::build_handle` already uses for a `slot_index` of `0`).
    /// **Borrowed by convention, not owned by this struct** — see the owning ADR's
    /// § Fd lifetime contract for who calls `close(2)` and when.
    pub fd0: NativeHandle,
    /// Second DMA-BUF fd (DRM object 1), only when the driver reported `num_objects == 2`
    /// (a driver that splits Y/UV into separate objects instead of composing them).
    pub fd1: Option<NativeHandle>,
    /// `DRM_FORMAT_*` (e.g. `DRM_FORMAT_NV12`) — **not** `VA_FOURCC_*`; numerically identical
    /// for NV12 today but a distinct namespace (see Open questions).
    pub fourcc: u32,
    /// DRM format modifier (tiling layout). `0` is `DRM_FORMAT_MOD_LINEAR`, itself a valid,
    /// meaningful value — never treated as "absent" (unlike `NativeHandle`'s `0`-is-None
    /// convention).
    pub modifier: u64,
    pub width: u32,
    pub height: u32,
    /// NV12 = 2 entries used; unused trailing entries are zeroed and ignored.
    pub planes: [DmaBufPlane; 2],
    pub plane_count: u8,
}
```

```rust
pub enum GpuBufferHandle {
    // ...existing 7 variants unchanged...
    /// Linux DRM/GEM DMA-BUF export (VA-API's native Zero-Copy mechanism). Boxed — see
    /// this variant's owning ADR for why (`mediaway-decoder` `adr/linux/0003`).
    DmaBuf(Box<DmaBufDescriptor>),
}
```

**Consequence this ADR must flag, not hide:** boxing forces dropping `#[derive(Copy)]` from the
whole `GpuBufferHandle` enum (a `Box` field is never `Copy`) — a cross-cutting change to
`mediaway-common`, a workspace-shared type. `VideoFrame`/`VideoFrameStorage`
(`mediaway-common/src/frame.rs:10,27`) already derive only `Clone`, not `Copy`, so the enum they
embed losing `Copy` is not expected to break those two call sites. The FFI mirror type
(`crates/mediaway-ffi/src/common/gpu.rs::GpuBufferHandle`, a separate flat `#[repr(C)] Copy`
struct with `kind` + up to 2 `usize` + `u32` + `u64` fields) is fully decoupled from the Rust enum
already — it does not need to change shape to add a `DmaBuf` `kind` (`native_a`/`native_b` can
carry `fd0`/`fd1`, three new `u32`/`u64` fields cover `fourcc`/`modifier`/dims, though it cannot
represent 2 planes flatly and needs its own design pass — **out of scope for this ADR**, and only
relevant once an FFI consumer needs Linux GPU decode output). Every other current
`GpuBufferHandle` construction site in the workspace (`vulkan/zero_copy.rs`, Windows encode/
decode bridges) builds a fresh value per call rather than implicit-copying an existing one — this
ADR's read of those call sites found no reliance on `GpuBufferHandle: Copy`, but this was **not**
verified by an actual `cargo build --workspace` (no build tool available in this design-only
session) — flagged as a required first implementation step, not asserted as fact.

### Fd lifetime contract — reintroducing DPB `outstanding` tracking

ADR-0002 deliberately dropped `vulkan/dpb.rs`'s `outstanding`/`mark_outstanding`/
`clear_outstanding` bookkeeping when porting `Dpb` to VA-API
(`crates/mediaway-decoder/src/linux/vaapi/dpb.rs:11-16`), reasoning explicitly: *"this crate's
decode path always copies pixels into an owned `Bytes`... and never exposes a Zero-Copy GPU
handle... so there is no dangling-handle risk class to guard against"* — a conditional rationale,
not a permanent one. Once `ZeroCopyGpu` output exists, the risk class returns: if a DPB slot's
`Surface` is recycled (sliding-window eviction, `Dpb::insert`) for a **new** decode operation
while a caller still holds/reads memory referenced by a previously-exported `fd` for that same
slot, the driver's next `vaBeginPicture`/`vaRenderPicture` on that surface overwrites pixels the
consumer may still be sampling — a real tear/corruption race, structurally identical to the one
`vulkan/dpb.rs`'s `outstanding` guards against (`crate::vulkan::dpb::Dpb::mark_outstanding`/
`clear_outstanding`, cited in `vulkan/zero_copy.rs:17-24`). **Decision**: re-port
`outstanding`/`mark_outstanding`/`clear_outstanding`/`SlotOutstanding` into
`linux/vaapi/dpb.rs`, gated so `insert` refuses to recycle an outstanding slot — same "fail
loudly, never silently overwrite" contract as the Vulkan precedent, and the exact behavior
ADR-0002 said it would need "if a future stage adds a Zero-Copy output path" (implied by its own
conditional framing, not a literal quote).

Who calls `close(2)` on the exported fd is the second half of the contract. **Decision**: the
decoder, not the caller. Add a parallel `Vec<Option<OwnedFd>>` (indexed identically to
`Pipeline::surfaces`/`Dpb::slots`, mirroring `DpbSlot`'s own documented pattern of keeping "no
pixel data, no VA-API surface handle" in `DpbSlot` itself and instead storing those in a parallel,
identically-indexed vec — `dpb.rs:31-33`) holding the `OwnedFd` returned by `export_prime()` for
any slot currently exported. The `GpuBufferHandle::DmaBuf` handed to the caller carries the raw
fd **bits** (`NativeHandle`, non-owning, matching every other variant's "opaque bits, ownership
documented in prose" convention — see `gpu.rs`'s own `NativeHandle` doc). The decoder's internally
-held `OwnedFd` is what actually keeps the fd open and is what `close`s it — on `clear_outstanding`
(the next `push_packet`/`poll_frame`/`flush` call that would otherwise recycle this slot),
mirroring the exact validity window `vulkan::zero_copy`'s own module doc already documents for
`GpuBufferHandle::Vulkan` (`crate::VideoDecoder::poll_frame`'s documented handle-lifetime
contract). A caller that needs the fd to outlive that window **must `dup()` it themselves**
before the next poll/flush call — the same obligation any DMA-BUF consumer already has toward a
short-lived exported fd, stated explicitly in this crate's rustdoc, not left implicit.

### Module layout

```text
crates/mediaway-decoder/src/linux/vaapi/
  dmabuf.rs        — NEW. Codec-agnostic: `Surface<D>::export_prime()` → `DmaBufDescriptor` →
                     `GpuBufferHandle::DmaBuf` construction (`build_handle`, mirrors
                     `vulkan::zero_copy::build_handle`'s shape/doc style); NV12 plane-layout
                     validation (reject `num_objects > 2` / `num_layers`'s planes not matching
                     NV12's 2-plane expectation as `DecodeError::Backend`, never silently
                     truncated). No codec-specific (`H264*`) types — callable from a future
                     `hevc.rs`/`av1.rs`/`vp9.rs` in this same crate for free.
  dpb.rs           — CHANGED. Re-add `outstanding: Vec<bool>` + `mark_outstanding`/
                     `clear_outstanding`/`SlotOutstanding` (ported back from `vulkan/dpb.rs`,
                     reversing ADR-0002's deliberate drop for the reason stated above).
  h264.rs          — CHANGED. `open()`'s `config.output != CpuFramesOk` early-reject
                     (`h264.rs:77-80`) narrows to reject only unsupported preferences, not
                     `ZeroCopyGpu`; `poll_frame`'s per-picture output-assembly path branches on
                     `config.output` — `CpuFramesOk` keeps calling `copy_nv12_from_planes`
                     unchanged, `ZeroCopyGpu` calls `dmabuf::build_handle` + `Dpb::
                     mark_outstanding` instead, wraps the result as `VideoFrameStorage::
                     Gpu(GpuBufferHandle::DmaBuf(..))`. A parallel `exported_fds:
                     Vec<Option<OwnedFd>>` field added to `Pipeline`, indexed like `surfaces`.
```

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Model `GpuBufferHandle::DmaBuf` as a flat, non-boxed variant (2 `NativeHandle` fields, mirroring `Vulkan`) | Cannot represent NV12's 2-plane layout (offset/pitch per plane) or a DRM format modifier honestly — would force the *consumer* to re-derive plane geometry from width/height/fourcc alone, which is exactly the kind of silent, undocumented assumption `docs/spec/caveats-and-clarity.md` forbids for a costly/fragile path. |
| Keep `GpuBufferHandle` unboxed and accept the size-of inflation workspace-wide | Rejected per ZCA discipline (`docs/spec/zero-cost-abstractions.md`) — DMA-BUF export is already syscall-heavy (`vaExportSurfaceHandle` + fd dup per call), so one `Box` alloc on that path is a negligible addition, whereas inflating every platform's `VideoFrame` by ~4x is a real, avoidable regression for paths that never touch Linux. |
| Have the caller own/close the exported fd directly (no internal `OwnedFd`, no `outstanding` re-port) | Reopens exactly the dangling-handle risk class ADR-0002 named and the Vulkan precedent (`mark_outstanding`/`clear_outstanding`) already solved once in this same crate family — re-deriving a different, weaker contract here would be inconsistent and unreviewed. |
| Route decode Zero-Copy output through `GpuBufferHandle::Vulkan` by having this crate import the dma-buf into a `VkImage` itself (`VK_EXT_external_memory_dma_buf`) before returning | Would require this decoder crate to depend on `vulkanalia`/Vulkan device management purely to *re-export* a handle shape — real, unjustified scope growth (a decode backend should not need a full Vulkan device just to relabel its own output), and duplicates work a *consumer*-side adapter (a future `mediaway-wgpu` Linux HAL bridge) is the natural owner of, per `docs/spec/gpu-interop.md`'s "adapters are optional crates" rule. Rejected — export the OS-native primitive (DMA-BUF); let a consumer adapter do the import. |
| Design and implement a `mediaway-wgpu` Linux DMA-BUF import path in this same ADR, for a full round-trip demo | Investigated (`crates/mediaway/src/wgpu/` — `dx12.rs`/`dx12_decode.rs`, Windows-only today; `docs/ai/wiki/zero-copy/gpu-interop.md` confirms no Linux/Vulkan HAL bridge exists anywhere in this workspace yet). This ADR would be the **first DMA-BUF-aware code in the whole workspace**, with no existing consumer to round-trip into. Scoping a consumer-side import into this ADR would be speculative (no `mediaway-wgpu` Linux backend exists to receive it) — deferred to its own ADR once/if `mediaway-wgpu` grows a Linux Vulkan HAL bridge, mirroring how the Windows DX12 bridges each got their own ADR only once a concrete two-sided need existed. |

## Consequences

### Positive

- Reuses a real, already-safe `cros-libva` API (`export_prime`) — no new `unsafe` in this crate.
- Fd-close ownership stays inside the decoder, symmetric with the existing Vulkan Zero-Copy
  handle-lifetime contract callers already need to learn once.
- `dmabuf.rs` is codec-agnostic by construction — the next VA-API codec backend in this crate
  (HEVC/AV1/VP9, all still unimplemented for VA-API as of this ADR, see § Scope correction below)
  gets Zero-Copy output for free.

### Negative / Trade-offs

- `GpuBufferHandle` loses `Copy` workspace-wide (see § Common-crate change) — confirmed by a real
  `cargo check --workspace --all-features` (Windows) + WSL2 Linux-target check: **zero** call
  sites across the whole workspace (Windows/Apple/Android/Web included) relied on implicit-copy
  semantics for a bare `GpuBufferHandle` value; every existing construction site already builds a
  fresh value per call. The predicted "small ripple risk" did not materialize.
- No real consumer exists yet (see Alternatives) — this ADR's own deliverable, once implemented,
  is only independently verifiable via direct fd/plane inspection (see § Test plan), not an
  end-to-end pixel round-trip, until a Linux `mediaway-wgpu` bridge exists.
- Adds a second parallel per-slot vec (`exported_fds`) alongside `surfaces`/`dpb.slots` —
  three parallel, identically-indexed collections is more bookkeeping surface than today's two;
  acceptable given `DpbSlot`'s own established "no pixel/handle data in `DpbSlot` itself"
  convention, but worth a follow-up refactor note if a 4th parallel vec is ever proposed.

## Scope correction: this backend is H.264-only today, not 4 codecs

The task that produced this ADR assumed HEVC/AV1/VP9 VA-API decode already existed alongside
H.264. **Not true** — confirmed via `Glob` over `crates/mediaway-decoder/src/linux/vaapi/*.rs`
and `mod.rs` (`mod.rs:9-17`): only `h264.rs` exists; `mod.rs` declares `codec`, `dpb`, `h264`,
`nv12`, `pps`, `slice`, `sps` — no `hevc`/`av1`/`vp9` modules. The roadmap
(`crates/mediaway-decoder/docs/linux/roadmap.md` § Stage 4, "Multi-codec (deferred)") confirms
this is still-future work, unstarted. This ADR's "codec-agnostic module" decision is therefore
not "retrofit 4 existing codec files" but "design the shared plumbing once so the 3 codec
backends that do not exist yet inherit it for free when written" — a smaller, cleaner claim than
originally framed.

## Open questions

1. **Real `num_objects`/`num_layers` shape on real Intel/Mesa/AMD drivers for NV12** —
   `export_prime()`'s hardcoded `COMPOSED_LAYERS` flag *should* yield 1 object, but this was not
   confirmed against a real driver in this session (no VA-API hardware available, same standing
   limitation as ADR-0001/0002). `DmaBufDescriptor` caps at 2 objects / 2 planes as a documented
   scope cut, not a verified upper bound — must be revisited against real capture before landing.
2. **`fourcc` namespace equivalence** — this ADR assumes `VA_FOURCC_NV12` and `DRM_FORMAT_NV12`
   are numerically identical (both `fourcc_code('N','V','1','2')`, little-endian). Plausible and
   consistent with how VA-API's own fourcc space was originally aligned with V4L2/DRM, but not
   independently re-verified against a real bindgen dump of both constants in this session.
3. **Whether `vaSyncSurface` alone is a sufficient fence for a DMA-BUF consumer on a different
   GPU/API** — `Surface::sync()` (`surface.rs:264-267`) blocks until VA-API's own pending ops
   finish, but says nothing about whether the kernel's implicit dma-buf fence (reservation
   object) is signaled for a *different* driver/device importing the same buffer. This affects
   whether a future Vulkan/wgpu consumer needs its own explicit wait beyond "we called `sync()`
   before exporting" — flagged, not resolved; needs real multi-GPU-API hardware testing.
4. **`GpuBufferHandle` `Copy`-removal blast radius** — **resolved**: confirmed by a real
   `cargo check --workspace --all-features` (Windows) + WSL2 Linux-target build, zero real call
   sites needed a fix.
5. **FFI mirror shape for `DmaBuf`** — explicitly out of scope (§ Common-crate change); needs its
   own design once a concrete FFI/binding consumer exists.

## Test plan

- Unit: `dmabuf.rs`'s `DmaBufDescriptor` validation logic (plane/object-count rejection) —
  pure data, no VA-API calls, unit-testable without hardware (mirrors `dpb_tests.rs`'s existing
  no-device-needed pattern). **Implemented** (`dmabuf_tests.rs`), plus `dpb_tests.rs`'s new
  `outstanding`/`mark_outstanding`/`clear_outstanding` coverage.
- Hardware-gated, direct fd/plane inspection (soft-skip on `Display::open()` failure, same
  convention as `tests/vulkan/hardware_h264_decode.rs`): export a real decoded surface's DMA-BUF
  handle, then **directly inspect the fd** via `rustix`/`nix`-free raw syscalls already reachable
  through std (`fstat` on the `RawFd` to confirm it is open and non-zero size) plus a raw `mmap` +
  byte-pattern check against the plane offsets/pitches this ADR's descriptor claims — this is the
  "verified via direct fd/plane inspection, no consumer needed yet" deliverable named in this
  ADR's own scope, not a full GPU-framework round-trip. **Not added this pass**: this crate's
  `h264_tests.rs` has no existing real-`Display::open()`-plus-real-bitstream integration harness
  to extend (unlike `mediaway-encoder`'s `video_tests.rs`, which already builds/pushes synthetic
  frames against a real session) — building one from scratch for this ADR alone would be new,
  undue scope; deferred to whenever this crate grows that harness for another reason.
- Default `cargo test`/`cargo nextest` suite must still pass with zero VA-API hardware present
  (per workspace testing policy) — every hardware-touching test soft-skips. **Confirmed**: WSL2
  Ubuntu run, 0 failures, `Display::open()` honestly returns `Unsupported` (no `/dev/dri/
  renderD*`) in that environment.

## References

- [ADR-0001](0001-vaapi-h264-cpu-out.md), [ADR-0002](0002-vaapi-h264-p-slice-dpb.md)
- `crates/mediaway-decoder/src/vulkan/zero_copy.rs`, `crates/mediaway-decoder/src/vulkan/dpb.rs`
- `crates/mediaway-common/src/gpu.rs`, `crates/mediaway-common/src/frame.rs`
- `cros-libva` 0.0.13: `src/surface.rs` (`export_prime`, `ExternalBufferDescriptor`), `src/lib.rs`
- Upstream `va/va_drmcommon.h` (`intel/libva`) — `VADRMPRIMESurfaceDescriptor` fd-ownership note
- [`docs/spec/gpu-interop.md`](../../../../docs/spec/gpu-interop.md) (ADR-0005),
  [`docs/spec/zero-cost-abstractions.md`](../../../../docs/spec/zero-cost-abstractions.md)
  (ADR-0009), [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md)
  (ADR-0006)
- Companion: `mediaway-encoder` `adr/linux/0006-vaapi-dmabuf-zero-copy-input.md` (import
  direction, same underlying mechanism)

ADRs are **English**. Numbering is local to this `adr/` folder.
