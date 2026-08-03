# ADR-0001: VA-API via `cros-libva`, H.264 CPU-upload encode

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder-linux`

## Context

Linux hardware encode needs VA-API (`libva`) bindings. Options considered:

1. Hand-written `bindgen` FFI directly in this crate (own `build.rs` + `libva-wrapper.h`
   against system `va.h`/`va_drm.h`/`va_drmcommon.h`, via `pkg-config`).
2. [`cros-libva`](https://crates.io/crates/cros-libva) (crates.io, BSD-3-Clause) — ChromiumOS's
   safe libva wrapper, also used standalone (no ChromeOS-specific deps).

Precedent: `mediaway-encoder-windows` ADR-0002 depends on the official `windows` projection
rather than hand-rolling COM bindings, for the same reason — the surface is large and an
existing, permissively-licensed, actively maintained crate tracks it better than a from-scratch
`build.rs`.

### `cros-libva` review (`docs/conventions/deps-policy.md` checklist)

| Question | Answer |
|----------|--------|
| Need | Real: VA-API session (`Display`/`Config`/`Context`/`Surface`) + H.264 encode buffer types are a large, fiddly FFI surface (bitfield unions, per-codec buffer structs) — not a 20–50 line local win. |
| License | BSD-3-Clause — already on `deny.toml`'s allow-list; no ADR-blocking issue. |
| Transitive license | `thiserror` (v1, its own internal pin — does not leak into our public API, we use workspace `thiserror` 2.0 for `EncodeError`), `bitflags`, `log`; build-deps `bindgen`, `pkg-config`, `regex`. All permissive, no GPL/LGPL/copyleft surprise. |
| Maintenance | Actively used by ChromeOS/`cros-codecs` (real production decode **and** encode); repo has recent commits/PRs/issues; crates.io shows real download volume. |
| API stability | **0.0.x** — every release is semver-exact per Cargo's `0.0.z` handling (effectively pinned per patch). Documented risk below. |
| Alternatives | Hand-written `bindgen`: more control, but reinvents `Display`/`Config`/`Context`/`Surface`/`Picture` lifetime and typestate management that `cros-libva` already provides safely (`Picture<S, T>` typestate enforces `vaBeginPicture → vaRenderPicture → vaEndPicture → vaSyncSurface` ordering at compile time — this is a genuine safety/ZCA win, not just convenience). |
| Cost | Build-time coupling to system `libva-dev`/`libva`/`libva-drm` (headers + `.so`) via the dep's own `pkg-config` + `bindgen` build script. Runtime coupling to `libva.so`/`libva-drm.so`. Confirmed via WSL: `libva-dev` 2.20.0 (libva 1.20.0 API) builds cleanly. |
| Unsafe surface | All `unsafe` FFI calls live inside `cros-libva`, not this crate — this crate's own code stays on the safe wrapper API (`Display`, `Config`, `Context`, `Surface`, `Picture`, `Buffer`, `EncCodedBuffer`, `MappedCodedBuffer`). No `unsafe` block is written in this crate's own source beyond what raw struct field access on `#[non_exhaustive]`-free bindgen types incidentally requires (none, in practice — see Decision). |

## Decision

> Depend on **`cros-libva` 0.0.13** (workspace-pinned, exact per `0.0.z` semver) as a
> **`cfg(target_os = "linux")`** target dependency, mirroring how `mediaway-encoder-windows`
> gates the `windows` crate under `cfg(windows)` — so `cargo check --workspace` on a
> non-Linux host never invokes `cros-libva`'s `pkg-config`/`bindgen` build script.

- Use the crate's **safe** layer end to end: `Display::open()` (DRM render-node
  auto-detect — tries `/dev/dri/renderD128..191` in order via its `DrmDeviceIterator`,
  wrapping `vaGetDisplayDRM` + `vaInitialize`), `Display::create_config` (`vaCreateConfig`),
  `Display::create_surfaces` (`vaCreateSurfaces`), `Display::create_context`
  (`vaCreateContext`), the typestate `Picture<PictureNew|Begin|Render|End|Sync, _>`
  (`vaBeginPicture`/`vaRenderPicture`/`vaEndPicture`/`vaSyncSurface`), `Context::create_buffer`
  / `Context::create_enc_coded` (`vaCreateBuffer`), and `MappedCodedBuffer` (`vaMapBuffer` /
  `vaUnmapBuffer`) for reading back the encoded Annex-B bitstream.
