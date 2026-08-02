# ADR-0001: Web device capture policy

## Status

Accepted

## Context

Browsers expose `getUserMedia` and `getDisplayMedia` with mandatory user consent UI.
There is no HWND / stable device ID API for silent or programmatic targeting.

## Decision

- `mediaway-device-web` wraps browser APIs only.
- `deviceId` and `displaySurface` are **preference hints** on optional config structs.
- Public wasm exports document the caveat in rustdoc and `device_selection_policy()`.

## Consequences

- Windows DXGI backends may offer programmatic window capture; Web does not.
- Playwright E2E uses fake media streams; manual tests use real pickers.
