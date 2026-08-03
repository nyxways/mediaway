# ADR-0002: `windows` crate for Media Foundation

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder-windows`

## Context

WMF H.264 encode needs COM/`IMFTransform` bindings. Options: hand-written `windows-sys` FFI, or the official [`windows`](https://crates.io/crates/windows) projection (MIT OR Apache-2.0).

## Decision

> Depend on **`windows` 0.62** (workspace-pinned) as a **`cfg(windows)`** target dependency with a **narrow feature set** (`Win32_Foundation`, `Win32_System_Com`, `Win32_Media_MediaFoundation`). No default features.

### Why not hand FFI / `windows-sys` alone

- MF surface is large; generated bindings track Windows SDK metadata.
- Same license allow-list as the rest of the workspace (`deny.toml`).

### Stage 1 encode path

1. Prefer **`CLSID_CMSH264EncoderMFT`** (sync inbox encoder) + CPU NV12 `upload_cpu_nv12_to_mft` (documented copy).
2. Hardware async MFT + DX11 Zero-Copy follows in a later change (separate ADR if event-driven unlock differs).

Non-Windows targets compile this crate as stubs (`EncodeError::Unsupported`).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Raw `windows-sys` only | More boilerplate for MF media types / COM QI |
| Third-party MF wrappers | Extra policy surface; prefer first-party `windows` |

## Consequences

### Positive

- Idiomatic COM; feature-gated link surface

### Negative / Trade-offs

- Large compile graph on Windows (mitigated by narrow features)

## References

- Deps policy: [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md)
- Facade boundary: `mediaway-encoder` ADR-0002