- CPU NV12 upload (`upload_cpu_nv12`, matching the Windows crate's naming/cost-disclosure
  convention) uses `Image::create_from` (`vaCreateImage` + `vaGetImage`) then writes our NV12
  bytes into the mapped image respecting `VAImage::pitches`/`offsets`; dropping the `Image`
  triggers `vaPutImage` (upload) automatically — this is a genuine CPU→driver copy, documented
  as costly like the Windows `upload_cpu_nv12`.
- H.264 encode buffers use `cros_libva::buffer::h264`'s safe constructors
  (`EncSequenceParameterBufferH264::new`, `EncPictureParameterBufferH264::new`,
  `EncSliceParameterBufferH264::new`, `H264EncSeqFields::new`, `H264EncPicFields::new`,
  `PictureH264::new`) — field names/order verified against the real `va_enc_h264.h`
  (`libva-dev` 2.20.0, WSL) rather than guessed from memory.
- Profile: **`VAProfileH264ConstrainedBaseline`**, entrypoint **`VAEntrypointEncSlice`**,
  rate control **`VA_RC_CQP`** (fixed QP — simplest RC mode, avoids needing
  `VAEncMiscParameterRateControl`/`FrameRate` buffers for this stage).
- **Every pushed frame is encoded as an independent IDR intra frame** (`frame_num` = 0,
  `idr_pic_flag` = 1, empty reference-picture lists every time). No P/B-frame reference
  picture management, no GOP structure. This keeps scope to "H.264 baseline CPU-upload,
  single profile" as directed — see Consequences.

## ⚠️ Zero real-hardware verification in this session

**This crate was written and is compile-verified on Linux (WSL2 Ubuntu 24.04 via
`cargo check` / `cargo test` / `cargo clippy`, real `libva-dev` 1.20.0 headers/bindgen
output), but has had `Display::open()` / `vaInitialize` invoked against exactly zero
real VA-API hardware.**

- This Windows dev box cannot run Linux VA-API natively.
- The WSL2 Ubuntu instance available in this session has **broken VA-API**
  (`vainfo` segfaults) and only software `llvmpipe` Vulkan — no real GPU exposed to WSL.
  Fixing WSL GPU passthrough was explicitly declined by the user for this session in favor
  of "ADR + scaffolding only."
