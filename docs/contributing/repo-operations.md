# Repository operations (public)

How this public repository is run: GitHub settings, issue labels, CI/badges,
package publishing, and releases. Nothing here is enforced by git hooks — the
hooks cover commit shape and code gates only.

## GitHub settings (in place)

- **Branch protection on `main`** (Settings → Branches):
  - Require PRs before merging; require 1 approval (or a CODEOWNERS rule when
    added).
  - Required status checks: `rust (windows-latest)`, `rust (ubuntu-latest)`,
    `deny`, `docs` build.
  - Require conversation resolution; block force-push.
- **Discussions**: enabled (feature brainstorm channel).
- **Issues**: enabled; blank issues allowed (intake stays open per
  [`docs/conventions/issues.md`](../conventions/issues.md)).
- **Security**: secret scanning + push protection on; dependabot alerts for
  `cargo` (advisory-db) and npm.

## Community health files (committed)

- [`SECURITY.md`](../../SECURITY.md)
- `CONTRIBUTING.md` (human entrypoint; agents use `AGENTS.md`)
- Issue forms + PR template under `.github/`
- License files `LICENSE-MIT` / `LICENSE-APACHE`

## Labels

The label set from [`docs/conventions/issues.md`](../conventions/issues.md) is
applied with `bun tools/scripts/sync-labels.ts` (idempotent; run it with `gh`
authenticated whenever the list changes). New issues start as
`state:needs triage`; binding issues carry `area:bindings` + `binding:<lang>`.

## CI / badges

- CI badge in `README.md` points at `nyxways/mediaway`; the `docs` badge is the
  published mdBook (gh-pages).
- Pre-push gates (also on CI): `clippy --all-targets --all-features
  -D warnings`, `cargo nextest run --workspace`, `cargo deny check` — the
  graph stays GPL-free.

## Publishing

Packages are built from `main` by the Bun scripts under `tools/scripts/`, then
uploaded with the registry's CLI:

| Registry | Package(s) | Build | Upload |
|---|---|---|---|
| npm | `@mediaway/ffi`, `@mediaway/container`, `@mediaway/device`, `@mediaway/encoder`, `@mediaway/browser` | `bun tools/scripts/build-node-packages.ts`; browser: `npm run build` in `bindings/browser/packages/browser` | `npm publish` (CI secret `NPM_TOKEN`, `@mediaway` scope) |
| NuGet | `Mediaway.*` (8 packages) | `bun tools/scripts/package-csharp.ts` | `dotnet nuget push` (secret `NUGET_KEY`) |
| PyPI | `mediaway` | `bun tools/scripts/build-python-package.ts` | `twine upload` (secret `PYPI_TOKEN`) |
| crates.io | `iso-bmff-wasm` (+ future `publish.workspace` crates) | `cargo publish` | cargo token (secret `CARGO_REGISTRY_TOKEN`) |
| C/C++ | `Mediaway-<version>-win64.zip/.tgz` | `cmake --build build && cpack` in `bindings/cpp` | GitHub release assets |

Versioning: workspace `0.1.0` today; tag `v<version>` per release.

## Releases

1. Bump the workspace version (workspace `Cargo.toml` `version`), update
   [`docs/spec/status.md`](../spec/status.md) if support promises change.
2. Tag `v<version>` on `main`; write release notes covering platforms
   (Windows-first), codecs (H.264/AAC/…), bindings
   (C/C++/C#/Python/Node/Browser), and the honest maturity bar.
3. Run the publishing table above; attach the C/C++ CPack archives to the
   release.
4. Wiki: note the new package versions under `docs/ai/wiki/bindings/`.
