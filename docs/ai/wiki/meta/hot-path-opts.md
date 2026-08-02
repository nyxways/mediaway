# Hot-path opts

- Zero-Copy / non-alloc on hot paths by default intent
- **⚡** = GPU handle **or** shared CPU buffer ([marks](../zero-copy/marks.md)) — not “GPU only”
- Prefer move/borrow / `Bytes` share over per-frame `Vec` churn
- Vectorization-friendly loops; measure before cleverness
