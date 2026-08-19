# ADR-0003: VA-API DMA-BUF Zero-Copy encode input

- **Status**: Accepted (implemented — WSL2 + Windows compile/clippy/test-verified; zero real
  VA-API hardware verification, see § Open questions)
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`src/linux/vaapi/`)

## Context

[ADR-0001](0001-vaapi-cros-libva-h264-cpu-upload.md) scoped this backend to CPU NV12 upload only.
Confirmed still true: `crates/mediaway-encoder/src/linux/vaapi/video.rs:97-104`:

```rust
pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
    validate(config)?;
    match config.input {
        VideoInputPreference::CpuUploadOk => Self::open_cpu(config),
        // DMA-BUF Zero-Copy surface import is deferred — see ADR-0001 § Scope / roadmap.
        _ => Err(EncodeError::Unsupported),
    }
}
```

`VideoInputPreference::ZeroCopyGpu` is rejected outright at `open()`. The roadmap
(`crates/mediaway-encoder/docs/linux/roadmap.md` § Stage 3) names the target as
"`GpuBufferHandle::Vulkan` interop path" — **corrected by this ADR's companion**,
`mediaway-decoder`'s `adr/linux/0003-vaapi-dmabuf-zero-copy-output.md` (read first; this ADR
reuses its `GpuBufferHandle::DmaBuf` design and does not re-derive it). This ADR designs the
**import** direction: accepting a caller-supplied `VideoFrameStorage::Gpu(GpuBufferHandle::
DmaBuf(..))` frame (e.g. produced by a GPU compositor, `mediaway-device`'s screen capture, or a
render pipeline) as this encoder's *input* surface without a CPU upload.

### `cros-libva` exposes an import-side trait — genuinely supports this direction

`cros-libva-0.0.13/src/surface.rs:66-96`:

```rust
pub trait ExternalBufferDescriptor {
    const MEMORY_TYPE: MemoryType;                    // → MemoryType::DrmPrime2 (== 0x40000000)
    type DescriptorAttribute: SurfaceExternalDescriptor;
    fn va_surface_attribute(&mut self) -> Self::DescriptorAttribute;
}

impl<T> SurfaceMemoryDescriptor for T
where T: ExternalBufferDescriptor, T::DescriptorAttribute: 'static
{
    fn add_attrs(&mut self, attrs: &mut Vec<bindings::VASurfaceAttrib>) -> Option<Box<dyn Any>> {
        // pushes VASurfaceAttribMemoryType + VASurfaceAttribExternalBufferDescriptor(desc)
    }
}
```

`bindings::VADRMPRIMESurfaceDescriptor` already implements `SurfaceExternalDescriptor`
(`surface.rs:63-64`), so a new local type implementing `ExternalBufferDescriptor` with
`MEMORY_TYPE = MemoryType::DrmPrime2` and `DescriptorAttribute = bindings::
VADRMPRIMESurfaceDescriptor` is enough to make `Display::create_surfaces` accept an
externally-supplied DMA-BUF (`display.rs:265-283`, `create_surfaces<D: SurfaceMemoryDescriptor>`
— already generic; no `cros-libva` change needed).

### Type-parameter shape: `Surface<D>` genericity is per-call, not per-session — no pool restructuring

