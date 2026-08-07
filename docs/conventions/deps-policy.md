# Dependency policy

Adding a Rust crate is a **design decision**, not a convenience. Prefer **not** adding a dependency unless the need is clear and alternatives were considered.

| Action | Rule |
|--------|------|
| Workspace dep version | Pin through minor (`tokio = "1.40"`) |
| Wildcard `*` | Forbidden (`deny` wildcards) |
| New dependency | Deliberate review (below); latest **stable**; license first |
| GPL/LGPL/FFmpeg **crates** / linking `libav*` | Reject immediately (`deny.toml` bans) |
| System `ffmpeg` / `ffprobe` on PATH | **Allowed for tests/dev only** — not a Cargo dep; see [testing.md](testing.md) · [ADR-0002](../adr/0002-system-oracle.md) |
| Security advisory | Blocked on pre-push |

## Review checklist (before adding)

Work through these. Record the answer in the PR (and an ADR when the dep is heavy, codec-related, FFI, or pulls a large graph).

### Need

1. Can **`std`**, an **existing workspace dep**, or **~20–50 lines** of local code cover it?
2. Is this a **real requirement** for the current stage, or speculative / “nice to have”?
3. Should it be **optional** (`optional = true` + feature) so slim builds stay slim?

### License & policy

4. License on crates.io / repo — fits [`security.md`](security.md) / `deny.toml` allow-list?
5. Any **transitive** GPL/LGPL/FFmpeg-family or copyleft surprise? (`cargo tree -i`, `cargo deny`)
6. Dual-license / “or later” / unclear LICENSE — resolve before merge.

### Quality & maintenance

7. **Maintenance:** recent releases, responsive issues, not abandoned for years.
8. **API stability:** MSRV / edition fit (`rust-version` 1.91, edition 2024); semver track record.
9. **Popularity ≠ sufficient** — still check code quality and fit; avoid trendy crates with poor ownership stories.
10. Prefer **well-scoped** crates over kitchen-sink frameworks.

### Cost

11. **Compile time / binary size** — is the graph heavy for the benefit?
12. **Feature flags** on the dep — can we disable unused features (`default-features = false`)?
13. **Platform / `no_std` / WASM** impact if relevant to this crate’s targets.
14. **Unsafe / FFI surface** introduced by the dep — acceptable? Document if we rely on it.

### Alternatives

15. Compare **≥1 alternative** (another crate, or in-house) with a short pros/cons note in the PR.
16. For codecs / media / crypto: prefer known-permissive, widely reviewed options; ADR required.

## Adding procedure

1. Complete the review checklist above.
2. Add to `[workspace.dependencies]` with a **minor-pinned** version; depend via workspace in member crates.
3. Prefer `default-features = false` + explicit features when the crate is large.
4. Run `cargo deny check advisories licenses bans sources` and a relevant `cargo tree`.
5. ADR when: platform FFI, codecs, crypto, large transitive graph, or policy-sensitive choice.
6. Commit: `build(deps): add <name> X.Y for <reason>` (**English**); PR body summarizes the checklist.

## Do not

- Add a dep “to try later” on `main`
- Vendoring GPL / FFmpeg as a path dep or submodule to bypass `deny`
- Pin `*` or ultra-loose ranges
- Depend on git sources unless workspace `deny.toml` / sources policy explicitly allows (default: crates.io only)

## Related

- [security.md](security.md) — license identity  
- [perf-crates.md](perf-crates.md) — high-perf crate allow / defer list  
- [testing.md](testing.md) — FFmpeg oracle is not a Cargo dep  
- PR checklist: [`docs/contributing/pull-requests.md`](../contributing/pull-requests.md)
