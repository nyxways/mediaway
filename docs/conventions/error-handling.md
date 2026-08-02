# Error handling

Workspace policy: [ADR-0010](../adr/0010-thiserror-errors.md).

## Rules

| Surface | Rule |
|---------|------|
| Library / sans-io / facade crates | Public errors = **`thiserror`** enums |
| `anyhow` / `eyre` / `Box<dyn Error + Send + …>` | **Not** the public library error type |
| Messages | **English** (language policy) |
| Binaries (`tools/*`) | Prefer typed errors; map to exit / stderr; no `unwrap` on expected failures |
| C-FFI | Map Rust variants → codes later; do not design C strings as the only API |

## Shape

```rust
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid track id or duplicate registration")]
    InvalidTrack,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("hash mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: std::path::PathBuf,
        expected: String,
        actual: String,
    },
}
```

### Checklist

1. **`#[derive(Debug, Error)]`** — implement `Display` via `#[error("…")]`, not hand-rolled `fmt` (unless a rare custom case).
2. **`#[non_exhaustive]`** on **public** library error enums that may gain variants (early crates included).
3. **Specific variants** — prefer `InvalidTrack` over `Other(String)` unless truly open-ended.
4. **`#[from]`** — only when every source error maps cleanly to that variant; otherwise map explicitly at the boundary.
5. **Fields for context** — paths, ids, expected/actual; keep hot-path errors cheap (no format! in `Ok` paths).
6. **Naming** — crate-primary type often `Error`; helper crates may use `TestMediaError`-style names. Shims: `pub use …::Error as MuxError`.
7. **Docs** — rustdoc on the enum and non-obvious variants; English.
8. **No panic for expected failure** — return `Result` (`unwrap_used` deny).

## Anti-patterns

- `Result<T, String>` / `Result<T, Box<dyn std::error::Error>>` in public APIs
- Swallowing errors (`let _ = …`) on fallible media paths
- Non-English error text
- Duplicating the same domain error in muxer + container without a single owner (own in the core crate; shim re-exports)

## Deps

- Pin `thiserror` via `[workspace.dependencies]` (already present).
- Do **not** add `anyhow` to library crates. If a binary ever needs it, keep it `tools/*`-only and justify in the PR (deps-policy).

## Related

- Code clarity: [`../spec/caveats-and-clarity.md`](../spec/caveats-and-clarity.md)
- C-FFI: [`../spec/c-ffi.md`](../spec/c-ffi.md)
- Language: [`language.md`](../ai/wiki/meta/language.md) (via AGENTS language policy)
