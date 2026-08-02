# Zero-cost abstractions (ZCA)

Canonical: [`docs/spec/zero-cost-abstractions.md`](../../../spec/zero-cost-abstractions.md) · [ADR-0009](../../../adr/0009-zero-cost-abstractions.md).

**ZCA ≠ Zero-Copy** — abstractions compile away vs avoid data copies (GPU **or** shared CPU — [marks](../zero-copy/marks.md)).

- **Before non-trivial Rust:** sketch types / ownership / typestate / alloc sites in chat, then code.
- Prefer `enum` · generics · typestate · concrete types over `Box` / `dyn` on hot / sans-io paths.
- `Box` only with rustdoc/ADR reason.
- **`smallvec`:** tracks ≤4 / sample rows ≤32 in `iso-bmff`.
- **`memchr`:** Annex-B start-code scan — [perf-crates](perf-crates.md).
- Aligns with [alloc-discipline](alloc-discipline.md) · [hot-path-opts](hot-path-opts.md).
