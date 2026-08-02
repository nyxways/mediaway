# Vision

Mediaway is a Rust media stack: high-level pipelines composed from **first-class low-level** surfaces (traits, sans-io cores, bitstreams, `GpuBufferHandle` / device handles).

Canonical detail: [`api-layers.md`](api-layers.md) · [`sans-io.md`](sans-io.md) · [`caveats-and-clarity.md`](caveats-and-clarity.md) · [`gpu-interop.md`](gpu-interop.md).

## Engineering context

Problems this architecture addresses:

| Problem | Effect |
|---------|--------|
| **GPU ↔ codec via CPU memory** | Frames leave VRAM (`readback`) then return (`upload`); bus cost and latency on encode/decode/render loops |
| **I/O coupled into format/bitstream cores** | Mux/demux/timebase logic tied to files or sockets; harder to reuse the same state machine on native hosts and WASM |
| **Low-level surfaces hidden behind convenience APIs** | Callers cannot compose HW sessions, GPU handles, or packet clocks without fighting the facade |
| **Device capture (camera/mic/screen) is scarce and fragmented** | Each project hand-rolls its own capture stack (DXGI/WASAPI/WGC or platform equivalents) with no shared, safe abstraction that composes with encode |
| **Zero-Copy is easy to claim, hard to use safely** | Fence/sync and buffer-lifetime handling across capture → processing → encode is typically re-implemented per project; most fall back to CPU readback "for safety" rather than risk lifetime/sync bugs |

Scope and order of work follow the roadmap and [maturity bar](maturity-bar.md) (evidence for claimed cells). Breadth is not forbidden ([ADR-0008](../adr/0008-no-narrow-vertical-mandate.md)).

## Pillars

| Pillar | Meaning |
|--------|---------|
| **Zero-Copy & HW paths** | Prefer OS codec sessions and GPU surfaces (`GpuBufferHandle`), **and** shared CPU buffers when there is no GPU path (e.g. audio). CPU readback / upload / payload `memcpy` are explicit paths, not the silent default. |
| **Sans-IO cores** | Mux, demux, bitstream, timebase, and config parsing are pure state machines; I/O lives in adapters only ([`sans-io.md`](sans-io.md)). |
| **High → low layers** | Ergonomic pipelines at the top; traits, packets/frames, and native/GPU handles underneath stay **public and usable** ([`api-layers.md`](api-layers.md)). High-level is composition, not a gate. |
| **Honest cost contracts** | Cross-API copies, readback, SW fallbacks: named APIs + rustdoc (+ catalog when cross-cutting). Code carries the contract ([`caveats-and-clarity.md`](caveats-and-clarity.md)). |

## Implications

- **Crate split:** sans-io cores, per-OS backends, facade traits ([`crate-packaging.md`](crate-packaging.md), ADR-0003).
- **C-FFI:** per-capability `mediaway-*-ffi` (+ optional umbrella) ([`c-ffi.md`](c-ffi.md), ADR-0004) — Rust remains primary.
- **GPU framework interop:** optional adapters (e.g. `mediaway-wgpu`; WebGPU/Dawn/OS-handle analogs) ([`gpu-interop.md`](gpu-interop.md), ADR-0005).
- **Platform adapters** (WMF, WebCodecs, VA-API, …) in `mediaway-*-<platform>` behind facade traits.
- Reject designs that force a slow path “for simplicity” when a Zero-Copy/HW path exists on the target — and if an escape hatch exists, **name it**.
- **Earn maturity deliberately** — corpora, oracles, benches, diagnostics for claimed scopes ([`maturity-bar.md`](maturity-bar.md)).
- **Allocate and copy deliberately** on hot paths ([`code-style.md`](../conventions/code-style.md)).
- **Streaming-first + async-capable** — incremental packet/frame/chunk APIs; sans-io cores stay sync/poll; async via facades/adapters ([`async-and-streaming.md`](async-and-streaming.md), ADR-0007).
- **ZCA where it matters** ([`zero-cost-abstractions.md`](zero-cost-abstractions.md), ADR-0009).

Roadmap platforms: **Windows → Web → Linux → other** ([`docs/roadmap.md`](../roadmap.md)).

## License & dependency boundary

Separate from the pillars above; still binding:

- Product and Cargo graph: **MIT OR Apache-2.0**. No GPL / LGPL / AGPL / SSPL / BUSL in dependencies.
- **No** FFmpeg / `libav*` / GPL codec libraries **linked or vendored** in shipped Mediaway crates.
- Software codecs via **pure Rust sans-io** only (`mediaway-sw` and codec cores; explicit opt-in).
- System `ffmpeg` / `ffprobe` on `PATH` may be used as a **test/dev oracle** only ([ADR-0002](../adr/0002-system-oracle.md)).

## Maturity

Early development — production use is **not recommended** ([`status.md`](status.md)).  
**Pre-1.0:** APIs and crate boundaries may change substantially; see [`status.md`](status.md) § API stability. Also [`maturity-bar.md`](maturity-bar.md).
