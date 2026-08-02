# Alloc discipline

- Hot paths: move/borrow, reuse buffers, Zero-Copy handoffs
- GPU: `GpuBufferHandle` pass-through; CPU: shared PCM / `Bytes` (same ⚡ when earned)
- Every production `.clone()` needs `// clone:` ([code-style](../../../conventions/code-style.md))
- Payload `memcpy` into a new `Vec` is **copy**, not ⚡
