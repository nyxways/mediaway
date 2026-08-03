# Objective

- What this PR achieves (English).
- Link issues with `Fixes #N` when applicable.
- Link ADR/spec/wiki when design changed.

## Solution

- How it works at a high level.
- Call out streaming vs batch, sync vs async, Zero-Copy vs copy/readback if relevant.

## Testing

- Commands run (`cargo test`, `-p <crate>`, optional PATH oracle).
- Gaps / platforms not tested (Windows-first when it matters).
- Bindings: per-language example re-verified against the real DLLs/wasm when the
  change touches a binding (scope `binding-<lang>`) — or N/A.

## Checklist

Details: [`docs/contributing/pull-requests.md`](../docs/contributing/pull-requests.md).

- [ ] Own diff reviewed
- [ ] English commits + PR text
- [ ] fmt / clippy `-D warnings` / tests (or explained)
- [ ] No secrets / test-media blobs / GPL·FFmpeg crates / `local/` scratch
- [ ] Source files ≤1000 lines
- [ ] `unsafe` justified (`// SAFETY:` + ADR when new backend)
- [ ] New deps deliberate ([`deps-policy.md`](../docs/conventions/deps-policy.md))
- [ ] Costly paths named + rustdoc; streaming/async policy respected
- [ ] Hot paths: no casual alloc/copy; vectorization-friendly when it matters (not intrinsic-soup by default)
- [ ] Docs/ADR/wiki updated when public behavior changed

## Notes (optional)

Perf numbers need `machine_id` + fair `oracle_ref` ([`benchmarking.md`](../docs/conventions/benchmarking.md)).

---

Release Notes: N/A | Added/Fixed/Improved …
