# ADR-0021: Workspace consolidation — merge the mediaway crate family

- **Status**: Accepted (2026-08-03)
- **Deciders**: @dev-nyxie (+ agent)

## Context

The workspace over-split the mediaway product family into ~40 crates: every
platform backend, GPU abstraction, and FFI surface got its own crate. The
split follows ADR-0003 (facade / platform backend / unprefixed core) and
ADR-0004 (per-capability `*-ffi`). Practical costs:

- **crates.io publishing is forced into the 39-crate dependency closure** —
  a published crate needs every dependency (target-gated included) on the
  registry, so shipping `mediaway-pipeline` drags in all platform backends,
  `mediaway-wgpu`, `vpl-sys`, etc. Pre-1.0 this is disproportionate to the
  API surface.
- **Backend selection is spread across crates instead of being a compile-time
  platform decision** — the same crate exists 4× (windows / linux / vulkan /
  web variants) with duplicated surface plumbing.
- **Three separate `-ffi` crates** ship three DLLs and three header sets that
  must stay in sync (ADR-0015 was a partial unification; the device/pipeline
  `PixelFormat` mirror drift remains).

## Decision

Consolidate the `mediaway-*` family into one crate per capability, with
platform backends as **`#[cfg]`-gated modules** inside their capability crate
(compile-time platform dispatch — **not** Cargo features). Keep the
freestanding unprefixed cores separate (ADR-0012 unchanged).

### Target layout

| Crate | Absorbs | Gating |
|---|---|---|
| `mediaway-common` | `mediaway-wgpu` (GPU abstraction; `GpuBufferHandle` already lives here) | `cfg(target_os …)` for wgpu adapters |
| `mediaway-container` | — (facade over the cores) | — |
| `mediaway-encoder` | `mediaway-encoder-windows` · `-nvenc` · `-quicksync` · `-vulkan` · `-web` · `-linux`; `vpl-sys` becomes an internal build-dependency of the quicksync module | `cfg(target_os)` / `cfg(target_family = "wasm")` |
| `mediaway-decoder` | `mediaway-decoder-windows` · `-vulkan` · `-web` · `-linux` | same |
| `mediaway-device` | `mediaway-device-camera` · `-desktop` · `-audio` + `mediaway-device-windows`×4 · `-linux` · `-web` | same |
| `mediaway` | `mediaway-pipeline` (EncodeSession + auto-dispatch) **+ re-exports** of container / encoder / decoder / device / sw — consumers depend on one crate | — |
| `mediaway-sw` | `mediaway-sw-opus` · `mediaway-audio-apm` | — |
| `mediaway-ffi` | `mediaway-common-ffi` · `mediaway-container-ffi` · `mediaway-pipeline-ffi` · `mediaway-device-ffi` → **one C ABI** (one cdylib, one header set) | — |

Unchanged: freestanding cores `iso-bmff`, `ebml-webm`, `flv`, `adts`, `ogg`,
`riff-wave`, `mpeg-ts`, `mpeg-audio`, `iso-cenc` (separate crates, reusable,
zero `mediaway-*` deps); CLIs `mediaway-avcli` / `mediaway-avprobe`.

### Rules amended

- **ADR-0003**: the platform-backend-as-crate split is abolished for the
  `mediaway-*` family — backends are `#[cfg]`-gated modules in their
  capability crate. The unprefixed-core rule stays. (Crate-local ADRs for the
  affected crates are superseded where they mandated separate backends.)
- **ADR-0004**: per-capability `*-ffi` is superseded by the single
  `mediaway-ffi` crate. ADR-0015 (common-ffi unification) is subsumed.
- **New rule**: platform selection is compile-time `#[cfg(target_os /
  target_family)]`, not Cargo features. Cargo features remain only for
  non-platform optionality (e.g. optional software codecs).

## Consequences

### Positive

- **crates.io publish set drops from 39 to ~17** (9 cores + 8 mediaway
  crates) — the dependency closure becomes tractable; the deferred crates.io
  release (see wiki `meta/release.md`) becomes viable again.
- One FFI surface: one DLL, one header set, no cross-header struct drift.
- Backend plumbing (trait wiring, error mapping) written once per capability
  instead of once per platform crate.

### Negative / Trade-offs

- Bigger per-crate binaries: every backend of a capability compiles on its
  platform unconditionally (no feature to exclude one).
- Wasm crates (`mediaway-encoder-web`, `mediaway-device-web`, `iso-bmff-wasm`)
  become `cfg(target_family = "wasm")` modules; wasm-only deps must be
  platform-gated in Cargo.toml.
- Bindings (C#/Node/Python/C++) and `tools/scripts/copy-native-dlls.ts`
  switch from 3 cdylibs to 1 (`mediaway_ffi.dll`); the release workflow's
  `native-assets` job builds one crate.
- Migration is a large diff (source moves + cfg gates + manifest rewrites);
  do it on a branch, keep `cargo nextest run --workspace` + clippy green per
  step.

## Migration steps

1. ADR-0003/0004 amendments landed with this ADR (crate docs updated in place).
2. `mediaway-ffi`: merge the four `-ffi` crates; update `copy-native-dlls.ts`,
   bindings loaders, and the release workflow's `native-assets` job.
3. Merge backends per capability (`device` → `encoder`/`decoder` → `common`
   wgpu), cfg-gating modules; delete the old crates.
4. `mediaway-sw` (sw + opus + apm) and `mediaway` (pipeline + re-exports).
5. Re-point crate-local ADRs and the wiki (`crate-map.md`, `release.md`);
   re-evaluate the crates.io deferral.

## References

- ADR-0003 crate packaging · ADR-0004 C-FFI · ADR-0012 unprefixed cores ·
  ADR-0015 common-ffi unification · ADR-0014 pipeline convenience crate
- wiki: `meta/release.md` (deferred crates.io), `meta/crate-map.md`

ADRs are written in **English**.
