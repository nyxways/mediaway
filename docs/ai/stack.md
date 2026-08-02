# Project stack

- **Language / build:** Rust, edition 2024, stable toolchain, Cargo workspace
- **Vision:** Zero-Copy (GPU **or** shared CPU) · HW · sans-io · high→low · honest costs; license as boundary ([`docs/spec/vision.md`](../spec/vision.md)); low-level APIs first-class ([`api-layers.md`](../spec/api-layers.md))
- **Status:** early development — **not for production** ([`status.md`](../spec/status.md)); earn trust via [`maturity-bar.md`](../spec/maturity-bar.md)
- **License:** MIT OR Apache-2.0 (product and dependency graph — no FFmpeg **crates**/link). System ffmpeg OK as test oracle ([ADR-0002](../adr/0002-system-oracle.md))
- **Media backends (target):** build order **Windows → Web → Linux → other**
  - Windows: WMF + DX11/DX12 GPU Zero-Copy · WASAPI (CPU ZC when shared buffers land)
  - Web: WebCodecs + WebGPU
  - Linux: VA-API / Vulkan Video
  - Other: VideoToolbox / AudioToolbox + Metal; MediaCodec + AHardwareBuffer
- **Containers:** **Sans-IO** Pure Rust mux/demux cores (MP4 / WebM); I/O via adapters only — [`docs/spec/sans-io.md`](../spec/sans-io.md)
- **Crate packaging:** sans-io · facade · `mediaway-*-<platform>` backends — [`crate-packaging.md`](../spec/crate-packaging.md)
- **C-FFI:** per-capability `*-ffi` + optional feature umbrella — [`c-ffi.md`](../spec/c-ffi.md)
- **GPU interop:** wgpu (Rust) · WebGPU/Dawn/OS handles elsewhere — [`gpu-interop.md`](../spec/gpu-interop.md)
- **Zero-Copy marks:** README **⚡** — [wiki/zero-copy/marks](wiki/zero-copy/marks.md)
- **Caveats:** document costly paths; code+rustdoc carry contract — [`caveats-and-clarity.md`](../spec/caveats-and-clarity.md)
- **API shape:** public traits · packets/frames · typed GPU handles · shared CPU buffers; convenience wrappers optional
- **SW fallback:** pure Rust sans-io (`mediaway-sw`); no C codec FFI
- **Quality gates:** lefthook, cargo deny, clippy `-D warnings`

Design: [`docs/spec/`](../spec/) · Decisions: [`docs/adr/`](../adr/) · Knowledge: [`wiki/index.md`](wiki/index.md)
