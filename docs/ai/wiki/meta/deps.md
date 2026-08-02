# Dependencies

Canonical: [`docs/conventions/deps-policy.md`](../../../conventions/deps-policy.md).

- Prefer std / existing / small local code over new crates
- Check license **and transitive** graph, maintenance, size/compile cost, alternatives
- Justify in the PR; ADR when heavy / codecs / FFI
- `cargo deny` must pass; no casual adds
