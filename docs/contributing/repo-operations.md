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

## Publishing (automatic)

A push to a **release branch** (`release` or `release/*`) publishes everything
via [`.github/workflows/release.yml`](../../.github/workflows/release.yml):

1. `version` — reads the version from `[workspace.package]` in the root
   `Cargo.toml` (**single source of truth**; npm / NuGet / PyPI / CPack
   versions are stamped from it at publish time) and refuses to re-release an
   existing `v<version>`; a `release/vX.Y.Z` branch must match the workspace
   version.
2. `crates` — publishes the crates.io set in dependency order with per-crate
   `cargo package` verification via `tools/scripts/publish-crates.ts` (skips
   versions already on the index). This is the pipeline gate: every later job
   waits for it, so a registry failure aborts before any binding artifacts
   are built.
3. `native-assets` — builds the win64 `mediaway-ffi` cdylib
   (`x86_64-pc-windows-gnu`, MinGW-w64) and stages it for the binding jobs.
4. `bindings-tests` — RC gate: C#/Python/Node/C/Browser round-trips against
   the staged DLL.
5. `npm` / `nuget` / `pypi` / `native` — publish `@mediaway/*` (5 packages),
   `Mediaway.*` (8 NuGet packages), the `mediaway` wheel, and the C/C++ CPack
   archives (`Mediaway-<version>-win64.zip/.tgz`).
6. `release` — creates the `v<version>` tag and the GitHub release
   (prefers `RELEASE_NOTES.md`, falls back to generated notes) and attaches
   the CPack archives — only after every registry job succeeded.

**crates.io** publishes the 19-crate set in dependency order (9 freestanding
cores + the `mediaway-*` family + avcli/avprobe + vpl-sys; rtmp and
mediaway-ffi stay `publish = false`) via `tools/scripts/publish-crates.ts`
(runs `--dry-run` in the pre-flight, `CARGO_REGISTRY_TOKEN`
(https://crates.io/settings/tokens) for the real publish).

All registry jobs run in parallel. Registries are **one-shot per version**: a
failed publish leaves a partial release, so fix and **bump the workspace
version** for the next attempt (or delete the half-published packages
manually). Never re-run a release without bumping.

- **Environment**: every publishing job (`npm` / `nuget` / `pypi` /
  `native` / `release`) runs under the GitHub **`release` environment**
  (Settings → Environments → release). Protection rules there (e.g. required
  reviewers) gate **all** registry publishes and the GitHub release; with no
  rules the jobs run immediately. Registry-side environment restrictions —
  the NuGet Trusted Publishing policy's Environment field and the npm trusted
  publisher — are set to `release` to match the workflow.

- **Secrets** (repo-level Actions secrets) — set interactively with
  `bun tools/scripts/release-secrets.ts` (prints where to obtain each token,
  lets you change only the ones you pick; `--list` / `--set` / `--delete` for
  scripting):

  | Secret | Registry | Get it at |
  |---|---|---|
  | `NUGET_USER` | nuget.org | your nuget.org account name (profile, not email) — used by NuGet/login@v1 |

  npm and NuGet need **no long-lived tokens**: the `@mediaway` org
  authenticates npm publishes with OIDC **Trusted Publishing** (npmjs.com →
  org Settings → Trusted Publishing → add this repo as a publisher), so
  `npm publish --provenance` runs tokenless via its `id-token`. nuget.org uses
  a **Trusted Publishing policy** (nuget.org → Trusted Publishing → repo
  `nyxways/mediaway`, workflow file `release.yml`, environment left empty);
  `NuGet/login@v1` exchanges the job's `id-token` for a 1-hour API key — only
  the nuget.org username (`NUGET_USER`) is stored. `GITHUB_TOKEN` is also
  automatic (creates the tag + release) — nothing to set.

- **Dry-run**: `workflow_dispatch` with `dry_run=true` (pick the release
  branch as the ref) runs the whole pipeline — version checks, crates
  pre-flight, `npm publish --dry-run`, `dotnet pack`, wheel build, CPack,
  and the `bindings-tests` RC gate (C#/Python/Node/C/Browser round-trips
  against the staged DLL) — and publishes nothing.

- **Manual fallback** (pre-1.0, when the workflow is not used): build with the
  Bun scripts under `tools/scripts/`, then upload with the registry's CLI:

  | Registry | Package(s) | Build | Upload |
  |---|---|---|---|
  | npm | `@mediaway/ffi`, `@mediaway/container`, `@mediaway/device`, `@mediaway/encoder`, `@mediaway/browser` | `bun tools/scripts/build-node-packages.ts`; browser: `npm run build` in `bindings/browser/packages/browser` | `npm publish --provenance` (OIDC Trusted Publishing, `@mediaway` scope) |
  | NuGet | `Mediaway.*` (8 packages) | `bun tools/scripts/package-csharp.ts` | `dotnet nuget push` with a 1-hour key from `NuGet/login@v1` (OIDC Trusted Publishing; needs `NUGET_USER`) |
  | PyPI | `mediaway` | `bun tools/scripts/build-python-package.ts` | `pypa/gh-action-pypi-publish` (OIDC Trusted Publishing + PEP 740 attestations) |
  | crates.io | 19 crates (dependency order, see note above) | `bun tools/scripts/publish-crates.ts` | cargo token (secret `CARGO_REGISTRY_TOKEN`) |
  | C/C++ | `Mediaway-<version>-win64.zip/.tgz` | `cmake --build build && cpack` in `bindings/cpp` | GitHub release assets |

## Releases

1. Bump the workspace version (root `Cargo.toml` → `[workspace.package]` →
   `version`) and update [`docs/spec/status.md`](../spec/status.md) if support
   promises change. Do **not** hand-bump the npm / NuGet / PyPI / CPack
   manifests — the workflow stamps them from the workspace version.
2. Create a **release branch** `release/vX.Y.Z` (version must match the
   workspace version — the workflow refuses a mismatch), finalize
   [`RELEASE_NOTES.md`](../../RELEASE_NOTES.md) from its `## Unreleased`
   section (development changes accumulate there via `AGENTS.md` § 10;
   finalize with `/release-notes <version>` or by hand), and push. The
   workflow publishes everything and opens the GitHub release (prefers
   `RELEASE_NOTES.md`, falls back to generated notes). When in doubt, run
   `workflow_dispatch` with `dry_run=true` first.
3. Wiki: note the new package versions under `docs/ai/wiki/bindings/`.

Release notes should cover platforms (Windows-first), codecs (H.264/AAC/…),
bindings (C/C++/C#/Python/Node/Browser), and the honest maturity bar.

Version history accumulates in [`CHANGELOG.md`](../../CHANGELOG.md). After the
GitHub release ships, restore the Unreleased template on `main`
(`/release-notes reset`).
