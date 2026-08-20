# ADR-0004: Encode backend preference hierarchy

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (types; session wiring later)

## Context

Root README codec tables (OS·CPU / OS·GPU / GPU·API / GPU·vendor / CPU·SW) are the selection SSOT. Auto needs typed preferences on those axes. OS codec APIs are not a thin shared HAL; VendorHw (NVENC/…) is not automatically faster than OS + GraphicsApi Zero-Copy (often same silicon).

## Decision

```text
EncodeMode: Auto | Os::{Cpu, Gpu::{GraphicsApi, VendorHw}} | Sw
DeviceSelect: Auto | GpuAdapter | CpuTarget | Compatible(native)   # orthogonal
```

- **GraphicsApi** vs **VendorHw** = sibling backends (not filters on one path).
- **Os** ≈ platform crate; path class (`zc`/`copy`/`upload`/`readback`/`sw`) stays orthogonal.
- **Auto order:** GraphicsApi ZC → labeled GPU costs → OsCpu → (policy) Sw; VendorHw not default #1.
- **Os·Cpu caveat:** upload may still use HW encode on a GPU adapter — may need both `CpuTarget` and `GpuAdapter`.
- **Foreign intake:** same device → Compatible/`zc`; framework adapt (`mediaway-wgpu`); cross-API → named bridge + honest label; never silent “ZC”.
- Session HAL (push/pull/`path_class`) OK; no union mega-trait over WMF/WebCodecs/VA. Packaging unchanged (facade / `*-platform` / `mediaway-sw`).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Api+Vendor as filters on one Gpu path | Conflates graphics interop with direct HW SDKs |
| Auto prefers VendorHw first | Same silicon often; control ≠ automatic throughput |
| Thin wgpu-hal over all OS codecs | APIs not isomorphic |
| Silent cross-API convert as ZC | Violates caveats |

## Consequences

- Shared vocabulary with README/Auto. `EncodeMode`/`OsMode`/`GpuMode` were real types in
  `mediaway_encoder::auto` from 2026-07-29 through 2026-07-30, then replaced — see the
  2026-07-31 addendum below. `DeviceSelect` (GPU adapter enumeration) is still not
  implemented — no code needed it yet.

## Selection shape flattened + capability probe added (2026-07-31)

`EncodeMode`/`OsMode`/`GpuMode` (three nested enums) are replaced by two flat types in
`mediaway_encoder::auto`, no deprecation shim kept:

```text
Backend: Os | Nvenc | QuickSync | Amf | Software
BackendSelection: Auto | AutoHardwareOnly | Explicit(Backend)
```

- **`Backend::Os`** replaces `GraphicsApi` as one neutral, OS-agnostic tag (Media
  Foundation / VA-API / `VideoToolbox` per platform crate) — the taxonomy stayed
  otherwise unchanged: vendor SDKs are still sibling backends, not filters on one path.
- **`BackendSelection::Auto`** replaces the old `EncodeMode::Auto`; behavior unchanged
  (`Os`'s own ZC → `GpuCopy` → CPU-upload chain, then Software if the policy allows —
  never a vendor SDK).
- **`BackendSelection::AutoHardwareOnly`** is new — the old design had no way to ask
  "any hardware backend, vendor SDKs included, but never Software" without naming one
  vendor explicitly. It ranks NVENC/QuickSync *ahead of* `Os`'s own CPU upload (not
  just as a fallback after it) — without that ranking, `AutoHardwareOnly` degenerates to
  plain `Auto` on any machine where `Os`'s CPU-upload path already works, defeating the
  variant's purpose.
- **`BackendSelection::Explicit(Backend)`** replaces `EncodeMode::Os(OsMode::Gpu(GpuMode::VendorHw))`
  (which blended NVENC/QuickSync into one indistinguishable request) — each vendor SDK
  is now named and pinned individually, matching a settings-UI row per backend rather
  than one blended "vendor hardware" choice.
- **`FallbackPolicy`'s 4 independent bits are replaced by one ceiling**,
  `AutoVideoEncodeConfig::max_path_class: EncodePathClass` (now `Ord`) — tolerance
  nests monotonically (accepting Readback implies accepting the cheaper CpuUpload), so
  no real policy needs bitflag independence; a ceiling makes the unrepresentable
  combinations actually unrepresentable. Default unchanged in effect: `CpuUpload`
  (ZC/GpuCopy/CpuUpload allowed, Readback/Software require raising it — same behavior
  as the old `balanced()`).
- **Dropped, no replacement:** `EncodeMode::Os(OsMode::Cpu)` (force CPU-only even when
  `gpu_device` is set). The common case — no GPU device at all — is unchanged (just
  leave `gpu_device: None`); only the narrow "keep `gpu_device` populated but skip it
  for this one session" benchmarking case has no direct equivalent now. Revisit if a
  real caller needs it.
