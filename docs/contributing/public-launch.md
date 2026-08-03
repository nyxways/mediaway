# Going public — launch checklist

Everything below is **not** enforced by git hooks — it is the human checklist
for flipping this repository public. Work through it in order, on `main`, then
flip the repo visibility.

## 1. Repo settings (GitHub)

- [ ] **Visibility**: flip to public (Settings → General → Danger Zone).
- [ ] **Branch protection on `main`** (Settings → Branches):
  - Require PRs before merging; require 1 approval (or a CODEOWNERS rule when
    added).
  - Require status checks: `rust (windows-latest)`, `rust (ubuntu-latest)`,
    `deny`, `docs` build.
  - Require conversation resolution; block force-push.
- [ ] **Discussions**: enabled (feature brainstorm channel).
- [ ] **Issues**: enabled; blank issues allowed (intake stays open per
    [`docs/conventions/issues.md`](../conventions/issues.md)).
- [ ] **Security**: enable secret scanning + push protection; enable
    dependabot alerts for `cargo` (advisory-db) and npm once packages publish.
- [ ] **Default branch**: `main` (already the case).

## 2. Community health files (committed)

- [ ] [`SECURITY.md`](../../SECURITY.md) — present.
- [ ] `CONTRIBUTING.md` — present (human entrypoint; agents use `AGENTS.md`).
- [ ] Issue forms + PR template under `.github/` — present.
- [ ] License files `LICENSE-MIT` / `LICENSE-APACHE` — present.
- [ ] (Optional, when ready) `CODE_OF_CONDUCT.md`.

## 3. Labels

- [ ] `bun tools/scripts/sync-labels.ts` with `gh` authenticated — applies the
      label set from [`docs/conventions/issues.md`](../conventions/issues.md).

## 4. CI / badges

- [ ] CI badge in `README.md` points at `nyxways/mediaway` (already does);
      verify it renders green after the flip.
- [ ] `docs` badge (mdBook) — publish the book on `gh-pages` if not already.
- [ ] `cargo deny` passes on `main` (pre-push gate) — the graph stays
      GPL-free before anyone else can fork it.

## 5. Package-manager presence

One-time owner setup on each registry, then point the packages at it:

| Registry | Package(s) | Owner setup |
|---|---|---|
| npm | `@mediaway/ffi`, `@mediaway/container`, `@mediaway/device`, `@mediaway/encoder`, `@mediaway/browser` | Create the `@mediaway` scope; add a publish token (CI secret `NPM_TOKEN`) |
| NuGet | `Mediaway.*` (8 packages) | Create the `Mediaway` owner/namespace; API key secret `NUGET_KEY` |
| PyPI | `mediaway` | Register the `mediaway` project name (once); token secret `PYPI_TOKEN` |
| crates.io | `iso-bmff-wasm` (+ future) | Confirm `publish.workspace` crates; cargo token secret `CARGO_REGISTRY_TOKEN` |

Builds before upload:
`bun tools/scripts/build-node-packages.ts` / `build-python-package.ts` /
`package-csharp.ts`, `npm run build` in `bindings/browser/packages/browser`,
`cpack` in `bindings/cpp`.

## 6. First public release

- [ ] Version the workspace (`0.1.0` today) and tag `v0.1.0` with release
      notes covering: platforms (Windows-first), codecs (H.264/AAC/…),
      bindings (C/C++/C#/Python/Node/Browser), and the honest maturity bar
      ([`docs/spec/status.md`](../spec/status.md)).
- [ ] Update `docs/spec/status.md` if the flip changes anything about support
      promises.
- [ ] Wiki: add a `bindings/` note that packages are live.
