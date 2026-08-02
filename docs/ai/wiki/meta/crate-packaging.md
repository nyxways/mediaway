# Crate packaging

Canonical: [`docs/spec/crate-packaging.md`](../../../spec/crate-packaging.md) · ADR-0003 · naming v1 [ADR-0012](../../../adr/0012-unprefixed-reusable-cores.md).

- Unprefixed domain cores when usable without Mediaway types (`iso-bmff`, `iso-cenc`, …).
- Facade → `mediaway-<capability>` (traits + thin Mediaway adapters).
- OS backends → `mediaway-<capability>-<platform>`, …
- Create platform crates when that backend’s code starts (Windows first).
