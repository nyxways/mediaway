# Crate packaging

Canonical decisions:

- Structure: [`docs/adr/0003-crate-packaging.md`](../adr/0003-crate-packaging.md)
- Naming **v1**: [`docs/adr/0012-unprefixed-reusable-cores.md`](../adr/0012-unprefixed-reusable-cores.md)

## Library crate kinds

```text
[unprefixed core]  iso-cenc, iso-bmff, …   ← no Mediaway types

[facade]           mediaway-container, …
        traits + Mediaway-typed adapters over unprefixed cores

[platform backend] mediaway-<capability>-<platform>
        e.g. mediaway-encoder-windows, mediaway-device-web
```

## Naming (v1)

| Kind | Pattern | Example |
|------|---------|---------|
| Reusable domain core | Unprefixed short name | `iso-cenc`, `iso-bmff` |
| Shared product types | `mediaway-common` | — |
| Capability facade | `mediaway-<capability>` | `mediaway-container` |
| I/O adapter (optional) | `mediaway-<name>-<adapter>` | `mediaway-container-std` |
| GPU framework adapter | `mediaway-wgpu`, … | [`gpu-interop.md`](gpu-interop.md) |
| OS/runtime backend | `mediaway-<capability>-<platform>` | `mediaway-device-windows` |
| C ABI (per capability) | `mediaway-<capability>-ffi` | [`c-ffi.md`](c-ffi.md) |
| C ABI (shared types) | `mediaway-common-ffi` | Optional |
| C ABI (umbrella) | `mediaway-ffi` | Feature-gated aggregate |

**Platform suffixes** (use consistently): `windows`, `web`, `linux`, `apple`, `android` (split further only with an ADR).

## Dependency rules

- Apps may depend on an **unprefixed core** directly (preferred when Mediaway types are not needed).
- Apps may depend on the **facade** for Mediaway-typed APIs + traits.
- Facades **must not** default-enable all platforms.
- Sans-IO crates **must not** depend on platform backend crates or on any `*-ffi` crate.
- **`*-ffi` / `mediaway-ffi` depend downward** on Rust crates; Rust cores must not depend on FFI crates.
- Platform backends depend on `mediaway-common` + the facade’s traits (or a tiny `mediaway-<capability>-api` if traits need to split later — avoid until needed).
- Unprefixed cores **must not** depend on `mediaway-common` or Mediaway facades.
- Thin Mediaway adapters over unprefixed cores live **in the facade** (e.g. `mediaway-container::mp4`), not as a separate `mediaway-container-<format>` crate, unless the adapter grows enough to need its own crate + ADR.

## When to add a crate

| Event | Action |
|-------|--------|
| New freestanding domain core | Unprefixed crate (ADR-0012) |
| New container format with freestanding core | Unprefixed core + facade adapter module |
| First code for an OS backend | `mediaway-<capability>-<platform>` + its `docs/` / `adr/` / roadmap |
| Shared trait surface | Facade crate; don’t invent a fourth layer without ADR |

Do not create empty platform crates far ahead of the platform order (**Windows → Web → Linux → other**).

## Layout on disk

```text
crates/iso-cenc/                 # unprefixed reusable core
crates/iso-bmff/                 # unprefixed ISOBMFF/MP4
crates/mediaway-container/       # traits + Mediaway MP4 surface
crates/mediaway-device/
crates/mediaway-device-windows/   # when Windows capture starts
…
tools/mediaway-avcli/
crates/mediaway-container-ffi/   # when added
crates/mediaway-ffi/             # optional feature-gated umbrella
```
