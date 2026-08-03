# Crate packaging

Canonical decisions:

- Structure: [`docs/adr/0003-crate-packaging.md`](../adr/0003-crate-packaging.md)
- Naming **v1**: [`docs/adr/0012-unprefixed-reusable-cores.md`](../adr/0012-unprefixed-reusable-cores.md)
- **Consolidation (ADR-0021): platform backends are `#[cfg]`-gated modules inside
  their capability crate; one C ABI (`mediaway-ffi`); umbrella `mediaway`.**
  Amends ADR-0003 (backend-as-crate split) and ADR-0004 (per-capability `*-ffi`).

## Library crate kinds

```text
[unprefixed core]  iso-cenc, iso-bmff, …   ← no Mediaway types; own version (ADR-0021)

[capability crate] mediaway-container, mediaway-encoder, …
        traits + Mediaway-typed adapters over unprefixed cores
        + platform backends as #[cfg]-gated modules (src/<backend>/)
        e.g. mediaway-encoder::windows, mediaway-device::linux, mediaway-encoder::web

[shared types]     mediaway-common

[umbrella]         mediaway   ← EncodeSession + re-exports of the capability crates
                     (+ mediaway::wgpu GPU bridge)

[C ABI]            mediaway-ffi   ← single cdylib + include/mediaway/*.h
```

## Naming (v1, amended)

| Kind | Pattern | Example |
|------|---------|---------|
| Reusable domain core | Unprefixed short name | `iso-cenc`, `iso-bmff` |
| Shared product types | `mediaway-common` | — |
| Capability crate (facade + backends) | `mediaway-<capability>` | `mediaway-container`, `mediaway-device` |
| Backend module | `mediaway-<capability>::<platform>` | `mediaway-encoder::windows`, `mediaway-device::linux`, `mediaway-encoder::web` |
| GPU framework module | `mediaway::wgpu` | [`gpu-interop.md`](gpu-interop.md) |
| C ABI | `mediaway-ffi` | [`c-ffi.md`](c-ffi.md) |

**Platform suffixes** (module names, used consistently): `windows`, `web`, `linux`,
`apple`, `android` (split further only with an ADR).

## Dependency rules

- Apps may depend on an **unprefixed core** directly (preferred when Mediaway types are not needed).
- Apps may depend on the **capability crate** for Mediaway-typed APIs + traits; backends
  are reachable at `mediaway-<capability>::<platform>` when the target matches.
- Apps may depend on the **umbrella `mediaway`** for everything (one dependency).
- Backend modules are compile-time `#[cfg(target_os / target_family)]` — **not** Cargo
  features (ADR-0021). Cargo features remain only for non-platform optionality.
- Sans-IO crates **must not** depend on platform backend modules or on `mediaway-ffi`.
- **`mediaway-ffi` depends downward** on Rust crates; Rust cores must not depend on FFI crates.
- Unprefixed cores **must not** depend on `mediaway-common` or Mediaway facades.
- Thin Mediaway adapters over unprefixed cores live **in the capability crate**
  (e.g. `mediaway-container::mp4`), not as separate crates, unless the adapter grows
  enough to need its own crate + ADR.

## When to add a crate

| Event | Action |
|-------|--------|
| New freestanding domain core | Unprefixed crate (ADR-0012), own version |
| New container format with freestanding core | Unprefixed core + facade adapter module |
| First code for an OS backend | `src/<platform>/` module in the capability crate + its `adr/<platform>/` |
| Shared trait surface | Capability crate; don’t invent a fourth layer without ADR |

Do not create platform crates far ahead of the platform order (**Windows → Web → Linux → other**).

## Layout on disk

```text
crates/iso-cenc/                 # unprefixed reusable core (own version)
crates/iso-bmff/                 # unprefixed ISOBMFF/MP4
crates/mediaway-container/       # facade over the cores
crates/mediaway-encoder/         # src/{nvenc,windows,linux,vulkan,quicksync,web}/
crates/mediaway-device/          # src/{camera,desktop,audio,windows*,linux,web}/
crates/mediaway/                 # umbrella: src/{platform,wgpu}/ + re-exports
crates/mediaway-ffi/             # single C ABI: src/{common,container,device,pipeline}/
tools/mediaway-avcli/
```
