# API layers

Canonical: [`docs/spec/api-layers.md`](../../../spec/api-layers.md).

- Low-level traits, sans-io cores, types, and `GpuBufferHandle` are **first-class public APIs**.
- High-level / CLI helpers only **compose** those surfaces — never the sole entry.
- Design bottom-up; ADRs must name the public low-level surface.
- Pairs with [container sans-io](../container/sans-io.md) and [zero-copy handles](../zero-copy/handles.md).
