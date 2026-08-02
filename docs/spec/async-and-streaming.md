# Async and streaming-first

Mediaway APIs prefer **incremental streaming** (packets, frames, byte chunks) over whole-media buffers, and support **async** without forcing a single runtime into cores.

Canonical ADR: [`docs/adr/0007-async-and-streaming.md`](../adr/0007-async-and-streaming.md).

## Streaming-first

| Prefer | Avoid as the only path |
|--------|-------------------------|
| Push/pull of packets, frames, or byte slices | “Give me the whole file as `Vec<u8>`” as the sole API |
| Progressive demux / mux / encode / decode | Must-buffer-entire-container cores |
| Backpressure-aware pull (`poll` / `try_pull` / async `Stream`) | Unbounded internal queues with no escape hatch |
| Chunked adapters over files, sockets, OPFS | Silent full-file reads inside sans-io cores |

Whole-buffer helpers may exist as **convenience** layers composed from streaming cores — never as the only public surface ([`api-layers.md`](api-layers.md)).

## Async support

| Layer | Async policy |
|-------|----------------|
| Sans-IO cores (mux/demux/bitstream/config) | **Sync / poll state machines** — no Tokio (or other runtime) in the core crate |
| Facade traits (`Encoder` / `Decoder` / device) | Offer **streaming** sync *and* async shapes where the platform allows; document which |
| Platform backends | Drive OS/GPU callbacks; expose async via adapters or `Future`-returning methods as needed |
| I/O adapters (fs, network, OPFS) | Async-friendly wrappers around sans-io cores; runtime optional via features |

**Do not** hard-require `tokio` (or any executor) in default features of library crates. Prefer `core`/`std` futures and optional runtime features when an executor is truly needed. Justify new async deps with [`deps-policy.md`](../conventions/deps-policy.md).

## Rules

1. New capability ADRs must say whether the public surface is streaming, batch-only (discouraged), or both — and how async is exposed.
2. Blocking “read entire media” may only live in convenience modules/examples, built on streaming cores.
3. C-FFI and WASM hosts get streaming-friendly handle/callback or poll APIs; do not assume a Rust async runtime behind the FFI boundary.
4. Aligns with sans-io push/pull ([`sans-io.md`](sans-io.md)) and Zero-Copy handoffs ([`gpu-interop.md`](gpu-interop.md) for GPU; shared CPU/`Bytes` for audio — [wiki marks](../ai/wiki/zero-copy/marks.md)).

## Anti-patterns

- Core demuxer that only returns `Vec<Packet>` for the whole file.
- Facade that is async-only and unusable from a sync embedder (or the reverse with no async path for servers).
- Pulling Tokio into `iso-bmff` / `mediaway-common` by default.
