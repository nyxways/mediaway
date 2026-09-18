# Async and streaming

Canonical: [`docs/spec/async-and-streaming.md`](../../../spec/async-and-streaming.md) · ADR-0007.

- Streaming-first (packets/frames/chunks); whole-buffer = convenience only
- Sans-io cores = sync/poll; no Tokio-in-core by default
- Async on facades/adapters; optional runtime features

`EncodeSession` was the one facade that broke the first rule (whole-buffer `finish()`
only) until ADR-0006 — see [encode-session-byte-output](encode-session-byte-output.md).
It was also welded to one container until ADR-0007 —
[encode-session-container-choice](encode-session-container-choice.md).
