# Code style

## Rust

| Item | Value |
|------|-------|
| Edition | **2024** |
| MSRV (`rust-version`) | **1.96** |
| Toolchain | `stable` (`rust-toolchain.toml`) |

## Workspace lints

`[workspace.lints.*]` in root `Cargo.toml` is the source of truth.

- `unsafe_code = deny` by default (platform crates may `#![allow(unsafe_code)]`; `forbid` cannot be overridden)
- `unwrap_used` / `expect_used` / `panic` / `todo` / `dbg_macro` = deny
- `print_stdout` / `print_stderr` = deny (library crates)

### `unsafe` exceptions (Mediaway)

Platform FFI is unavoidable. Allow only when:

1. Crate root has `#![allow(unsafe_code)]` (overrides workspace `deny`)
2. Every `unsafe` block has a `// SAFETY:` comment (invariants / call contract)
3. The backend ADR documents the unsafe boundary

Examples: `mediaway-encoder` Windows WMF modules, `*-sys` crates.

**Comments and `// SAFETY:` prose: English only.**

## File size (source)

Staged source files (`.rs`, C/C++ headers/sources, TS/JS, Go, Zig, …) must be **≤1000 lines**. Enforced by `tools/hooks/forbid-long-source.sh` on pre-commit.

Prefer splitting modules over growing mega-files. Override only with explicit approval and `[skip-hooks: …]` (discouraged). Env `MEDIAWAY_MAX_SOURCE_LINES` can change the limit for local experiments — do not rely on that for commits to `main`.

## Unit test placement

Inline `#[cfg(test)] mod tests { … }` is **forbidden**. Put unit tests in a sibling file and declare only:

```rust
#[cfg(test)]
#[path = "foo_tests.rs"]
mod tests;
```

```rust
// foo_tests.rs
#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
```

- Root `lib.rs` → `lib_tests.rs`.
- Integration / oracle / conformance stay under `tests/` ([testing.md](testing.md)).
- Rare escape: `tests/unit_*.rs` when a file-level inner attribute cannot live on a sibling module.

## Allocation, clone, and copy discipline

Hot paths (encode/decode/mux, packet/frame pumps, GPU interop) treat **extra work as a bug until justified**. Aligns with **No performance surrender**, Zero-Copy ([`vision.md`](../spec/vision.md)), and **zero-cost abstractions** ([`zero-cost-abstractions.md`](../spec/zero-cost-abstractions.md) · ADR-0009).

| Prefer | Avoid on hot paths (unless measured/justified) |
|--------|--------------------------------------------------|
| Move / borrow (`&`, `&mut`, ownership transfer) | Habitual `.clone()` / `.to_owned()` “to make it compile” |
| `Bytes` / shared buffers / arenas / pre-sized `Vec` | Per-packet / per-frame fresh `Vec` + copy |
| `SmallVec<[T; N]>` when length is usually ≤ `N` | Tiny lists as always-heap `Vec` (see ZCA spec) |
| `Arc` / `Rc` when sharing is real | Clone large payloads to fake sharing |
| In-place updates, reuse scratch buffers | Allocate → fill → discard every iteration |
| GPU handle pass-through | CPU mirror “just in case” |
| Explicit `copy_…` / `readback_…` APIs | Silent `memcpy` inside innocent helpers |
| Enums / generics / typestate (ZCA) | Hot-path `Box` / `dyn` without justification |

### Rules

1. **Mandatory `// clone:` comments** — every `.clone()` / `.to_owned()` in non-test production code must have an adjacent English comment whose text starts with `// clone:` (or rustdoc that includes the same justification). State why borrow/move cannot work **or** what the clone buys (ownership boundary, shared buffer, measured perf). Example: `// clone: caller retains input; fragment sink needs owned Bytes`.
2. **Exemptions** — `tests/` and `*_tests.rs`; `Arc` / `Rc` refcount-only bumps may use `// clone: Arc share` / `// clone: Rc share`. `#[derive(Clone)]` needs no per-field comment.
3. **No casual clones** of packets, frames, bitstreams, or GPU-related wrappers on hot paths.
4. **Allocate with intent** — prefer `with_capacity`, buffer pools, or reuse; avoid repeated grow-by-append in inner loops.
5. **Copies are named** — byte or texture copies use clear names and caveats ([`caveats-and-clarity.md`](../spec/caveats-and-clarity.md)).
6. **APIs prefer borrowing** — take `&[u8]` / owned only at boundaries that need it; don’t force callers to clone to call you.
7. **FFI / C ABI** — document who allocates/frees; don’t double-copy across the edge without need ([`c-ffi.md`](../spec/c-ffi.md)).
8. **Measure when unsure** — hot-path retention of a clone/alloc needs a bench or an explicit PR trade-off. Policy is review/absolute-rule today (no git-hook parser yet).

