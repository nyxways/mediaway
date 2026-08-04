# Crate README template

Every crate in this workspace (and published on crates.io) carries a `README.md`
written from this template. The template mirrors the root [`README.md`](../../README.md)
style: badges, a status callout, a real code snippet, and an honest status table.

## Writing rules

1. **User-facing, not architecture-facing.** Readers are developers picking the crate up
   from crates.io/docs.rs. Say what the crate *does* and how to use it. Do **not** use
   internal packaging vocabulary — "freestanding", "facade", "unprefixed", "no Mediaway
   types" — and do **not** link to ADRs from the README. ADR references belong in the
   crate's [`docs/roadmap.md`](../../crates/mediaway-common/docs/roadmap.md) only.
2. **Sans-IO is a user-facing property** (pure state machine, caller owns I/O) — keep it.
   Zero-Copy (`⚡`) claims must be honest per the root README's legend.
3. **Snippets must be real.** Copy minimal usage from the crate's tests, `examples/`, or
   the root README's verified snippets. Never invent APIs.
4. **Status table is mandatory.** Every row gets one of the shared marks below; "planned"
   and "blocked" must be distinguishable.
5. English only (workspace language policy).
6. Keep it short — a `README.md` is an overview, not the crate's docs. `docs/roadmap.md`
   holds stages, deferred items, and design decisions.

## Shared status marks

Same legend as the root README — keep it in sync when it changes there.

| Mark | Meaning                                                                       |
| ---- | ----------------------------------------------------------------------------- |
| ✅    | First-class (tests for claimed scope)                                         |
| ⚡    | Zero-Copy path — **no payload `memcpy`** (GPU handle **or** shared CPU buffer; implies ✅) |
| 🆗   | Best-effort / prototype                                                       |
| 🛠️  | Planned                                                                       |
| ❌   | Attempted and genuinely blocked — no upstream API to build on, a hard version/license conflict, or a real query returned "unsupported" |
| 👻   | Not exercisable yet — license/patent blocked, no target hardware, or no device/daemon/session available to run otherwise-tested code against |

## Template body

````markdown
# <crate-name>

<p align="center">
  <a href="https://docs.rs/<crate-name>"><img src="https://img.shields.io/docsrs/<crate-name>" alt="docs.rs"></a>
  <a href="https://crates.io/crates/<crate-name>"><img src="https://img.shields.io/crates/v/<crate-name>.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

One or two sentences: what the crate does, who uses it, and the property that matters to
the caller (Sans-IO push/poll shape, platform scope, Zero-Copy out, …).

## Quick start

```rust
// Minimal real usage — copied from the crate's tests/examples; must compile as written.
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| …    | ✅      | What works today |
| …    | 🛠️     | What is planned |
| …    | ❌ / 👻 | What is blocked / not exercisable yet |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- Root [README](../../README.md) — codec/container/device support matrices across all crates

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
````
