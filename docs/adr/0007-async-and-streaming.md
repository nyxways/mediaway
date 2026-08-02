# ADR-0007: Async support and streaming-first APIs

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)

## Context

Mediaway targets real-time and large-media workloads where buffering entire assets is wrong for latency, memory, and Zero-Copy. Callers need both sync embedders (games, plugins) and async hosts (servers, WASM event loops).

Sans-io cores already favor push/pull ([`sans-io.md`](../spec/sans-io.md)). Facades and backends need an explicit rule against batch-only or Tokio-everywhere drift.

## Decision

> Mediaway is **streaming-first** and **async-capable without a mandatory runtime in cores**.

- **Streaming-first:** packet/frame/byte-chunk incremental APIs; whole-buffer APIs are convenience only
- **Sans-io cores** stay sync/poll state machines — no async runtime dependency
- **Async** via facade/adapter layers (`Future`/`Stream`/poll) and optional features; no mandatory Tokio in library defaults
- Platform backends may use OS async internally; expose streaming contracts upward

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Sync-only forever | Blocks idiomatic server/WASM hosts |
| Tokio required in every crate | Heavy; wrong for embeds and WASM |
| Batch-first APIs | Fights Zero-Copy, latency, and large files |

## Consequences

- Clear agent/review rule; dual sync/async surfaces need careful rustdoc (ADR-0006 for awkward platforms)

## References

- [`docs/spec/async-and-streaming.md`](../spec/async-and-streaming.md), [`docs/spec/sans-io.md`](../spec/sans-io.md)
