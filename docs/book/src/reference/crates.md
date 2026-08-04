# Crate Map

Pulled directly from the project README.

{{#include ../../../../README.md:crates}}

OS backends live as `#[cfg]`-gated modules inside the facade crates
(`mediaway-encoder`, `mediaway-decoder`, `mediaway-device`); per-crate API docs are on
[Crate Docs](./crate-docs.md). Platform order and stages: [`docs/roadmap.md`](https://github.com/nyxways/mediaway/blob/main/docs/roadmap.md)
in the repository.
