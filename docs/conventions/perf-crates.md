# High-performance crate catalog

Deliberate deps for Mediaway hot paths. Process: [`deps-policy.md`](deps-policy.md). **Do not** add speculative crates.

## In use (workspace)

| Crate | Pin | Why |
|-------|-----|-----|
| `bytes` | workspace | Shared packet payloads (`Bytes`) without copy |
| `smallvec` | `1.15`, `default-features = false` | Tiny track / `trun` tables (ADR-0009) |
| `thiserror` | `2.0` | Library errors (ADR-0010) |
| `blake3` | `1.8` | Test-media digests (not a media hot path) |
| `memchr` | `2.8`, `default-features = false` | SIMD-friendly byte / substring search (Annex-B NAL scan) |
| `aes` | `0.9`, `default-features = false` | Block cipher for `iso-cenc` ClearKey CENC (ADR-0011); CTR owned in-crate |

## Approved — add at first justified call site

| Crate | When | Notes |
|-------|------|--------|
| `bitflags` | Many packed flag sets (ISOBMFF / codec) | Prefer over magic `u32` once ≥~3 flag groups |
| `bitter` | Hot **bit-reader** on `&[u8]` (Exp-Golomb, RBSP, AAC) | Fast, `no_std`-friendly; prefer over `bitstream-io` inside sans-io cores |
| `bitstream-io` | Bit read/write over `Read`/`Write` adapters | OK at I/O edges; cores prefer slice-oriented APIs (`bitter` or local) |
| `parking_lot` | Contended `Mutex`/`RwLock` in adapters | Not for sans-io cores (prefer no locks) |
| `ahash` or `hashbrown` | Hot `HashMap`/`HashSet` | Measure first; std hasher often fine |
| `bytemuck` | Pixel / POD cast boundaries | Needs careful `unsafe` / allow policy; not in sans-io by default |
| `crc32fast` | Formats that need CRC | When a container/protocol requires it |
| `std::simd` / `wide` / `pulp` | **Measured** pixel/sample loops where auto-vec fails | Explicit SIMD is an exception — see below |

## Bit packing (policy)

| Need | Prefer | Avoid initially |
|------|--------|-----------------|
| Named flag bits in a `u32`/`u64` | `bitflags` | Stringly masks scattered in call sites |
| Unaligned bit **parse** (codec headers) | `bitter` on `&[u8]` | Pulling `Read` into sans-io cores |
| Unaligned bit **emit** | Small local bit-writer, or `bitstream-io` at adapter edge | Heavy `bitvec` for one-off fields |
| Dense `bool` arrays / bitsets | Revisit `bitvec` when a real set appears | Speculative `BitVec` everywhere |

Stage 1 ISOBMFF / Annex-B framing is mostly **byte-aligned** — do not add a bit crate until RBSP / entropy / AAC raw bitstreams need it.

## Vectorization (policy)

1. **Default:** contiguous slices, predictable loops, `memchr` for scans — let LLVM auto-vectorize ([`code-style.md`](code-style.md) § Hot-path).
2. **Already in graph:** `memchr` (SIMD substring / byte search).
3. **Explicit SIMD** (`std::simd`, `wide`, `pulp`, arch intrinsics): only after a bench shows auto-vec is not enough; document path class; `unsafe` / `// SAFETY:` gates apply.
4. Do **not** add a SIMD crate “for readiness.”

## Deferred — do **not** add yet

| Crate | Why deferred |
|-------|----------------|
| `rayon` / `crossbeam-*` | No parallel core workloads yet; sans-io stays sync |
| `bumpalo` / arenas | No proven alloc hotspot |
| `arrayvec` | Prefer `smallvec` when spill is OK (ADR-0009) |
| `byteorder` | `to_be_bytes` / `from_be_bytes` enough |
| `once_cell` | Use `std::sync::OnceLock` |
| `anyhow` / `eyre` | Forbidden as public library errors (ADR-0010) |
| `tokio` (in cores) | Async only in facades/adapters (ADR-0007) |
| `bitvec` | Heavy; wait for real bitset / bitfield-collection need |
| Hand SIMD crates (unmeasured) | Prefer vectorization-friendly loops first ([`code-style.md`](code-style.md)) |

## Checklist (new perf dep)

1. Real call site in this PR (not “for later”).
2. License + `cargo deny` (incl. transitive).
3. `default-features = false` when sensible.
4. Alternative considered (std / existing / ~50 lines local).
5. Note in this file’s **In use** table when merged.

Wiki summary: [`docs/ai/wiki/meta/perf-crates.md`](../ai/wiki/meta/perf-crates.md).