### Hot-path micro-optimizations (when justified)

On measured hot paths (bitstream transforms, pixel/sample convert, interleave, checksums, …), prefer:

| Prefer | Notes |
|--------|--------|
| **Zero-Copy** / handle or shared-buffer pass-through | Default product story — GPU **or** CPU ([`vision.md`](../spec/vision.md), [wiki marks](../ai/wiki/zero-copy/marks.md), [`gpu-interop.md`](../spec/gpu-interop.md)) |
| **Non-alloc** inner loops | Reuse scratch; no per-packet/`frame` heap churn |
| **Vectorization-friendly** code | Contiguous data, predictable loops so the compiler (or explicit vector ops) can vectorize — do **not** mandate hand-written SIMD intrinsics as the default style |
| **Branch-light** logic where it wins | Prefer when profiling shows branch cost; clarity still matters — no cryptic “branchless for its own sake” |

Explicit SIMD/portable-simd crates or `unsafe` vector paths need the usual gates (`deps-policy`, `// SAFETY:`, honest rustdoc). Prefer readable code that **vectorizes well** over opaque intrinsic soup unless benches prove the win ([`benchmarking.md`](benchmarking.md)).

Tests and one-shot setup may be looser; still avoid teaching bad patterns in examples meant for production use.

## Public Rust API shape

Prefer idiomatic Rust over C/C++-flavored surfaces:

| Prefer | Avoid |
|--------|--------|
| `Type::open(&config)` / `Type::try_new(...)` | Free functions like `open_video(config)`, `auto::open(config)` |
| Explicit `width` / `height` / codec from the caller | Named resolution presets on product APIs (`h264_1080p`, “4K default”, …) |
| Methods on the session type | Global “open then use opaque handle” helpers as the primary API |

High-level / auto APIs **must not** bake marketing resolutions or quality ladders into constructor names. Size and codec come from the app (capture size, timeline, etc.). Low-level test helpers may take explicit `(width, height)` (e.g. `VideoEncoderConfig::h264(w, h, tb)`), not `*_1080p`.

C ABI (`*-ffi`) may expose `mediaway_*_open`-style functions; that does not justify the same shape in the Rust API.

## Continuous refactoring

Code should read as if designed in one pass for the current shape.

- Inside the blast radius: no hacks, shims, or dual paths — when the new path absorbs callers, delete the old one
- “It costs time” is not a reason to skip refactoring
- Does not conflict with surgical changes (don’t touch unrelated code; do clean what you break)

## Code as primary documentation

Callers and maintainers must be able to work from **source + rustdoc** without missing critical intent ([`docs/spec/caveats-and-clarity.md`](../spec/caveats-and-clarity.md), ADR-0006).

- Public items: rustdoc for purpose, ownership, errors, and **performance / compatibility notes** when relevant
- Names encode cost when the path copies, readbacks, or stalls (`copy_…`, `readback_…`, `compat_…`)
- Prefer explicit enums/types over opaque booleans
- Prefer borrow/move over clone on hot paths (see Allocation section above)
- `// SAFETY:` must state invariants completely enough to review the block

Markdown ADRs/specs remain mandatory for decisions; they **amplify** the code contract and must not be the only place a footgun is mentioned.

## Errors

Public library errors use **`thiserror`** (English messages, prefer `#[non_exhaustive]`). See [`error-handling.md`](error-handling.md) · ADR-0010. Do not hand-roll `Display` for new library error enums.

## Performance / compatibility caveats

Any escape hatch (cross-API GPU copy, CPU readback, SW fallback, …) requires rustdoc at the item and a catalog/ADR update when cross-cutting. No silent slow defaults. See ADR-0006.

## TODO

`TODO(#123)` / `FIXME(#123)` only. Bare TODOs are blocked at pre-commit.
