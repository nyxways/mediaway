# Benchmarking

Canonical: [`docs/conventions/benchmarking.md`](../../../conventions/benchmarking.md).

- Label paths: `zc` · `copy` · `readback` · `sw` · `pure`
- **`zc`** = GPU-resident **or** shared CPU (no payload memcpy) — [marks](../zero-copy/marks.md)
- **`machine_id`** on every published row — [`machines.md`](../../../benchmarks/machines.md)
- **`oracle_ref`** beside Mediaway when comparable; fair timing + **same class** (`zc` vs `zc`)
- Official baselines only from `ref-*` profiles
- Default CI stays light; HW benches optional
- Never sell a copy path as Zero-Copy in bench names or claims
