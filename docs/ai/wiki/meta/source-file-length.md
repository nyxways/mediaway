# Source file length

Canonical: [`docs/conventions/code-style.md`](../../../conventions/code-style.md) § File size · hook `tools/hooks/forbid-long-source.sh`.

- Staged source (`.rs`, …) **≤1000 lines** or pre-commit fails
- Split modules; do not grow mega-files
- Exempt: `local/` scratch and `vendor/` third-party headers (e.g. `crates/vpl-sys/vendor/` — oneVPL `.h` files exceed 1000 lines by design). Keep the `ci.yml` length check in sync with the hook.
