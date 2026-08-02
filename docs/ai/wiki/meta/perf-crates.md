# Perf crates

Canonical: [`docs/conventions/perf-crates.md`](../../../conventions/perf-crates.md).

**In use:** `bytes` · `smallvec` · `thiserror` · `memchr` (Annex-B) · `blake3` (test digests).

**Bit packing (later):** `bitflags` (flag fields) · `bitter` (fast slice bit-reader) · `bitstream-io` (adapter `Read`/`Write` edges). Skip `bitvec` until a real bitset need.

**Vectorization:** default = auto-vec-friendly loops + `memchr`. Explicit `std::simd` / `wide` / `pulp` only after benches.

**Not yet:** `rayon` · `bumpalo` · `arrayvec` · `byteorder` · `anyhow` in libs.

Add only with a real call site + deny + deps-policy.
