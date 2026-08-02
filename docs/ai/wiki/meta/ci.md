# GitHub Actions CI

Canonical: [`docs/conventions/hooks.md`](../../../conventions/hooks.md) § CI · workflow [`.github/workflows/ci.yml`](../../../../.github/workflows/ci.yml).

- `rust` job: Windows + Ubuntu — fmt, clippy, test, source ≤1000 lines
- `deny` job: Ubuntu — `cargo deny`
- No GPU / system FFmpeg in default CI
