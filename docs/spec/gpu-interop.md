# GPU framework interop

Canonical decision: [`docs/adr/0005-gpu-interop.md`](../adr/0005-gpu-interop.md).

Mediaway **GPU** Zero-Copy uses **OS/GPU handles** (`GpuBufferHandle`). That is one half of README **⚡**; the other is **CPU↔CPU** shared buffers (audio PCM, `Bytes` share) with no payload `memcpy` — see [wiki marks](../ai/wiki/zero-copy/marks.md).

Hosts often use a **GPU framework** (wgpu, WebGPU, Dawn, …). Provide **optional adapters** so those workflows stay GPU Zero-Copy instead of forcing CPU readback.

## Rust — wgpu (priority)

| Item | Plan |
|------|------|
| Crate | `mediaway-wgpu` (when encode/decode GPU paths exist) |
| Goal | Idiomatic import/export between `wgpu::Texture` (and related) and `GpuBufferHandle` |
| Mechanism | wgpu HAL / shared handles (DX12, Vulkan, Metal) into WMF / VT / VA paths — exact recipe in that crate’s ADR |
| Dependency | Optional; **not** on the default graph of facades |

Rust apps should be able to: render or compute in **wgpu** → hand frames to Mediaway encode (or accept decode output back into wgpu) without a mandatory download.

## Other languages / stacks (“wgpu analogs”)

There is no universal “wgpu for every language.” Map to the closest **WebGPU-shaped** or **native API** stack:

| Language / host | Analog to wgpu | Mediaway interop |
|-----------------|----------------|------------------|
| **JS/TS (browser)** | **WebGPU** (`GPUDevice` / `GPUTexture`) | `GpuBufferHandle::WebGpu` on the Web track |
| **JS/TS (Node)** | Native GPU uncommon; if present, OS handles via C ABI | `*-ffi` + DX/Vulkan/Metal tokens |
| **C / C++** | **Dawn** / `webgpu.h`, or raw **Vulkan** / **D3D12** / **Metal** | C ABI passes native handles; Dawn via same tokens underneath |
| **C#** | Silk.NET, Vortice.Windows, Veldrid, etc. | P/Invoke same OS handles as Rust backends |
| **Python** | Limited; Vulkan/DX bindings or frameworks vary | Best-effort tokens; CPU path allowed when Zero-Copy bindings are missing |
| **Zig / Go / Swift / Kotlin** | Bindings to Vulkan / Metal / Dawn / WebGPU | Same handle story as C ABI + platform |
| **Unity / Godot / …** | Engine GPU textures | Guide: export native ptr / shared handle → Mediaway (mostly out-of-tree) |

### Portable mental model

```text
WebGPU-shaped:  wgpu (Rust)  ≈  WebGPU (browser)  ≈  Dawn (native)
OS-native:      D3D11/12 · Vulkan · Metal · AHardwareBuffer · …
        ↘
     GpuBufferHandle  →  Mediaway encode / decode
```

Prefer WebGPU-shaped adapters where the host already uses them; fall back to raw OS variants for codec backends that require them (e.g. WMF + DX11 texture).

## Rules

1. Adapters are **optional crates** (`mediaway-wgpu`, later others) — not baked into `mediaway-common`.
2. Never require CPU readback when a shared-handle path exists on that platform.
3. Document copy fallbacks explicitly when interop is impossible — names like `copy_…` / `readback_…`, rustdoc, and the caveat catalog ([`caveats-and-clarity.md`](caveats-and-clarity.md)).
4. FFI does not invent a cross-language “wgpu object” — it passes **handles/tokens**.
5. Schedule: after Windows GPU encode path is real; `mediaway-wgpu` follows once HAL export is designable.

### Example costly interop (must not be silent)

| Path | Why costly | Required disclosure |
|------|------------|---------------------|
| OpenGL texture → D3D11 texture | Typically a GPU↔GPU copy or CPU staging; not Zero-Copy sharing | `copy_gl_*_to_dx11` (or similar) + rustdoc + catalog row |
| GPU texture → CPU `Vec<u8>` → encoder | Readback bandwidth + latency | `readback_…` + rustdoc; never a silent default |
| Forced RGBA→NV12 via staging | Extra pass when VideoProcessor/HW path exists | Document why the fast path was unavailable |

## Related

- Handle enum draft: [`docs/ai/wiki/zero-copy/handles.md`](../ai/wiki/zero-copy/handles.md)
- Packaging: [`crate-packaging.md`](crate-packaging.md)
- C-FFI: [`c-ffi.md`](c-ffi.md)
