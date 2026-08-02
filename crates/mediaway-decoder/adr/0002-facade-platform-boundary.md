# ADR-0002: Facade vs `mediaway-decoder-<platform>` boundary

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder`

## Context

Workspace ADR-0003: facade must not pull WMF into every consumer.

## Decision

> **`mediaway-decoder`** = traits, configs, errors (`forbid(unsafe_code)`).  
> **`mediaway-decoder-<platform>`** = OS/GPU decode sessions and platform ADRs.

Facade default features do **not** depend on platform crates. Windows Stage 1: HW H.264 → DX11 `GpuBufferHandle` out.

## Capability probe added (2026-07-31)

`mediaway_decoder::capability::{DecodeSupport, DecodeUnavailable}` mirrors
`mediaway-encoder`'s capability probe (ADR-0004's 2026-07-31 addendum), itself modeled
on `mediaway-device` ADR-0003. Reports a single `DecodeSupport` per codec, not a `Vec`
of backend rows: decode has exactly one implementation per platform today, so
enumerating a `Backend`-style fleet would be a hollow abstraction (no second backend to
compete — `mediaway-decoder-vulkan` is real but not wired into
`mediaway_pipeline::platform::AutoDecoder::open`). Dispatched from
`mediaway_pipeline::platform::decoder_support(codec)`, same compile-time OS filtering
as the encoder side (non-Windows/non-Linux never opens a session).

## References

- Workspace [`docs/adr/0003-crate-packaging.md`](../../../docs/adr/0003-crate-packaging.md)
