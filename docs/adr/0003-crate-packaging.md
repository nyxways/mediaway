# ADR-0003: Crate packaging — sans-io cores, per-OS backends, facade crates

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)

## Context

Mediaway needs portable sans-io logic, OS/GPU backends, and unified traits. Backends inside one mega-crate via `cfg` force unwanted platform deps, bloated features, and weak boundaries. File/OPFS I/O in sans-io cores fights [`sans-io.md`](../spec/sans-io.md) and [`api-layers.md`](../spec/api-layers.md).

## Decision

> **Split by responsibility:** sans-io cores alone; one crate per OS/runtime backend; one facade per capability (shared traits, optional re-exports).

- **Device/encode/decode:** `mediaway-<capability>` (facade) + `mediaway-<capability>-<platform>` backends
- **Container:** `iso-bmff` / `iso-cenc` (unprefixed sans-io, ADR-0012); `mediaway-container` facade with Mediaway-typed `mp4`
- **Rules:** new sans-io → own crate; new backend → `mediaway-<capability>-<platform>` when code lands; facade owns cross-platform traits, no hard-dep on every platform; FFI/`unsafe` in platform crates; optional I/O adapters (`*-fs`, OPFS) never folded into cores
- **Do not:** add `mediaway-muxer`/`mediaway-demuxer` shims or `mediaway-container-mp4`; platform `cfg` sessions in container cores

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Single crate + `cfg(target_os)` | Dep/feature spaghetti; WASM and native fight |
| Facade-only, backends as modules | `unsafe` and link flags leak |
| Platform crates without facade | Apps reimplement selection; traits drift |

## Consequences

- Clean dependency graphs (web need not link WMF); sans-io stays pure; more crates/docs to maintain

## References

- [`docs/spec/crate-packaging.md`](../spec/crate-packaging.md), [`0012-unprefixed-reusable-cores.md`](0012-unprefixed-reusable-cores.md)
- [`docs/spec/sans-io.md`](../spec/sans-io.md), [`docs/spec/api-layers.md`](../spec/api-layers.md)
