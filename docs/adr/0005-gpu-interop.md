# ADR-0005: GPU framework interop (wgpu and analogs)

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)

## Context

Mediaway's **GPU** Zero-Copy path uses **OS/GPU native handles** (`GpuBufferHandle`). (CPU↔CPU shared-buffer Zero-Copy is separate — audio/`Bytes`; same README ⚡ when earned.) Many hosts manage GPU work through a framework (wgpu, WebGPU, Dawn, Vulkan/D3D12/Metal) rather than raw APIs.

Without explicit interop, apps are forced to CPU readback between their GPU world and Mediaway encode/decode — violating **No performance surrender**.

## Decision

> Treat **framework interop adapters** as first-class optional crates, especially a **wgpu-friendly** path for Rust. Other languages get WebGPU/Dawn/native analogs — not a single cross-language GPU abstraction inside Mediaway.

- **Native handles remain source of truth** in `mediaway-common`; frameworks wrap or export them
- **Adapters are separate crates** (e.g. `mediaway-wgpu`), feature-gated — cores do not depend on wgpu/Dawn
- **Prefer shared-handle import/export** over copies; document when copy is unavoidable
- **FFI languages** pass OS/WebGPU tokens through `*-ffi`; no fake "wgpu object" in C

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| CPU frames only between wgpu and Mediaway | Perf surrender |
| wgpu types inside `mediaway-common` | Forces wgpu on all consumers |
| Custom Mediaway GPU API instead of interop | Duplicates wgpu/WebGPU; worse ecosystem fit |

## Consequences

- Rust apps keep normal wgpu workflow and HW-encode; wgpu HAL details are version-sensitive

## References

- [`docs/spec/gpu-interop.md`](../spec/gpu-interop.md), [`docs/spec/api-layers.md`](../spec/api-layers.md)
- Wiki: [`docs/ai/wiki/zero-copy/`](../ai/wiki/zero-copy/)
