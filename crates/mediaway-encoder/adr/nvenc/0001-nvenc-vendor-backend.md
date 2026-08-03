# ADR-0001: NVENC direct vendor backend (GPU · by vendor)

- **Status**: Proposed
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder-nvenc` (new)

## Context

Root README's "GPU — by vendor" table lists NVIDIA H.264/HEVC/AV1 as 🛠️ (planned).
This is a separate axis from `mediaway-encoder-windows`'s existing
`Os::Gpu::GraphicsApi` backend (Media Foundation): WMF's hardware MFT layer may
internally dispatch to NVENC silicon, but that routing is opaque and driver-decided,
not a path Mediaway controls or can rely on.

Two concrete, already-documented facts motivate a *direct* NVENC path rather than
treating WMF as "good enough":

1. Wiki [`platform/windows-encode.md`](../../../docs/ai/wiki/platform/windows-encode.md)
   records a real ad-hoc box where **neither an RTX 4090 nor an Intel UHD 770**
   registered a working Media Foundation **encode** HW MFT for H.264 — NVENC
   silicon exists but the OS layer simply doesn't expose it as an
   `IMFTransform` there. A direct `nvEncodeAPI64.dll` call bypasses that
   enumeration gap entirely.
2. `mediaway-encoder-windows` ADR-0006 documents that D3D12 apps cannot feed WMF's
   HW MFT natively (`D3D11On12` → `MF_E_UNSUPPORTED_D3D_TYPE`) and must go through
   `D3d12SharedEncodeBridge`, an explicit **GpuCopy** (one VRAM copy per frame).
   NVIDIA's own NVENC API has first-class, fence-based D3D12 input (SDK ≥ 11.1,
   2021) with **no copy** — this is the concrete perf/feature case for a vendor
   backend, not just "maybe faster."

`mediaway-encoder` ADR-0004 already types this axis:
`EncodeMode::Os::Gpu::VendorHw`, a sibling of `GraphicsApi`, explicitly **not**
Auto's default #1 ("same silicon often underlies WMF HW MFT and NVENC; VendorHw ≠
automatic win"). This ADR only decides the `VendorHw` *backend* shape for NVIDIA;
it does not change Auto's ordering.

A real NVIDIA GeForce RTX 4090 (driver 32.0.15.9579) is available — genuine
target hardware exists for eventual verification, though (per point 1 above) GPU
presence alone does not guarantee any given path works; it must still be probed
and honestly reported.

## Research — Rust NVENC bindings candidates

The real constraint driving this research: NVIDIA does not ship an official
`.lib`/`.so` import target for `nvEncodeAPI64.dll` / `libnvidia-encode.so.1`
outside the full (registration-gated) Video Codec SDK download. The standard,
vendor-documented integration pattern (used by OBS, x264's `nvenc` plugin, etc.)
is: dynamically load the driver-provided library at runtime (`LoadLibrary`/
`dlopen`), resolve `NvEncodeAPICreateInstance`, and call through the returned
function-pointer table. `nvEncodeAPI.h` itself carries **its own separate,
fully permissive (MIT-style) per-file copyright notice** distinct from the rest
of the SDK's EULA — NVIDIA explicitly allows that one header to be copied/
redistributed. That is the legal basis every surveyed crate below relies on.

| Crate | License | Header / SDK story | Maintenance | Windows + D3D | Verdict |
|---|---|---|---|---|---|
| `nvenc-sys` ([legion-labs](https://github.com/legion-labs/nvenc-sys)) | MIT OR Apache-2.0 | Vendors NVIDIA's own `nvEncodeAPI.h` under `nvenc/`; bindings **pre-generated** via bindgen and checked in — no CUDA Toolkit / Video Codec SDK download needed to build. No `#[link]` anywhere; `NvEncodeAPICreateInstance` / `NvEncodeAPIGetMaxSupportedVersion` are plain `unsafe extern "C" fn` **type aliases** meant to be resolved via `GetProcAddress`/`dlsym` at runtime — exactly the dynamic-load shape required. | 6 commits total, **last touched 2022-04-26** — effectively unmaintained; 3 open issues. `libloading` is a **dev-dependency only** upstream (tests), not wired into a runtime loader for consumers. | Structs cover Windows + Linux (bindgen'd from the real header); consumer must write the loader + session logic itself (it is a `-sys` crate, by design). | **Best-fit shape, stale upstream — vendor/fork rather than trust releases.** |
| `nvenc` ([AlsoSylv](https://github.com/AlsoSylv/nvenc)) | MIT (+ standard codec-patent disclaimer boilerplate; does not affect our license graph) | **Hand-written** (not bindgen'd) structs/enums/function-table — a clean-room reimplementation of the ABI, sidestepping the "did we copy NVIDIA's header correctly" question by not copying it. Ships its own `libloading`-based dynamic loader (`NVENCLibrary` / `nvenc_init()`) as a real runtime dependency, not a dev-only test helper. | v0.1.0, ~4 commits, published a few months before this research (~2026-02). `rust-version = "1.85.1"`, `edition = "2024"` — matches this workspace's pins exactly. 1 open issue. | Real `[target.'cfg(windows)']` dep on `windows = "0.62"` (same version this workspace pins) with `Win32_Graphics_Direct3D11`; `open_dx(device: &impl Interface)` session-open path is generic over any COM interface, so it *can* take `ID3D11Device`. **Gap:** its `NVencDeviceType` enum only has `DirectX` / `Cuda` / `OpenGL` — no `D3D12` device type or fence structs yet; README/example is Linux/GLX-focused even though the Windows code path exists. | **Closest in dependency hygiene (matches our `windows`/edition/MSRV exactly) but pre-1.0 and missing D3D12 — a watch candidate, not yet a default dependency.** |
| `nvidia-video-codec-sdk` (crates.io; ViliamVadocz / rust-av lineage) | MIT | `build.rs` searches for `nvEncodeAPI.lib` / `nvcuvid.lib` (Windows) or `libnvidia-encode.so` / `libnvcuvid.so` (Linux) via `NVIDIA_VIDEO_CODEC_SDK_PATH` / `CUDA_INCLUDE_PATH` — **requires downloading and installing the full, registration-gated NVIDIA Video Codec SDK to build.** | 138 commits — most mature of the group. | Windows + Linux documented. | **Disqualified — build-time SDK download, not just a runtime driver DLL. License is fine; the build requirement is what fails the brief.** |
| `nvidia-video-codec-rs` ([rust-av](https://github.com/rust-av/nvidia-video-codec-rs)) | MIT (rust-av convention) | Headers looked up under `/opt/cuda/include` / `/opt/nvidia-video-codec/include` by default; same SDK-install requirement as above (likely the predecessor of the crate above). | — | Linux-oriented paths shown; Windows not the primary story. | **Disqualified, same reason.** |
| `nvcodec-rs` ([shiguredo](https://github.com/shiguredo/nvcodec-rs)) — found during research, not in the original list | Apache-2.0 | Dynamically `dlopen`s the CUDA driver API at runtime (good pattern) but still expects NVIDIA Video Codec SDK headers at compile time. | — | **Linux-only (x86_64)**, no Windows support documented. | **Out of scope — Windows-first, no D3D input path.** |

**License verdict for the chosen bindings source (`nvenc-sys`): MIT OR
Apache-2.0 — fully permissive, no GPL/LGPL/AGPL/SSPL/BUSL, passes `cargo deny`.**
The vendored `nvEncodeAPI.h`-derived structs carry NVIDIA's own separate,
also-permissive per-file notice, which must be preserved verbatim in whatever we
vendor (see Decision).

## Decision

### Bindings dependency

Depend on the **shape** of `nvenc-sys` 0.1 (legion-labs, MIT OR Apache-2.0) as
the low-level struct / function-pointer-type source, but do not lean on
unattended upstream releases:

1. Vendor the specific bindgen'd items we need into `mediaway-encoder-nvenc`'s
   own internal `sys` module (copy, not a crates.io dependency edge we don't
   control) — carrying NVIDIA's per-file permission notice forward verbatim, as
   that notice itself requires ("above copyright notice and this permission
   notice shall be included in all copies").
2. Extend that vendored copy with `NV_ENC_DEVICE_TYPE_D3D12`,
   `NV_ENC_INPUT_RESOURCE_D3D12`, `NV_ENC_OUTPUT_RESOURCE_D3D12`, and
   `NV_ENC_FENCE_POINT_D3D12` — present in NVIDIA's current `nvEncodeAPI.h`
   (SDK ≥ 11.1) but absent from both surveyed crates' safe/sys layers. Source
   these from a current `nvEncodeAPI.h` copy (same permissive per-file notice),
   never from SDK *sample code* (which carries the separate, non-redistributable
   EULA).
3. Track `AlsoSylv/nvenc` as a **watch candidate**: if it reaches a released
   version with D3D12 device-type/fence support and a couple more maintenance
   cycles, re-evaluate depending on it directly (or upstreaming our D3D12 work)
   instead of maintaining our own fork — its dependency shape (`libloading`,
   `windows` 0.62, matching MSRV/edition) is the better long-term fit if it
   matures.
4. Do **not** add `nvidia-video-codec-sdk` / `nvidia-video-codec-rs` — their
   build-time SDK download requirement is disqualifying regardless of license.

New workspace dependency: **`libloading`** (small, MIT/Apache-2.0, widely used)
for the Windows `LoadLibraryW`/`GetProcAddress` (and later Linux
`dlopen`/`dlsym`) runtime resolution — pin via `[workspace.dependencies]` when
implementation starts, per [`deps-policy.md`](../../../docs/conventions/deps-policy.md).

The runtime driver library itself (`nvEncodeAPI64.dll` on Windows,
`libnvidia-encode.so.1` on Linux later) is proprietary NVIDIA driver software,
loaded dynamically at runtime, **never linked at build time, never vendored or
redistributed by Mediaway** — the same legal shape already accepted for
Windows' own proprietary Media Foundation / COM surface in
`mediaway-encoder-windows`. It requires an NVIDIA GPU + driver present at
runtime; absence is probed and reported as `EncodeError::Unsupported`, never a
build or license gate.

### Crate placement

New crate **`mediaway-encoder-nvenc`** (vendor-scoped, **not** OS-suffixed),
sibling to `mediaway-encoder-windows` under the `mediaway-encoder` facade,
selected through `EncodeMode::Os::Gpu::VendorHw` (ADR-0004).

Reasoning:

- NVENC's C API, function table, struct layout, and the bulk of the session
  state machine (open → configure → register/map resource → encode → drain →
  destroy) are **platform-independent**. Only the GPU device-handle acquisition
  (`ID3D11Device*` / `ID3D12Device*` on Windows vs. a CUDA context or Vulkan
  device on Linux) and the driver-library path differ per OS. Naming it
  `mediaway-encoder-windows-nvenc` would force a near-total duplicate
  `mediaway-encoder-linux-nvenc` crate later, splitting one C API's Rust
  wrapper across two crates — the opposite of what the
  `mediaway-<capability>-<platform>` pattern exists for (WMF vs. VA-API vs.
  WebCodecs really are unrelated, non-portable OS APIs; NVENC is one portable
  vendor API reachable from multiple OSes).
- This mirrors how the workspace already carved out `mediaway-wgpu` as a
  **framework-scoped** crate orthogonal to `mediaway-<capability>-<platform>`
  OS backends ([`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md)).
  NVENC is the same shape of axis (vendor/framework, not OS), just producing
  encoded packets instead of bridging GPU buffer handles.
- `Os::Gpu::VendorHw`'s nesting under `Os` in the `auto` selection enum is a
  **selection-UX** grouping only (one knob an app turns); it does not mandate a
  1:1 crate-per-OS split. Crate-packaging boundaries and the `auto` type tree
  are separate concerns — `crate-packaging.md` does not name a "vendor" axis
  today, which is exactly why this ADR decides it.
- Internally the crate still `cfg`-gates by OS: Stage 1 wires the Windows
  loader + D3D11/D3D12 device types only (`#[cfg(windows)]`), with a
  `#[cfg(not(windows))]` `EncodeError::Unsupported` stub — identical to
  `mediaway-encoder-windows`'s own existing cross-compile stub pattern. Linux
  (CUDA/Vulkan device types, `libnvidia-encode.so.1`) is explicit future work,
  not a crate rename.

**Coordination note (not a final workspace decision):** this ADR's naming
(`mediaway-encoder-nvenc`) is proposed as the pattern Intel Quick Sync / AMD AMF
would mirror (`mediaway-encoder-quicksync`, `mediaway-encoder-amf`) for axis
consistency. Per this task's scope, README/wiki updates and final cross-ADR
consistency review are reserved for whoever compares all three vendor-SDK ADRs
together — this ADR does not touch README/wiki and should not be read as
pre-deciding that comparison.

### ZCA shape (sketch, pre-implementation)

Mirrors `mediaway-encoder-windows`'s existing style (`wmf::WmfVideoEncoder`): a
concrete struct with enum/`Option`-typed fields, not `PhantomData`-driven
compile-time typestate. Illegal calls after close are rejected at runtime via
`EncodeError::Closed`, matching `WindowsVideoEncoder`'s
`inner: Option<...>` + `.ok_or(EncodeError::Closed)` pattern exactly.

```rust
// Process-wide, lazily loaded once; Arc-shared across sessions in this process.
struct NvencLibrary {
    _module: libloading::Library,             // keeps resolved fn pointers valid
    functions: NV_ENCODE_API_FUNCTION_LIST,    // NVIDIA's raw #[repr(C)] fn-pointer table
}
static NVENC_LIBRARY: OnceLock<Result<Arc<NvencLibrary>, EncodeError>> = OnceLock::new();

// Closed, small variant set — enum + exhaustive match, no `dyn Device`.
enum NvencDeviceKind {
    DirectX11(NativeHandle),
    DirectX12(NativeHandle),
    // Cuda(..) / Vulkan(..) follow when Linux lands.
}

// Usually 1-3 rotating textures per session (double/triple buffering).
struct RegisteredResource {
    native: NativeHandle,
    handle: NV_ENC_REGISTERED_PTR,
}

pub struct NvencVideoEncoder {
    library: Arc<NvencLibrary>,   // clone: Arc share — process-wide fn table, no data copy
    session: NonNull<c_void>,     // opaque NVENC encoder handle (per SDK contract)
    device: NvencDeviceKind,
    registered: SmallVec<[RegisteredResource; 4]>,
    pending: VecDeque<Packet>,
    flushed: bool,
    info: StreamInfo,
}
```

- `NvencVideoEncoder::open(config: &VideoEncoderConfig) -> Result<Self, EncodeError>`
  dispatches on `config.input` / `config.gpu_device` exactly like
  `WmfVideoEncoder::open` dispatches on `VideoInputPreference` today —
  `open_cpu` (CUDA-context device type + `NvEncCreateInputBuffer` host-visible
  staging, named `upload_cpu_nv12` to match the existing WMF caveat-catalog
  naming for the same cost), `open_dx11`, `open_dx12`.
- Closed `enum` over `dyn Backend`: the device-kind set is small and known —
  matches the ZCA toolkit's "prefer enum + exhaustive match" /
  "enum of known backends over `dyn Backend` when the set is closed".
  `NvEncRegisterResource` is not free per call; real callers rotate a small,
  bounded set of physical textures, so caching registered handles by
  `NativeHandle` identity in `registered` avoids re-registering every frame —
  `SmallVec<[RegisteredResource; 4]>` fits the "usually small, bounded" case
  directly (approved per `zero-cost-abstractions.md`).
- `impl VideoEncoder for NvencVideoEncoder` implements the **existing** facade
  trait (`stream_info` / `push_frame` / `poll_packet` / `flush`) from
  `mediaway-encoder::video` — no new trait, no facade change.
- `#![allow(unsafe_code)]` at the crate boundary (matches `mediaway-encoder-windows`'s
  `wmf` modules); every raw NVENC call site carries `// SAFETY:` naming the
  invariant (handle non-null, resource lifetime across register/map/unmap,
  fence ordering for D3D12).

### D3D11 Zero-Copy (concrete mechanism)

1. `config.gpu_device = Some(GpuDeviceHandle::DirectX11(handle))` → open with
   `NV_ENC_DEVICE_TYPE_DIRECTX`, `device = handle.get() as *mut c_void` — the
   cast happens only at this FFI boundary, per `mediaway-common::gpu`'s
   contract that `NativeHandle` is never dereferenced outside platform crates.
2. Per pushed `VideoFrame` carrying
   `VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 { texture, subresource })`:
   `NvEncRegisterResource(NV_ENC_INPUT_RESOURCE_TYPE_DIRECTX3D, texture.get() as *mut c_void, subresource, ..)`
   once per distinct `texture` (cached via `registered`), then
   `NvEncMapInputResource` per frame, `NvEncEncodePicture`, then
   `NvEncUnmapInputResource` once the picture is consumed.
3. No `CopyResource`, no CPU map/readback anywhere in this path — the caller's
   own `ID3D11Texture2D` is handed to NVENC directly.
   `EncodePathClass::ZeroCopy` (`"zc"`) — the existing label from
   `mediaway-encoder::auto`, no new path-class variant needed.

### D3D12 Zero-Copy (concrete mechanism — the actual "NVENC direct beats WMF" case)

This is the path WMF's own ADR-0006 cannot offer without a copy: D3D11On12
fails MF's HW MFT, so `mediaway-encoder-windows` bridges D3D12 → native D3D11
via `D3d12SharedEncodeBridge`, an explicit, documented **GpuCopy** (one VRAM
copy per frame), not Zero-Copy. NVENC's own API has first-class D3D12 support
instead:

1. Open with `NV_ENC_DEVICE_TYPE_D3D12`, `device = ID3D12Device*`.
2. Per registered `ID3D12Resource*`, NVENC needs explicit fence-based sync
   (D3D12 has no implicit ordering like D3D11): the caller submits
   `NV_ENC_INPUT_RESOURCE_D3D12 { pInputBuffer, inputFencePoint: NV_ENC_FENCE_POINT_D3D12 { pFence, value } }`
   in `NV_ENC_PIC_PARAMS::inputBuffer`. NVENC's internal queue waits on that
   fence `value` before touching the texture — the caller signals the fence
   (`ID3D12CommandQueue::Signal`) after its own render/compute work; no
   host-side (CPU) wait is required on either side.
3. NVENC signals `NV_ENC_OUTPUT_RESOURCE_D3D12::outputFencePoint` on
   completion so downstream GPU work can chain without a CPU round-trip.
4. Net effect: an app already rendering/compositing on D3D12 (or wgpu's DX12
   backend) hands its texture straight to NVENC with **zero VRAM copies**, vs.
   the current WMF path's mandatory one-copy GpuCopy bridge.
   `EncodePathClass::ZeroCopy`, not `GpuCopy` — this is the concrete
   perf/architecture claim, and it must be benched against
   `zc_wmf_h264_dx11` / the `GpuCopy` bridge numbers before any README ⚡
   promotion, per `benchmarking.md`'s "same path class for headlines" rule.
5. Depends on the fence structs flagged as **missing from both surveyed
   crates' safe layers** above — real implementation work, not just wiring.

### Codec coverage caveat

NVENC covers **H.264 / HEVC / AV1 encode** (AV1 requires Ada / RTX 40-series+
silicon) but has **no VP9 encoder** (NVENC is VP9 decode-only). WMF covers all
four (H.264/HEVC/AV1/VP9) today via MFT enumeration. The `VendorHw` axis is
therefore honestly a **subset** of `GraphicsApi`'s codec coverage for NVIDIA,
not a superset — this belongs in the codec matrix once the crate lands
(README/wiki are out of this ADR's scope).

### Dependency checklist (per `deps-policy.md`, recorded here since this ADR is required by policy)

- **Need:** real requirement for the README "GPU — by vendor" NVIDIA row; not speculative.
- **License:** `nvenc-sys`-shape source is MIT OR Apache-2.0; our vendored fork
  keeps the same identity. No GPL/LGPL/AGPL/SSPL/BUSL/EULA-gated code enters
  the Cargo graph.
- **Runtime driver DLL:** proprietary, driver-supplied, dynamically loaded,
  never linked/vendored — see Decision above.
- **Maintenance:** both surveyed crates are thin (stale or very fresh, small
  teams) — mitigated by vendoring only what we use instead of trusting
  upstream release cadence.
- **New workspace dep:** `libloading` (small, permissive, widely used).
- `cargo deny check advisories licenses bans sources` must pass before merge.
  This ADR satisfies the "ADR required" trigger (platform FFI + codec +
  vendor-SDK-adjacent + non-trivial transitive surface).

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Feature-gated `nvenc` module inside `mediaway-encoder-windows` | Blocks Linux NVENC reuse later; couples an OS-scoped crate to a vendor-SDK axis. `crate-packaging.md`'s "thin adapter stays in the facade unless it grows enough to need its own crate" rule points the other way once D3D11 + D3D12 + CPU-upload + HEVC/AV1 is real code, not a thin adapter |
| `mediaway-encoder-windows-nvenc` (OS-prefixed) | Forces a near-duplicate `-linux-nvenc` crate later for one portable C API; wrong axis for the `<capability>-<platform>` naming pattern |
| Depend directly on crates.io `nvenc-sys` / `nvenc` without vendoring | Both are effectively single-maintainer, low-commit-count projects; a silent yank or breaking release with no path-class caveat update is exactly what the workspace's honesty rules exist to prevent |
| Hand-write NVENC bindings fully from scratch (no bindgen base) | `nvEncodeAPI.h` is a large, ABI-precise `#[repr(C)]` surface; bindgen-from-header (as `nvenc-sys` already did) is materially less error-prone than hand transcription, even though we still extend it for D3D12 |
| CUDA-context-only device type (skip D3D11/D3D12 entirely) | Throws away the entire reason to prefer NVENC over WMF here — Zero-Copy from the app's existing D3D surface is the point; CUDA-only has no advantage over WMF's existing DX11 ZC path and would need its own separate interop story |
| Auto-prefer `VendorHw` over `GraphicsApi` by default | Contradicts ADR-0004 ("VendorHw not default #1 … same silicon often") — stays an explicit opt-in backend, benched before any Auto-order change |

## Consequences

### Positive

- Unlocks the README "GPU — by vendor" NVIDIA row on a real, controllable code
  path, independent of whatever a given driver chooses to expose through Media
  Foundation's HW MFT enumeration.
- True Zero-Copy D3D12 input (fence-based), removing the one mandatory VRAM
  copy `D3d12SharedEncodeBridge` (ADR-0006) currently requires for D3D12-origin
  frames.
- Crate boundary (`mediaway-encoder-nvenc`, vendor-scoped) is ready for Linux
  NVENC later without a rename/split.

### Negative / Trade-offs

- No surveyed permissively-licensed Rust NVENC crate is both actively
  maintained *and* has D3D12 device-type/fence support today — real
  fork/extension work, not a drop-in dependency.
- Multi-thousand-line subsystem (see size estimate below), not a small add;
  D3D12 fence synchronization is genuinely fiddlier than WMF's DXGI
  surface-buffer submission.
- NVENC hardware/driver presence still needs graceful `EncodeError::Unsupported`
  probing (no GPU, no driver, old driver missing D3D12 fence support) — another
  honest-fallback surface to maintain alongside WMF's own.
- VP9 is not covered by this axis at all — a real codec-matrix gap vs. the
  existing WMF backend, not cosmetic.

## Size estimate (informational — not a commitment)

Staged like `mediaway-encoder-windows` itself (CPU first, then Zero-Copy, then
multi-codec):

| Stage | Scope | Rough size |
|---|---|---|
| 0 | Vendored/extended `sys` module (fn table, structs, enums incl. D3D12 fence types) + dynamic loader (`libloading`, Windows first) | 400–700 lines (bindgen-shaped; low hand-authored density) |
| 1 | H.264 CPU-upload session (CUDA device type, `NvEncCreateInputBuffer` lock/unlock, bitstream drain, extradata) | 400–600 lines |
| 2 | D3D11 Zero-Copy input (`NvEncRegisterResource`/map, resource cache) | 200–350 lines |
| 3 | D3D12 Zero-Copy input (fence structs, `ID3D12Fence` signal/wait wiring) | 250–400 lines |
| 4 | HEVC / AV1 profile + preset/tuning GUID tables | 150–250 lines |
| 5 | `auto` wiring into `EncodeMode::Os::Gpu::VendorHw` | 100–200 lines |
| — | Tests (unit + hardware-gated skip-safe integration, mirroring the existing WMF test file's depth) | 400–700 lines |

**Total: roughly 2,000–3,200 lines** across Stage 0–5 plus tests — a genuine
multi-thousand-line subsystem comparable to (and, because of D3D12 fence
handling, somewhat more intricate than) the existing WMF backend, not a "few
hundred lines" add. Fits the workspace's `≤1000 lines per source file` rule via
the same kind of module split `wmf/` already uses (e.g. `loader.rs`,
`session.rs`, `cpu_input.rs`, `d3d11_input.rs`, `d3d12_input.rs`,
`bitstream.rs`, `guids.rs`, `error.rs`, `auto.rs`).

## References

- `mediaway-encoder` ADR-0004 (backend preference hierarchy: `Os::Gpu::VendorHw`)
- `mediaway-encoder-windows` ADR-0001 (WMF H.264 surface), ADR-0003 (DX11
  Zero-Copy), ADR-0005 (BGRA input), ADR-0006 (D3D12 shared → D3D11 GpuCopy
  bridge — the copy this ADR's D3D12 path removes)
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md)
- [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md) ·
  [`docs/adr/0003-crate-packaging.md`](../../../docs/adr/0003-crate-packaging.md) ·
  [`docs/adr/0012-unprefixed-reusable-cores.md`](../../../docs/adr/0012-unprefixed-reusable-cores.md)
- [`docs/spec/zero-cost-abstractions.md`](../../../docs/spec/zero-cost-abstractions.md)
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md)
- [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md)
- Wiki: [`docs/ai/wiki/platform/windows-encode.md`](../../../docs/ai/wiki/platform/windows-encode.md)
  (RTX 4090 / no working HW MFT caveat),
  [`docs/ai/wiki/encode/backend-preference.md`](../../../docs/ai/wiki/encode/backend-preference.md)
- `nvenc-sys` (legion-labs, MIT OR Apache-2.0): <https://github.com/legion-labs/nvenc-sys>
- `nvenc` (AlsoSylv, MIT — watch candidate): <https://github.com/AlsoSylv/nvenc>
- NVIDIA, "Encoding for DirectX 12 with NVIDIA Video Codec SDK 11.1" (D3D12
  fence design reference — blog post, not vendored code):
  <https://developer.nvidia.com/blog/encoding-for-directx-12-with-video-codec-sdk-11-1/>

## Addendum — 2026-07-29: Stage 1 (H.264 CPU-upload) implemented + hardware-verified

**Status of this addendum: Implemented (Stage 1 only)** — the sections above remain the
original research-only record; this addendum documents what was actually built, verified,
and what changed versus the original Decision.

### Bindings decision: revised — depend on `nvenc` directly, do not vendor `nvenc-sys`

The original Decision favored vendoring `nvenc-sys`'s bindgen'd shape into an internal `sys`
module and treating `AlsoSylv/nvenc` as a watch candidate only. Building Stage 1 for real
changed that call:

- `nvenc` 0.1.0 (crates.io) compiles as-is against this workspace's exact `windows = "0.62"`
  / edition 2024 / `rust-version 1.85.1` pins (confirmed via `cargo check`/`clippy`/`test`),
  and its safe `session`/`encoder`/`input_buffer`/`bitstream` modules cover everything Stage
  1 needs: `Session::open_dx`, `get_encode_codecs`/`get_encode_presets`/
  `get_encode_preset_config_ex`, `init_encoder`, `register_resource_dx11`,
  `create_bitstream_buffer`, `encode_picture`, `try_lock`. No hand-vendoring of NVIDIA's
  `nvEncodeAPI.h` was needed for this scope.
- `nvenc-sys` is not published on crates.io at all (`cargo info nvenc-sys` fails); using it
  would have meant vendoring its bindgen'd Windows/Linux structs from GitHub source into this
  crate before writing a single line of session logic — real extra work with no Stage 1
  benefit, since the D3D12 fence structs that motivated "extend the vendored copy" in the
  original Decision are Stage 3 (Zero-Copy D3D12) scope, not touched here.
- **This crate depends on `nvenc` directly as a normal workspace dependency** (see root
  `Cargo.toml`), not vendored. `libloading` was **not** added as a direct dependency of this
  crate — `nvenc` resolves and loads `nvEncodeAPI64.dll` internally via its own `libloading`
  use; this crate never touches the DLL/loader directly.
- `AlsoSylv/nvenc`'s dependency graph (`libloading` 0.8, `bitfields` 1.x, both MIT) passes
  `cargo deny check advisories licenses bans sources` cleanly alongside the rest of the
  workspace graph.
- The original Decision's D3D12 fence-struct gap (`NV_ENC_INPUT_RESOURCE_D3D12` /
  `NV_ENC_FENCE_POINT_D3D12` missing from `nvenc`'s safe layer) is still real — those structs
  *do* exist in `nvenc::sys::structs` (hand-written, present) but are not wired into the safe
  `encoder`/`session` API yet. Stage 3 (D3D12 Zero-Copy) will need to either extend `nvenc`
  upstream/locally or drop to its `sys` layer directly; re-evaluate vendoring at that point,
  not before.

### Real hardware finding: `nvenc` 0.1.0's native CPU input-buffer path is broken

While implementing the "true" CPU-upload path NVENC's own API offers — `Encoder::
create_input_buffer` (`NV_ENC_MEMORY_HEAP_SYSTEM_CACHED`) + `InputBuffer::lock()` (wraps
`NvEncLockInputBuffer`) + memcpy + drop (wraps `NvEncUnlockInputBuffer`) — this session
reproduced a real, deterministic failure on the RTX 4090 (driver 32.0.15.9579):

- `create_input_buffer` and the initial `lock()` call both succeed (valid pointer + pitch
  returned).
- Writing **more than 8 bytes** into the locked pointer before unlocking — confirmed via a
  bisection harness (`WRITE_N` env var over a standalone reproduction binary) across
  `std::ptr::write_bytes`, an element-wise write loop, and `write_volatile` — makes the
  subsequent unlock (`Drop` of `InputBufferLock`, i.e. `NvEncUnlockInputBuffer`) fail with
  `NVencError::InvalidParam`. Writing ≤8 bytes succeeds; writing 16 bytes or the full NV12
  payload (460,800 bytes for 640×480) both fail identically. This is **not** a buffer-size
  overflow in the classical sense — even a single 640-byte row (`width` bytes, well inside
  any legitimately-sized NV12 buffer) already triggers it.
- Root cause not further isolated (e.g., not confirmed whether this is `nvenc` 0.1.0's own
  bug or a `NVencLockInputBuffer`/`NVencUnlockInputBuffer` struct-layout mismatch against the
  real NVENC 13.0 ABI) — reported here as an exact, reproduced symptom, not a diagnosis.
  `nvenc`'s own example (`examples/simple_encode.rs`) never exercises this path either — it
  only uses `register_resource_dx11`, which is consistent with this path being genuinely
  unexercised upstream.
- **Workaround (what Stage 1 ships):** CPU-upload is instead implemented via a private,
  internally-owned D3D11 staging texture (`D3D11_USAGE_STAGING`, `Map`/memcpy/`Unmap`) +
  `CopyResource` into a private GPU-resident texture (`D3D11_USAGE_DEFAULT`), registered
  **once** with NVENC via `register_resource_dx11` and reused across frames (the driver reads
  live texture contents at `encode_picture` time, not a snapshot at register time — confirmed
  by re-uploading + re-encoding 5 times against the same registered resource). This is still
  a CPU-upload path from the facade's point of view (caller only supplies
  `VideoFrameStorage::Cpu` bytes, no `gpu_device`); it is simply implemented via a D3D11
  texture write instead of NVENC's native host-buffer lock. See
  [`dx11::device::Dx11Upload::upload_cpu_nv12`](../src/dx11/device.rs) for the documented
  cost (two copies: CPU→staging `Map`, staging→GPU `CopyResource`).

### Other real findings

- **Minimum resolution:** `init_encoder` fails with `NVencError::InvalidParam` at 64×64 but
  succeeds at 640×480 on this GPU/driver. The exact NVENC-reported minimum
  (`NV_ENC_CAPS_WIDTH_MIN`/`HEIGHT_MIN`) was not queried (`nvenc`'s safe layer does not expose
  `NvEncGetEncodeCaps`) — `validate()` in this crate does not hardcode a guessed minimum;
  tests use 640×480 (confirmed working) rather than risk a flaky guess.
- **Preset defaults are not encode-ready as-is:** `get_encode_preset_config_ex`'s returned
  `NVencConfig` needs `gop_len` and `frame_interval_p` set explicitly (`30` and `1` in this
  implementation) before `init_encoder`/`encode_picture` — leaving the raw preset values
  reproducibly made `encode_picture` fail with `InvalidParam` on the very first pushed frame
  (session open itself still succeeded). `nvenc`'s own example makes the same overrides
  (`gop_len = 0xffffffff`, `frame_interval_p = 1`), which is what pointed at the fix.

### What shipped (Stage 1 scope)

- `crates/mediaway-encoder-nvenc/{Cargo.toml, src/lib.rs, src/dx11/{mod,device,video,
  video_tests}.rs, src/lib_tests.rs}` — `NvencVideoEncoder` implementing
  `mediaway_encoder::VideoEncoder` (`stream_info`/`push_frame`/`poll_packet`/`flush`),
  Windows-only (`#[cfg(windows)]`) with an honest `EncodeError::Unsupported` stub on other
  targets, mirroring `mediaway-encoder-linux`'s cross-compile stub pattern.
- H.264 only, `VideoInputPreference::CpuUploadOk` only, fixed P3/`HighQuality` preset,
  `enable_ptd = true` (automatic GOP/picture-type decisions, no explicit IDR/GOP control),
  keyframe detection via a local Annex-B NAL scan (`contains_idr_nal`) rather than a separate
  extradata channel — SPS/PPS ride inline before each IDR, same convention as
  `mediaway-encoder-linux`'s VA-API backend.
- **Not** wired into `mediaway-encoder`'s facade/`auto` selection — deliberately out of scope
  for this pass (a later integration task), per the coordination note in the original
  Decision section above.
- **Hardware-verified 2026-07-29** (this session, real machine, real RTX 4090, driver
  32.0.15.9579): `cargo test -p mediaway-encoder-nvenc -- --nocapture` — both the low-level
  `dx11::video_tests::nvenc_open_and_encode_or_skip_without_hw` (5 synthetic NV12 frames,
  asserts real Annex-B start codes, an inline SPS NAL, and a leading IDR keyframe) and the
  top-level `lib_tests::open_h264_cpu_upload_or_skip_without_hw` (through the public
  `NvencVideoEncoder` wrapper) **actually encoded** rather than skipped — output confirmed as
  genuine H.264: `00 00 00 01 67 64 00 1e ac 2b 20 14 …` (start code + NAL type 7 SPS,
  `profile_idc = 0x64` High profile) followed by a PPS and IDR slice, then P-frames on
  subsequent pushes. Also clean: `cargo check`/`clippy --all-targets -- -D warnings`/
  `fmt --check`/`cargo deny check advisories licenses bans sources`.
- Both hardware-gated tests were also confirmed stable running concurrently (default
  parallel `cargo test` threads) after the preset-defaults fix above — no cross-session
  contention observed on this single-GPU machine for two simultaneous NVENC sessions.

### Deferred (unchanged from the original Decision, restated for the roadmap)

D3D11/D3D12 Zero-Copy input, HEVC/AV1/multi-codec, Linux (`libnvidia-encode.so.1`/CUDA
device type), and `auto` wiring — see [`docs/roadmap.md`](../docs/roadmap.md).

## Addendum — 2026-07-29: HEVC + AV1 CPU-upload encode added + hardware-verified

**Status of this addendum: Implemented** — extends the H.264 CPU-upload addendum above to
HEVC and AV1, same session shape, same D3D11 staging-texture upload workaround. Both codecs
are real, hardware-verified findings on this crate's reference RTX 4090, not compile-only or
simulated results.

### Both codecs worked with no bindings gap and no driver/hardware gap

Before implementing, the concern flagged by this task was whether the `nvenc` crate's safe
`session`/`encoder` layer even exposes HEVC/AV1, since `safe::encoder::CodecPicParams` (the
enum carried in `NVencPicParams::codec_pic_params` for codec-specific per-picture params) only
has an `H264(NVencPicParamsH264)` variant — no `Hevc`/`Av1` variant exists in that enum today.
That gap turned out **not to block encoding**: this crate's H.264 path already passes `None`
for `codec_params` in every `encode_picture` call (no per-picture codec-specific params are
used), and the rest of the safe API — `Session::open_dx`, `get_encode_codecs`,
`get_encode_preset_config_ex`, `init_encoder`, `register_resource_dx11`,
`create_bitstream_buffer`, `encode_picture`, `try_lock` — is entirely codec-agnostic: codec
selection is just the `Guid` passed to `get_encode_preset_config_ex`/`InitParams::encode_guid`.
`nvenc::sys::guids` already carries real, correct-looking `NV_ENC_CODEC_HEVC_GUID` and
`NV_ENC_CODEC_AV1_GUID` constants (see `nvenc-0.1.0/src/sys/guids.rs`), and `nvenc::sys::structs`
already carries the codec-specific config/pic-param structs (`NVencConfigHEVC`,
`NVencConfigAV1`, `NVencPicParamsHEVC`, `NVencPicParamsAV1`) inside the relevant unions, even
though the safe `encoder.rs` layer doesn't wire per-picture HEVC/AV1 params through
`CodecPicParams` yet. Net effect: **swapping the codec GUID through the existing generic
session-open path was sufficient** — no `nvenc` fork/extension was needed for either codec,
unlike the D3D12 fence gap flagged in the original Decision (still real, still Stage 3 scope).

Concretely, this crate's `codec_guid()` (`src/dx11/video.rs`) maps `CodecKind::H264` /
`::Hevc` / `::Av1` to their NVENC GUIDs and `None` for anything else (VP9 — no NVENC encoder
at all — and non-video codecs); `NvencSession::open()` still probes
`Session::get_encode_codecs()` before proceeding, so an absent codec on a given GPU/driver
still surfaces as `EncodeError::Unsupported` rather than a hard failure deeper in the call
chain.

**Hardware-verified 2026-07-29** (`cargo test -p mediaway-encoder-nvenc -- --nocapture`, real
RTX 4090, driver 32.0.15.9579): both `dx11::video_tests::nvenc_open_and_encode_hevc_or_skip_without_hw`
and `dx11::video_tests::nvenc_open_and_encode_av1_or_skip_without_hw` **actually encoded**
(session open, `get_encode_codecs` contained both GUIDs, `init_encoder` succeeded, real bytes
came back from `try_lock`), running both alongside the existing H.264 tests under default
parallel `cargo test` threads (4 concurrent NVENC sessions on this single-GPU machine) with no
contention observed.

### HEVC: genuine Annex-B NAL output, VPS confirmed

Same bitstream shape as H.264 (Annex-B, 4-byte start codes), but a 2-byte NAL header
(`forbidden_zero_bit(1) + nal_unit_type(6) + nuh_layer_id(6) + nuh_temporal_id_plus1(3)`)
instead of H.264's 1-byte header — `contains_hevc_idr_nal` (`src/dx11/video.rs`) reads the type
from the top 6 bits of the first header byte (`(byte >> 1) & 0x3F`) and checks for 19
(`IDR_W_RADL`) or 20 (`IDR_N_LP`). Real first-packet bytes from the test run:

```
00 00 00 01 40 01 0c 01 ff ff 01 60 00 00 03 00 90 00 00 03 00 00 03 00 5a 97 02 40 00 00 00 01
```

`00 00 00 01` start code, then header byte `0x40`: `(0x40 >> 1) & 0x3F == 32` — a genuine VPS
NAL (type 32), the codec NVENC would only emit for a real HEVC bitstream. The scan-verified
(not just the 32-byte prefix shown above) assertions in the test — first packet is a keyframe,
carries an inline VPS (type 32) *and* SPS (type 33) — all passed against the full payload.

### AV1: genuine OBU-framed output, sequence header OBU confirmed

AV1 has no NAL/Annex-B start codes at all — its bitstream is a sequence of OBUs (Open
Bitstream Units), each with its own header byte (`forbidden_bit(1) + obu_type(4) +
obu_extension_flag(1) + obu_has_size_field(1) + reserved(1)`) and, when the size-field bit is
set, a `leb128`-encoded payload length. This crate added
`contains_av1_sequence_header_obu`/`read_leb128` (`src/dx11/video.rs`) to walk that framing and
detect `OBU_SEQUENCE_HEADER` (`obu_type == 1`) as the keyframe signal (NVENC — like other AV1
encoders — emits the sequence header OBU only on/before a keyframe; there is no NAL-type
equivalent to scan for). Real first-packet bytes from the test run:

```
12 00 0a 0b 00 00 00 24 c4 ff df 00 86 60 10 32 2f 10 01 05 c1 01 04 10 40 20 00 80 18 20 10 9e
```

Decoded by hand against the AV1 OBU header layout: `0x12` = `OBU_TEMPORAL_DELIMITER` (type 2,
has_size_field set), `0x00` = size 0 (empty payload) → next OBU at offset 2: `0x0a` =
`OBU_SEQUENCE_HEADER` (type 1, has_size_field set), `0x0b` = size 11 (single-byte `leb128`) →
an 11-byte sequence header payload (`00 00 00 24 c4 ff df 00 86 60 10`) → next OBU at offset
15: `0x32` = `OBU_FRAME` (type 6, has_size_field set), matching the expected
temporal-delimiter → sequence-header → frame OBU shape of a real AV1 keyframe access unit.
This is genuine AV1 hardware encode output, not a stub.

### What this means for the codec-coverage caveat in the original Decision

The original Decision's "Codec coverage caveat" section (above) already correctly stated NVENC
covers H.264/HEVC/AV1 encode (AV1 requiring Ada+) and no VP9 encode. This addendum confirms
that statement is now backed by real, hardware-verified code in this crate, not just a claim
from NVIDIA's public documentation — on this specific RTX 4090 + driver 32.0.15.9579 +
`nvenc` 0.1.0, both HEVC and AV1 encode end to end through the existing D3D11 CPU-upload path
with zero additional bindings work beyond the codec-GUID generalization described above.

### What shipped (this addendum's scope)

- `codec_guid()`, codec-generic `NvencSession::open()`/`push_frame()` (was H.264-only),
  `is_keyframe_packet()` dispatch, `contains_h264_idr_nal` (renamed from `contains_idr_nal`),
  `contains_hevc_idr_nal`, `contains_av1_sequence_header_obu` + `read_leb128` — all in
  `src/dx11/video.rs`.
- `validate()` now accepts `CodecKind::H264 | Hevc | Av1`; VP9 (and non-video codecs) remain
  rejected — NVENC genuinely has no VP9 encoder, so this is not a scope gap to close later.
- Hardware-gated tests `nvenc_open_and_encode_hevc_or_skip_without_hw` and
  `nvenc_open_and_encode_av1_or_skip_without_hw` (`src/dx11/video_tests.rs`), skip gracefully
  (do not fail the suite) on machines without a matching NVENC-capable GPU/driver, mirroring
  the existing H.264 test's pattern. Plus pure-logic unit tests for the new NAL/OBU scanners
  and `read_leb128` (no hardware needed).
- **Not** wired into `mediaway-encoder`'s facade/`auto` selection — same deliberate scope
  decision as the H.264 addendum; still a later integration task.
- No new Cargo dependency — `nvenc`'s existing GUID/struct surface was sufficient (`cargo
  check`/`clippy --all-targets -- -D warnings`/`fmt --check` all clean; `cargo deny check` not
  re-run since the dependency graph did not change).

### Deferred (updated)

D3D11/D3D12 Zero-Copy input (still Stage 3, still needs the fence-struct work flagged in the
original Decision — unaffected by this addendum), Linux (`libnvidia-encode.so.1`/CUDA device
type), and `auto` wiring — see [`docs/roadmap.md`](../docs/roadmap.md). Multi-codec is no
longer deferred: H.264/HEVC/AV1 are all real and hardware-verified as of this addendum.

ADRs are **English**. Numbering is local to this `adr/` folder.