This backend's existing reference/reconstruction surface pool is `Vec<Option<Surface<()>>>`
(`video.rs:66`, `SURFACE_POOL_SIZE` entries, driver-allocated, created once at `open_cpu` via
`vec![(); SURFACE_POOL_SIZE]`, `video.rs:135-144`). Checked `cros-libva`'s `Picture<S, T>`
(`picture.rs:105`, `T: Borrow<Surface<D>>` bound per-method, e.g. `picture.rs:113-115`,
`152-154`) — the surface type parameter is chosen **per `Picture` instance**, not baked into one
session-wide type. This backend already treats "the surface being rendered into this call" (the
source picture) and "the DPB pool of reference surfaces" (ADR-0002's `GopState`/`surfaces` pool)
as separate objects at the call-site level (`encode_one`'s `surface: Surface<()>` parameter,
`video.rs:363`, is distinct from `reference`'s pool-indexed surface). **Consequence**: a
DMA-BUF-imported `Surface<DmaBufImportDescriptor>` for the *current input frame* can coexist with
the existing `Surface<()>` reference pool without restructuring it — only `encode_one`'s (and
`push_frame`'s) source-surface handling needs to branch on `VideoFrameStorage`, not the pool's
element type. This is a genuinely **thin, localized addition**, not a pool-wide generic
rewrite — confirmed by reading the actual type signatures, not assumed.

### fd ownership on import — unconfirmed, flagged honestly

The companion decoder ADR cites `va_drmcommon.h`'s explicit fd-ownership note for the **export**
direction ("backend driver will not close the file descriptor"). No equivalent explicit text was
found for the **import** direction (`vaCreateSurfaces` + `VASurfaceAttribExternalBuffers` /
`DRM_PRIME_2`) in this session. General DRM PRIME/GEM-import semantics (`drmPrimeFDToHandle`)
typically dup the fd into a kernel-owned GEM reference at import time, meaning a caller could
close its own fd immediately after a successful `vaCreateSurfaces` — but this is an inference
from general DRM convention, **not a cited libva guarantee**. See § Open questions;
this ADR's decision defensively dups the caller's fd before handing it to `cros-libva` rather
than assuming the driver does so.

## Decision

> **Implemented** (originally proposed as design-only; a follow-up pass in this same PR
> implemented and WSL2/Windows-verified it — see § Test plan for the real commands run). A
> Zero-Copy encode input path for
> `VideoInputPreference::ZeroCopyGpu`, reusing `mediaway-common::GpuBufferHandle::DmaBuf` (see
> the decoder companion ADR) as the caller-facing type, that:
>
> 1. Adds a new local `DmaBufImportDescriptor` type implementing `cros_libva::
>    ExternalBufferDescriptor`, built from the caller's `GpuBufferHandle::DmaBuf` fields.
> 2. Creates a **single, per-call** `Surface<DmaBufImportDescriptor>` via `Display::
>    create_surfaces`, separate from the existing `Vec<Option<Surface<()>>>` reference pool —
>    no pool restructuring (see § Context above).
> 3. Defensively `dup()`s the caller-supplied fd before construction, closing the encoder's own
>    duplicate once `create_surfaces` returns (successfully or not) — see § fd ownership.
> 4. Adds one new codec-agnostic module, `linux/vaapi/dmabuf.rs`, mirroring the decoder
>    companion's module of the same name/purpose but for the opposite (import) direction.

### Module layout

```text
crates/mediaway-encoder/src/linux/vaapi/
  dmabuf.rs        — NEW. Codec-agnostic: `GpuBufferHandle::DmaBuf` → `DmaBufImportDescriptor`
                     (implements `cros_libva::ExternalBufferDescriptor`) → single-surface
                     `Display::create_surfaces` call. Validates plane/object count against the
                     NV12 shape this backend's `VA_FOURCC_NV12`/`VA_RT_FORMAT_YUV420` session
                     already assumes (`video.rs:137-138`) — mismatched fourcc/plane count is
                     `EncodeError::InvalidInput`, never silently reinterpreted. Owns the
                     defensive `dup()`/`close()` pair around `create_surfaces`. No codec-specific
                     types — reusable by a future `hevc.rs`/`vp9.rs` VA-API backend for free.
  video.rs         — CHANGED. `open()`'s `_ => Err(EncodeError::Unsupported)` arm
                     (`video.rs:100-102`) gains a `VideoInputPreference::ZeroCopyGpu => Ok(..)`
                     branch (still validates `config` the same way `open_cpu` does — profile,
                     capability probe, context/reference-pool creation are unchanged; only the
                     *source* surface's provenance differs per call). `push_frame`'s existing
                     `VideoFrameStorage::Cpu { data }` branch (currently the only branch — CPU
                     upload is the sole path today) gains a sibling `VideoFrameStorage::
                     Gpu(GpuBufferHandle::DmaBuf(desc))` branch calling `dmabuf::import_surface`
                     instead of `upload_cpu_nv12` (`video.rs:349`, `407-...`). A caller that
                     opened with `ZeroCopyGpu` but pushes a `Cpu` frame (or vice versa) gets
                     `EncodeError::InvalidInput`, not a silent fallback — matches this backend's
                     existing "never a silent slow default" posture.
```

### Why no `outstanding`/lifetime bookkeeping is needed here (asymmetry vs. decode)

Decode's Zero-Copy output handle can be held by a caller indefinitely (rendering, buffering) —
hence ADR-0003 (decoder)'s DPB `outstanding` re-port. Encode's imported input surface has a
**bounded, synchronous lifetime**: `vaBeginPicture → vaRenderPicture → vaEndPicture →
vaSyncSurface` (this backend's existing `Picture<S, T>` typestate chain, unchanged by this ADR)
fully consumes the source pixels into the bitstream before `encode_one` returns. There is no
"the driver might reuse this while a caller still needs it" risk on the input side — the imported
`Surface<DmaBufImportDescriptor>` is not pooled or recycled by this backend at all; it is created
fresh per `push_frame` call and dropped (`vaDestroySurfaces` via `Surface`'s `Drop`,
`surface.rs:422-427`) once `encode_one` returns. This genuine, cited structural asymmetry is why
the encoder side needs materially less new bookkeeping than the decoder side, despite sharing the
same underlying DMA-BUF mechanism.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Fold the imported surface into the existing `SURFACE_POOL_SIZE` pool (make the whole pool `Surface<DmaBufImportDescriptor>`-typed when Zero-Copy mode is active) | Would require every reference/reconstruction surface (driver-allocated, reused across many pictures for GOP prediction — ADR-0002) to also be DMA-BUF-imported, which makes no sense (those are never externally supplied); rejected once `Picture<S,T>`'s actual per-call genericity was confirmed (see § Context), which removes the only reason to consider this. |
| Trust the driver to dup the imported fd internally; do not defensively `dup()` on our side | Plausible per general DRM PRIME convention, but **not cited** for this exact libva code path (§ fd ownership) — given a leaked/double-closed fd is a real, silent-until-it-isn't bug class, the defensive `dup()` costs one extra syscall per encoded frame and removes the ambiguity entirely. Chosen as the safe default; flagged as removable once real driver behavior is confirmed (see Open questions). |
| Share one `dmabuf.rs` module (or a new shared crate) between `mediaway-decoder` and `mediaway-encoder` | Investigated: the two crates have no dependency relationship and no existing shared VA-API helper crate (`Cargo.toml` review — neither depends on the other; each pulls `cros-libva` independently). The logic is not even identical (export vs. import are different `cros-libva` call shapes: `Surface::export_prime()` vs. implementing `ExternalBufferDescriptor` + `create_surfaces`). Matches this workspace's own established precedent of **porting**, not sharing via a new crate, for structurally-similar-but-direction-different logic (`vaapi/dpb.rs` was *ported* from `vulkan/dpb.rs`, not extracted into a shared crate; `vaapi/gop.rs` likewise from `vulkan/h264_gop.rs`). A shared crate would be a real, undue new dependency-graph edge for ~100-150 lines per side that legitimately differ. Rejected — each crate gets its own `dmabuf.rs`. |

## Consequences

### Positive

- No pool-wide type change — confirmed via `Picture<S, T>`'s real per-call genericity, not
  assumed; small, reviewable diff once implemented.
- Reuses the decoder companion's `GpuBufferHandle::DmaBuf` shape — one caller-facing contract for
  both directions, not two competing designs.
- Encode-input Zero-Copy needs no new fd-lifetime bookkeeping beyond a single defensive `dup()`/
  `close()` pair — genuinely simpler than the decode-output direction, for a cited structural
  reason (bounded synchronous consumption).

### Negative / Trade-offs

- Depended on the decoder-side ADR's `GpuBufferHandle::DmaBuf` design landing first (or
  concurrently) in `mediaway-common` — a real ordering dependency between the two ADRs, resolved
  by implementing both in the same pass.
- The defensive `dup()` is an extra syscall per Zero-Copy-input encoded frame until real driver
  behavior is confirmed to make it provably unnecessary (Open questions).
- Still H.264-only in this crate (see § Scope correction) — same caveat as the decoder side.

### Implementation note: `encode_one` genericity was a real, non-optional change

The design-only pass above did not spell out that `VaapiVideoEncoder::encode_one` (which drives
the `Picture<S, T>` typestate chain) is hard-typed to `Surface<()>` in the pre-ADR-0003 code.
Making the DMA-BUF-imported `Surface<DmaBufImportDescriptor>` flow through the same method
required generalizing it to `encode_one<D: SurfaceMemoryDescriptor>(surface: Surface<D>, ...)` and
threading `D` through `Picture::new::<D>`/`.begin::<D>()`/`.sync::<D>()` — confirmed to typecheck
because `cros-libva`'s own bounds (`T: Borrow<Surface<D>>`, satisfied trivially by `Surface<D>:
Borrow<Surface<D>>`) impose no extra constraint. `push_frame` itself was split into a dispatcher
(`push_frame`) plus `push_frame_cpu`/`push_frame_dmabuf`, sharing a `resolve_reference` helper —
a real, if mechanical, refactor this ADR's own sketch did not size correctly in advance.

## Scope correction: this backend is H.264-only today, not 3 codecs

Confirmed via `Glob` over `crates/mediaway-encoder/src/linux/vaapi/*.rs` and `mod.rs`
(`mod.rs:14-18`): only `codec.rs`, `gop.rs`, `video.rs` exist — no `hevc.rs`/`vp9.rs`. The
roadmap (`crates/mediaway-encoder/docs/linux/roadmap.md` § Stage 4, "Multi-codec (deferred)")
confirms HEVC/VP9 VA-API encode is unstarted. Same correction as the decoder companion ADR: this
ADR's "codec-agnostic module" framing means "designed once for backends that do not exist yet,"
not "retrofit existing codec files."

## Open questions

1. **Does `vaCreateSurfaces` with `DRM_PRIME_2` import dup the fd internally, or hold the exact
   fd it was given for the surface's lifetime?** Not confirmed by a cited libva guarantee in this
   session (§ fd ownership). Determines whether the defensive `dup()`/`close()` this ADR proposes
   is load-bearing or purely precautionary. Needs a real driver + either header text with an
   explicit statement or empirical `strace` confirmation.
2. **Format/modifier negotiation** — does the caller's DMA-BUF (produced by an arbitrary GPU
   pipeline) always carry a modifier this backend's encode session can accept, or can
   `vaCreateSurfaces` reject an incompatible tiling layout? Not tested against real hardware;
   the honest behavior is `EncodeError::Backend` on rejection, never a silent re-linearize.
3. **Same open questions #1/#2 from the decoder companion ADR** (real object/layer/plane shape on
   real drivers; `VA_FOURCC_NV12` vs. `DRM_FORMAT_NV12` numeric equivalence) apply symmetrically
   here and are not re-derived.

## Test plan

- Unit: `DmaBufImportDescriptor`'s `va_surface_attribute()` construction from a synthetic
  `DmaBufDescriptor` (plane math, no real fd needed for the pure-data assembly step).
  **Implemented** (`dmabuf_tests.rs`) — also covers `dup_from_native` against a real (but
  content-irrelevant, `/dev/null`) fd, and `open_zero_copy_gpu_no_longer_unconditionally_unsupported`
  in `lib_tests.rs` (a pre-existing test asserting the old unconditional-`Unsupported` rejection
  had to be corrected — a real regression this implementation pass surfaced and fixed).
- Hardware-gated (soft-skip on `Display::open()` failure): construct a real DMA-BUF via a
  software path this session can actually produce (e.g. a `memfd`/`udmabuf`-backed test buffer,
  or — more realistically — export one from this same crate's own **decoder** companion, giving a
  real decode-export → encode-import round trip entirely inside this workspace without needing a
  third-party GPU compositor). **Not added this pass** — no VA-API hardware was available to
  exercise or even smoke-test such a fixture in this session (same standing gap named throughout
  both companion ADRs); deferred to whoever first runs this workspace against real VA-API
  hardware, per both ADRs' own "achievable, honest deliverable" scoping.
- Default `cargo test`/`cargo nextest` suite must still pass with zero VA-API hardware present.
  **Confirmed**: WSL2 Ubuntu run (`mediaway-encoder`/`mediaway-decoder`/`mediaway-common`,
  `--all-features`), 0 failures.

## References

- Companion: `mediaway-decoder` `adr/linux/0003-vaapi-dmabuf-zero-copy-output.md` (defines
  `GpuBufferHandle::DmaBuf`/`DmaBufDescriptor` — read first, not re-derived here)
- [ADR-0001](0001-vaapi-cros-libva-h264-cpu-upload.md), [ADR-0002](0002-vaapi-h264-p-frame-gop.md)
- `cros-libva` 0.0.13: `src/surface.rs` (`ExternalBufferDescriptor`, `SurfaceMemoryDescriptor`),
  `src/picture.rs` (`Picture<S, T>` genericity), `src/display.rs` (`create_surfaces`)
- [`docs/spec/gpu-interop.md`](../../../../docs/spec/gpu-interop.md) (ADR-0005),
  [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) (ADR-0006)

ADRs are **English**. Numbering is local to this `adr/` folder.
