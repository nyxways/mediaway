# Mediaway overview

## Status

**Early development.** Do not use in production yet — [`status.md`](status.md).  
Pre-1.0: public APIs and crate boundaries may change without a stability guarantee.

## Vision

See [`vision.md`](vision.md): Zero-Copy (GPU **or** shared CPU) / HW paths, sans-io cores, high→low layers, honest cost contracts; license and dependency rules as a separate boundary. Marks: [`../ai/wiki/zero-copy/marks.md`](../ai/wiki/zero-copy/marks.md).

## Goal

Provide a Rust stack that:

1. Encodes / decodes AV using **OS-native / GPU APIs** and `GpuBufferHandle` when the backend allows (CPU readback/upload only as explicit paths)
2. Muxes / demuxes containers (MP4, WebM, …) in **sans-io** cores (I/O via adapters only) — [`sans-io.md`](sans-io.md)
3. Exposes facade + platform APIs across Windows, macOS, iOS, Android, Linux, Web/WASM (platform order: roadmap)
4. Lets callers work at **high level** or **low level** (HW session, GPU handle, sans-io mux) — low-level surfaces are **first-class** ([`api-layers.md`](api-layers.md))
5. Keeps convenience APIs as thin composition over those surfaces — never the only way in
6. Keeps the Cargo graph **MIT OR Apache-2.0** with no `libav*` / GPL codec linkage in shipped crates ([`vision.md`](vision.md) § License & dependency boundary)

## Pipeline

```
[sans-io crates]  demux / mux / bitstream / timebase / config parse
[adapter crates]  file · OPFS · network  (optional; outside cores)

[facade crates]   mediaway-device | encoder | decoder   (traits)
[platform crates] mediaway-*-windows | *-web | *-linux | …
```

Packaging: [`crate-packaging.md`](crate-packaging.md) (ADR-0003).

Logical flow:

```
[device | demuxer] → packets → [decoder] → frames (Cpu | Gpu)
                                              ↓
                                         (app / compositor)
                                              ↓
[muxer] ← packets ← [encoder] ← frames (Cpu | Gpu)
```

AV CLI compat (`mediaway-avcli` / `mediaway-avprobe`) are **separate tool crates**, not part of the library API. They are **not affiliated with the FFmpeg project**.

## MVP order

Platform priority: **Windows → Web → Linux → other**.

1. `mediaway-common` — types
2. Windows — `mediaway-encoder-windows` (+ facade traits) + DX11 Zero-Copy + sans-io MP4 mux (+ decode round-trip)
3. Web — `mediaway-*-web` (WebCodecs + WebGPU handles)
4. Linux — `mediaway-*-linux` (VA-API / Vulkan Video)
5. Other — `mediaway-*-apple`, `mediaway-*-android`, …
6. Device / SW fallback / CLI (after core platforms as needed)
7. C ABI — `mediaway-*-ffi` (optional feature-gated `mediaway-ffi`) after useful Rust surfaces exist ([`c-ffi.md`](c-ffi.md))
8. GPU framework adapters — `mediaway-wgpu` etc. ([`gpu-interop.md`](gpu-interop.md)), after Windows GPU encode path exists

See [`docs/roadmap.md`](../roadmap.md) (index) and each crate’s `docs/roadmap.md`.

## Non-goals (v0)

- Legacy codecs (WMV, MPEG-2, …)
- Bundling GPL encoders
- Shipping a “simple” API that permanently hides or disables Zero-Copy/HW paths
- Making low-level traits/handles `pub(crate)`-only while only shipping high-level wrappers
- Stabilizing the C ABI before Rust MVP capabilities exist
- Official per-language SDKs in the first FFI milestone (C ABI is enough)
- Treating browser JS/TS and Node JS/TS as the same interop path
- Claiming production readiness without maturity-bar evidence
