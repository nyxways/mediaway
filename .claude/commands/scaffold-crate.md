---
description: Scaffold crates/mediaway-<name> with docs/ and adr/, register in workspace.
argument-hint: <crate-suffix>
---

Scaffold `mediaway-$1`.

Read [`docs/spec/crate-packaging.md`](../../docs/spec/crate-packaging.md) first — choose kind: **sans-io core**, **facade**, or **platform backend** (`mediaway-<capability>-<platform>`).

1. Read `docs/conventions/docs-layout.md`
2. `crates/mediaway-$1/Cargo.toml` — workspace inheritance + lints
3. `src/lib.rs` — English `//!` docs (state kind: sans-io / facade / platform)
4. Create `README.md`, `docs/roadmap.md`, `adr/README.md`, `adr/template.md`
5. Add to root `Cargo.toml` `members` (+ `[workspace.dependencies]` path if needed)
6. Update `docs/roadmap.md` index + wiki `meta/crate-map.md`
7. `cargo check -p mediaway-$1`
8. If a design decision is ready, `/adr mediaway-$1 <title>`

Do not create empty platform crates ahead of the platform order. Chat in the user's language; artifacts in **English**.
