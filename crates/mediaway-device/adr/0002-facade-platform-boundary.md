# ADR-0002: Facade vs `mediaway-device-<platform>` boundary

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device`

## Context

Workspace ADR-0003 requires separate facade and per-OS backend crates. Capture starts on **Windows** (`mediaway-device-windows`) and **Web** (`mediaway-device-web`).

## Decision

> **`mediaway-device`** = traits, configs, errors only (`forbid(unsafe_code)`).  
> **`mediaway-device-<platform>`** = OS capture sessions, `unsafe`, and platform ADRs.

| Crate | May contain | Must not |
|-------|-------------|----------|
| `mediaway-device` | Traits, `CaptureError`, configs | DXGI/MF/Web APIs, `unsafe` |
| `mediaway-device-windows` | DXGI Desktop Duplication, later MF/WASAPI | Sans-io mux, other OS APIs |
| `mediaway-device-web` | `getUserMedia` / `getDisplayMedia` | Native DXGI |

Facade default features do **not** depend on platform crates.

## References

- Workspace [`docs/adr/0003-crate-packaging.md`](../../../docs/adr/0003-crate-packaging.md)
