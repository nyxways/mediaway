# ADR-0002: Facade vs `mediaway-encoder-<platform>` boundary

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder`

## Context

Workspace ADR-0003 requires separate facade and per-OS backend crates. Encoder work starts on **Windows** (`mediaway-encoder-windows`). The facade must not pull WMF/`windows-sys` into every consumer.

## Decision

> **`mediaway-encoder`** = traits, configs, errors, docs only (`forbid(unsafe_code)`).  
> **`mediaway-encoder-<platform>`** = OS/GPU sessions, `unsafe`, link flags, and platform ADRs.

| Crate | May contain | Must not |
|-------|-------------|----------|
| `mediaway-encoder` | Traits, `EncodeError`, configs | WMF/VT/WebCodecs calls, `unsafe`, default platform deps |
| `mediaway-encoder-windows` | MF transform / sink writer, DX11 texture push | Sans-io mux logic, other OS APIs |
| `mediaway-encoder-web` (later) | WebCodecs | Native MF |

### Dependency rules

1. Platform crates depend on `mediaway-encoder` + `mediaway-common`.
2. Facade **default features** do **not** depend on any platform crate.
3. Optional facade feature `windows` may re-export `mediaway-encoder-windows` for batteries apps — **only if** it does not create a Cargo cycle; today Windows `AutoVideoEncoder::open` lives in the platform crate instead.
4. FFI (`mediaway-encoder-ffi`) wraps platform or facade later — not inside the facade lib.

### Windows Stage 1 target (backend crate ADR)

- WMF H.264 encode; DX11 `GpuBufferHandle::DirectX11` Zero-Copy push; then AAC.
- Until MF is wired, the Windows crate may compile a typed stub that returns [`EncodeError::Unsupported`](../src/error.rs) — no silent software encode.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `cfg(windows)` modules inside the facade | Violates ADR-0003; WASM/native fight |
| Empty platform crates for every OS now | Stub spam; add when work starts |

## Consequences

### Positive

- Clean graphs; web apps never link MF

### Negative / Trade-offs

- More crates; feature matrix for “just work” apps is explicit

## References

- Workspace [`docs/adr/0003-crate-packaging.md`](../../../docs/adr/0003-crate-packaging.md)
- Crate roadmap Stage 1 Windows
