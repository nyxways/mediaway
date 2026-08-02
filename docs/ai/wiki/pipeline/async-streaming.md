# Async and streaming

Canonical: [`docs/spec/async-and-streaming.md`](../../../spec/async-and-streaming.md) · ADR-0007.

- Streaming-first (packets/frames/chunks); whole-buffer = convenience only
- Sans-io cores = sync/poll; no Tokio-in-core by default
- Async on facades/adapters; optional runtime features
