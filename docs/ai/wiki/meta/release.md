# Release pipeline

Canonical: [`.github/workflows/release.yml`](../../../../.github/workflows/release.yml) ·
runbook: [`docs/contributing/repo-operations.md`](../../../contributing/repo-operations.md) § Publishing.

- **Trigger**: push to `release` / `release/*` (or `workflow_dispatch` —
  select the release branch as ref; `dry_run=true` validates without publishing).
- **Version SSOT**: `[workspace.package] version` in the root `Cargo.toml`; the
  workflow stamps npm (`package.json`), NuGet (`Directory.Build.props`), PyPI
  (`pyproject.toml`), and CPack (`CMakeLists.txt`) from it.
- **Jobs**: `version` gate (semver + refuses existing `v<version>`) → `crates`
  (metadata pre-flight: publishable-set closure; publish in dependency order
  via retry rounds) + `native-assets` (win64 GNU cdylibs, MinGW-w64) →
  `bindings-tests` (RC gate — C#/Python/Node/C/Browser round-trips run
  against the staged DLL; every publish job waits on it) → `npm` /
  `nuget` / `pypi` (wheel build on Windows) + `pypi-publish` (Linux: OIDC +
  PEP 740 attestations — `gh-action-pypi-publish`'s container cannot run on
  Windows runners) + `native` (CPack) in parallel → `release` (tag
  `v<version>` + RELEASE_NOTES.md + CPack assets). The `npm` job self-updates
  npm to >= 11.5.1 (required for OIDC trusted publishing).
- **Environment**: all publishing jobs run under the GitHub **`release`**
  environment (Settings → Environments → release); protection rules there
  gate every publish. NuGet policy / npm trusted-publisher Environment fields
  are set to `release` to match. Manual approval via the API:
  `POST /repos/{owner}/{repo}/actions/runs/{run}/pending_deployments` with
  `{"environment_ids":[<id>],"state":"approved"}`.
- **Secrets** (repo Actions secrets): `CARGO_REGISTRY_TOKEN` (fallback only;
  crates.io uses OIDC trusted publishing) · `NUGET_USER` — set with
  `bun tools/scripts/release-secrets.ts` (interactive TUI, prints each token's
  source URL; `--list` / `--set` / `--delete` for scripting). npm, NuGet, and
  PyPI need **no long-lived tokens**: npm via OIDC **Trusted Publishing**
  (`@mediaway` org Settings on npmjs.com → add this repo), NuGet via a
  **Trusted Publishing policy** on nuget.org (repo `nyxways/mediaway`, workflow
  file `release.yml`) + `NuGet/login@v1` exchanging the job's `id-token` for a
  1-hour API key — only the nuget.org username is stored. PyPI via **Trusted
  Publishing** (pypi.org → Pending publishers: repo `nyxways/mediaway`, workflow
  `release.yml`, environment `release`) + `pypa/gh-action-pypi-publish` with
  PEP 740 attestations. `GITHUB_TOKEN` is automatic.
- **Crates set**: publishable set is **20 crates** (10 freestanding cores with
  independent versions — `-core`-suffixed where the bare name is taken:
  adts-core, mpeg-ts-core, riff-wave-core, flv-core, ogg-core — + 8
  `mediaway-*` family crates sharing the workspace version + avcli/avprobe +
  vpl-sys + mediaway-test-media; rtmp and mediaway-ffi stay `publish = false`)
  — published in dependency order by `tools/scripts/publish-crates.ts`
  (skips versions already on the index; 429 auto-waits the 5-new-crates/10-min
  window; per-crate OIDC trusted publishing registered via
  `POST /api/v1/trusted_publishing/github_configs`).
- **Failure = partial release**: registries are one-shot per version; bump the
  workspace version and re-run (or delete half-published packages manually).
  The GitHub release is created only when every registry job succeeded.
- **Native DLLs**: the three `-ffi` cdylibs are built once by `native-assets`
  and downloaded as an artifact; `MEDIAWAY_SKIP_CARGO_BUILD=1` makes
  `tools/scripts/copy-native-dlls.ts` stage prebuilt DLLs without rebuilding.
  Gotcha: `upload-artifact@v4` strips the common ancestor of multi-path
  uploads, so the `native-dlls` artifact lacks the `bindings/` prefix and the
  download steps must use `path: bindings` to restore it.
