# ADR-0003: Exclude window from capture

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-windows`

## Context

Overlay / HUD windows must not appear in DXGI Desktop Duplication (or WGC) or they feedback into the recorded picture. Win32 exposes `SetWindowDisplayAffinity(..., WDA_EXCLUDEFROMCAPTURE)`.

## Decision

> Expose [`exclude_window_from_capture`](../src/capture_exclusion.rs) taking opaque `HWND` bits. No windowing crate dependency — callers pass their own hwnd.

## Consequences

- Overlay apps can opt out of capture without Mediaway owning HWND lifetime.
- Older Windows builds may reject the affinity; callers see `CaptureError::Backend`.

## References

- overlay-engine capture-exclusion pattern (reference only)
