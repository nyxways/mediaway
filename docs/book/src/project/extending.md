# Extending the Book

This page is the checklist for adding a **new package (crate)** to the Mediaway mdBook,
and for adding **per-language examples** and **reference entries**. The book is the
human-facing narrative layer; the README, `docs/spec/`, crate `docs/roadmap.md`, and
`adr/` remain the source of truth for facts.

## Adding a new crate

1. **Scaffold** the crate per the workspace layout:
   `crates/<name>/` with `README.md` (see the
   [`crate-readme-template`](https://github.com/nyxways/mediaway/blob/main/docs/conventions/crate-readme-template.md)),
   `docs/roadmap.md`, and `adr/` (crate-local ADRs).
2. **Cargo manifest**: add the crate to `[workspace].members` and
   `[workspace.dependencies]` (with its version). For publishable crates set
   `publish = true`, a user-facing `description` (no internal jargon, no ADR
   references), and an independent version if the crate is a freestanding core.
3. **Root README**: add a row to the Crates table *between the
   `<!-- ANCHOR: crates -->` markers*, and update any support-matrix cell the crate
   touches (codec / container / device tables). The book's [Crate Map](./crates.md)
   and matrix pages include these anchors — this one edit syncs them all.
4. **Workspace roadmap**: add the crate to the crate-roadmaps index table in
   [`docs/roadmap.md`](https://github.com/nyxways/mediaway/blob/main/docs/roadmap.md).
5. **Book SUMMARY**: add pages under the matching section — a guide for a new
   capability, a reference entry, and examples pages.
6. **Reference**: add the crate to [Crate Docs](./crate-docs.md) once it is
   published (docs.rs link).
7. **Guides**: extend an existing `guides/<capability>.md` or add a new one.
   Guides are **hand-written and teaching-focused** — do not `{{#include}}` code from
   `examples/`; the runnable examples stay the compiling source of truth and the guide
   links to them.
8. **Examples**: add a Rust example under `examples/` (workspace member, one
   capability per file) and mirror it per language under
   `bindings/<lang>/examples/<capability>/` when the crate has user-facing APIs worth
   demoing. Register the files in the language's Examples page.
9. **Wiki**: add or update a `docs/ai/wiki/` page (agent knowledge — Rule 0 upkeep).

## Adding a per-language example

- Mirror the Rust example's capability and file name (`mux_roundtrip`, `screen_record`,
  …) in `bindings/<lang>/examples/<capability>/`.
- Keep it runnable and verified; the binding's README documents the build/run steps.
- Add the file to the language page under [Examples](../examples/index.md) and the
  root README's binding table if it demonstrates a new capability.

## Reference support rules

- Support-matrix pages are `{{#include}}`d from root README anchors — edit the README,
  not the book page, and keep the `<!-- ANCHOR: … -->` comments in sync.
- Per-crate API docs live on docs.rs; [`Crate Docs`](./crate-docs.md) only links them.
- Facts (status, versions, platform support) belong in README / `docs/spec/` / crate
  roadmaps — the book narrates, it does not duplicate.
