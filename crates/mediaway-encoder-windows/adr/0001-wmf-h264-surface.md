# ADR-0001: WMF H.264 / AAC encode surface (Windows)

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder-windows`

## Context

Stage 1 Windows encode needs Media Foundation for H.264 (then AAC) and DX11 textures for Zero-Copy. The facade traits are fixed in `mediaway-encoder` ADR-0001. This crate owns `unsafe` MF/DX calls.

## Decision

> Expose concrete [`WindowsVideoEncoder`](../src/lib.rs) via [`WindowsVideoEncoder::open`](../src/lib.rs). Stage 1 uses sync **`CLSID_MSH264EncoderMFT`** with CPU NV12 (`upload_cpu_nv12`).

### Stage 1 order

1. ~~MF H.264 from CPU NV12 (honest upload path named in rustdoc).~~ **Done**
2. DX11 `GpuBufferHandle::DirectX11` Zero-Copy push (preferred default).
3. MF AAC.
4. Smoke with `mediaway-container` + `mediaway-test-media`.

No silent third-party SW encode in this crate (`mediaway-sw` remains opt-in elsewhere).

### `unsafe`

`#![allow(unsafe_code)]` on Windows modules; `// SAFETY:` on every `unsafe` block. Workspace lint is `deny` so platforms can allow.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Sink Writer file-only API | Fights streaming packet output into mux |
| Soft encode inside this crate | License/perf honesty; use `mediaway-sw` explicitly |
| Hardware async MFT first | Event-driven unlock is larger; follow after sync path |

## Consequences

### Positive

- Working H.264 packets on Windows; clear upload caveat

### Negative / Trade-offs

- Sync inbox MFT is not the HW Zero-Copy path yet

## References

- Facade ADR-0001 / ADR-0002 · this crate ADR-0002 (`windows` dep)
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md)