- Every hardware-touching code path (`Display::open()`/`open_drm_display`, `Config`,
  `Context`, `Surface`, `Picture` begin/render/end/sync, `MappedCodedBuffer`) is written to
  be **correct enough to run on a real Linux + VA-API machine**, grounded in the actual
  `va.h`/`va_enc_h264.h` struct layouts (not paraphrased from memory) and the actual
  `cros-libva` safe-wrapper source — but **none of it has been observed to succeed against
  real `vaInitialize`, a real driver, or a real encode session**. Treat every VA-API call
  path in this crate as **unverified until run on real hardware** (Intel iHD / Mesa VA-API
  / AMD, per the project's platform-order Stage 3).
- The honest-skip test (`vaapi_open_or_skip_without_hw`, mirroring the Windows
  `_or_skip_without_hw` convention) is **expected to skip** in this session and in any CI
  environment without a real `/dev/dri/renderD*` VA-API device. A skip here is correct,
  not a bug.

## Sans-IO / ZCA shape

Mirrors `mediaway-encoder-windows`'s session shape (transform/sample ≈ context/picture),
with real differences called out:

| Windows (WMF) | Linux (VA-API) | Difference |
|---------------|-----------------|------------|
| `IMFTransform` (single object, configure via media types) | `Config` + `Context` (two objects: codec/profile capability vs. bound session) | VA-API separates "what the driver can do" from "the session using it" |
| `IMFSample` (push one at a time, async event pump for HW MFTs) | `Picture<S, T>` **typestate** (`PictureNew → Begin → Render → End → Sync`) | VA-API's ordering is enforced at the **type level** in `cros-libva`, not just by convention — no dedicated event pump needed since this crate's path is fully synchronous per frame (`vaSyncSurface` blocks) |
| `MFCreateMemoryBuffer` + `Lock`/`Unlock` (CPU upload) | `Image::create_from` + `vaPutImage` on `Drop` (CPU upload) | Same cost class (`upload_cpu_nv12`), different driver call shape |
| Output via `ProcessOutput` buffer loop | `EncCodedBuffer` + `MappedCodedBuffer` segment iteration | VA-API coded output is a mapped buffer segment list, not a pull-style transform output queue |
| DX11 Zero-Copy (`GpuBufferHandle::DirectX11`, hardware MFT + DXGI device manager) | **Not implemented this stage** — would map to `GpuBufferHandle::Vulkan` / DMA-BUF surface import (`VASurfaceAttribExternalBuffers` / `VADRMPRIMESurfaceDescriptor`) | Explicitly deferred (see roadmap) |

No `Box<dyn _>` / `dyn Trait` introduced: `LinuxVideoEncoder` wraps a concrete
`vaapi::VaapiVideoEncoder` behind `Option` (closed-after-move sentinel), exactly like
`WindowsVideoEncoder` wraps `wmf::WmfVideoEncoder`. `PictureH264` "no reference" placeholders
are built via `std::array::from_fn` (no heap `Vec` needed for the fixed 16/32-entry arrays
`VAPictureH264` requires).

## Scope (this stage)

**In:**

- H.264 Constrained Baseline, CQP rate control, CPU NV12 upload only
  (`VideoInputPreference::CpuUploadOk`).
- Every frame independent IDR (no GOP / no P-frames / no reference management).
- DRM render-node auto-detect (`Display::open()`), honest `EncodeError::Backend` /
  `EncodeError::Unsupported` when no device/driver is available.

**Out (deferred, tracked in `docs/roadmap.md`):**

- Zero-Copy DMA-BUF surface import (`VideoInputPreference::ZeroCopyGpu`) — returns
  `EncodeError::Unsupported`.
- HEVC / AV1 / VP9 encode (VA-API supports them; this crate does not yet).
- Vulkan Video encode path (README/roadmap mentions VA-API **and/or** Vulkan Video for
  Linux — this ADR picks VA-API first; Vulkan Video would need its own ADR).
- P/B-frame GOP structure, VBR/CBR rate control, VUI/cropping SPS fields.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Hand-written `bindgen` FFI in this crate | Reinvents `Display`/`Config`/`Context`/`Surface`/`Picture` safety and typestate ordering that `cros-libva` already provides; larger unsafe surface owned directly by this crate instead of an upstream-maintained wrapper. |
| Vulkan Video encode instead of VA-API | Less mature driver support workspace-wide at this stage; VA-API is the more established Linux HW encode path today. Left as a future ADR per roadmap. |
| `libva-sys`-style raw sys crate | No such actively maintained crate found on crates.io at review time (search turned up only `cros-libva`, which already bundles the raw bindings **and** a safe layer). |
| Depend on system `ffmpeg`/`vaapi` filters | Forbidden — FFmpeg stays a test/dev oracle only (ADR-0002), never a product dependency. |

## Consequences

### Positive

- Small, real unsafe surface (none written directly in this crate; all FFI unsafety lives in
  `cros-libva`).
- Typestate `Picture` flow makes an invalid VA-API call order a compile error, not a runtime
  bug class.
- Structural parity with the Windows backend eases future `auto`-style cross-platform
  dispatch work.

### Negative / Trade-offs

- `cros-libva` 0.0.x: no semver stability guarantee yet; a future minor bump could break this
  crate and require re-review.
- Build-time hard dependency on system `libva-dev` (+ `libva-drm`) on any Linux build of this
  crate — acceptable per this crate's own platform scope (never required for
  Windows/Web/other builds, per the `cfg(target_os = "linux")` gate).
- All-IDR encoding is bitrate-inefficient (no inter prediction) — acceptable for this stage's
  narrow scope, not for a production streaming path.
- **Zero hardware verification** (see caveat above) — real-world correctness is unproven
  until run on actual VA-API hardware.

## References

- [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md)
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md)
- `mediaway-encoder-windows` ADR-0002 (`windows` crate precedent), ADR-0001 (encode surface)
- [`cros-libva` on crates.io](https://crates.io/crates/cros-libva) ·
  [GitHub](https://github.com/chromeos/cros-libva) (BSD-3-Clause)
- `docs/roadmap.md` § Linux (VA-API and/or Vulkan Video)