- **New:** `AutoVideoEncoder::resolved_backend() -> Backend` alongside the existing
  `path_class()` — `path_class` answers "how expensive", `resolved_backend` answers
  "which concrete backend", and `Auto`/`AutoHardwareOnly` can silently resolve to
  either axis independently.
- **New:** a capability probe (`mediaway_encoder::capability` +
  `mediaway_encoder_windows::auto::support`), mirroring `mediaway-device`'s
  [ADR-0003](../../mediaway-device/adr/0003-capability-and-permission-probe.md) — lets
  an app ask "what's usable on this machine right now" for a settings list instead of
  only discovering it via a failed `open`. Off Windows, every row is `NotImplemented`
  **at compile time** (`#[cfg(not(windows))]`) — no live session is opened on a target
  where this crate's backends are compile-time stubs.
- Decode did **not** get the same `Backend`/`BackendSelection` treatment — it has only
  one real backend per OS today (no sibling vendor decode SDKs wired into
  `mediaway::platform` yet), so adding `Backend` variants there would be an
  empty abstraction with nothing to select among.

## `Backend::Vulkan` resolves the taxonomy gap (2026-08-20)

`mediaway-encoder::vulkan`'s `VulkanVideoEncoder` (H.264/HEVC, CPU-upload, hardware-verified)
had no home in `Backend` — this crate's own `docs/ai/wiki/encode/backend-preference.md` flagged
it as "doesn't fit `GraphicsApi` or `VendorHw` cleanly: cross-vendor (unlike `VendorHw`) but
packaged like a vendor crate," left unresolved rather than guessed.

**Resolution**: add `Backend::Vulkan` and rank it in the **same tier as the vendor SDKs** —
`AutoHardwareOnly` tries it (after `Nvenc`/`QuickSync`, same CPU-upload cost class), and
`Explicit(Backend::Vulkan)` opens it directly. Plain `Auto` never resolves to it, same reasoning
already applied to `Nvenc`/`QuickSync`: the same silicon usually backs `Os`'s own path too, so
picking a non-`Os` backend by default would be a surprise.

Rationale for "packaging beats semantics" here: `BackendSelection`'s whole job is *ranking*, not
taxonomizing graphics APIs — from a caller's perspective, "one more hardware-capable backend to
try before giving up on GPU-only" is exactly what `AutoHardwareOnly` already models for
`Nvenc`/`QuickSync`, and Vulkan Video fits that role even though it isn't vendor-specific. A
`GraphicsApi`-shaped variant (alongside `Os`) was considered and rejected — see § Alternatives.

Windows-only wiring for now (`windows::auto::AutoVideoEncoder`'s `hardware_only` ranking and its
`Explicit(Backend::Vulkan)` match arm) — same scope Nvenc/QuickSync already have; Linux Vulkan
wiring is future work, not blocked by this decision.

### Alternatives Considered (2026-08-20 addendum)

| Alternative | Why not |
|---|---|
| New `GraphicsApi`-shaped enum axis (`Os` vs `GraphicsApi` vs `VendorHw`) | A real taxonomy fix, but a much larger, cross-cutting change to `Backend`'s whole shape for one crate's one backend — the wiki itself deferred this as "not resolved by this doc either," and no second cross-vendor graphics-API backend exists yet to justify a 3-way split over a 1-more-tier addition. |
| Leave Vulkan unreachable through `Backend`/`BackendSelection`, keep it callable only by naming `mediaway_encoder::vulkan::VulkanVideoEncoder` directly | Already true today and remains true regardless — this ADR is about giving the *auto-selection* facade a path to it, not about whether direct low-level access exists (it always has, per `docs/spec/api-layers.md`). |

### Implementation + hardware verification (2026-08-20)

`windows::auto::AutoVideoEncoder` gained `EncoderImpl::Vulkan`, `try_vulkan` (CPU-upload only,
mirrors `try_nvenc`/`try_quicksync`), an `Explicit(Backend::Vulkan)` match arm, and a spot in
`AutoHardwareOnly`'s ranking (`Nvenc` → `QuickSync` → `Vulkan`). Hardware-verified on the
reference RTX 4090: `explicit_vulkan_opens_or_skip` and `auto_hardware_only_tries_nvenc_then_
quicksync_then_vulkan_or_skip` (new) both resolve successfully — real finding along the way,
not a wiring bug: `VulkanVideoEncoder::open` rejects 64x64 (`EncodeError::InvalidInput`, this
driver's reported `minCodedExtent` for H.264 encode), so the new tests use 176x144, matching
the size `mediaway-encoder::vulkan`'s own `encoder_tests.rs` already uses for the same reason.
`cargo check`/`clippy --all-targets --all-features -- -D warnings`/`fmt --check` clean across
the whole workspace.

## References

- Root README § Codec support · [ADR-0003](0003-auto-encode.md) · wiki [backend-preference](../../../docs/ai/wiki/encode/backend-preference.md)
