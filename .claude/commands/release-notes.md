---
description: Finalize RELEASE_NOTES.md from the Unreleased section into a versioned release note
argument-hint: <version | reset>
allowed-tools: Read, Write, Edit, Bash(git:*)
---

Finalize Mediaway release notes: turn the `## Unreleased` section of
`RELEASE_NOTES.md` into the versioned release note for `$1`, archive it to
`CHANGELOG.md`, and keep the files consistent for the release pipeline.

## Modes

- `$1` = semver like `0.2.0` → finalize that version.
- `$1` = `reset` → restore the Unreleased template (step 6), nothing else.

## Steps

1. **Version check.** Read `[workspace.package] version` from the root
   `Cargo.toml`. If it differs from `$1`, tell the user and ask which is
   authoritative before touching anything (the release workflow refuses a
   mismatch; bumping the workspace version is runbook step 1).
2. **Collect.** Read `RELEASE_NOTES.md` and group the `## Unreleased` bullets
   by their subsection (`Added` / `Changed` / `Fixed` / `Removed` /
   `Deprecated` / `Breaking`). Run `git log` since the last version recorded
   in `CHANGELOG.md` and fold any `BREAKING CHANGE:` footers or `!`-marked
   conventional commits into the `Breaking` group. Drop empty groups.
3. **Compose.** Write the new `RELEASE_NOTES.md` — this is what the GitHub
   release workflow consumes as `--notes-file`:
   - `# Mediaway v$1`
   - `## What's new` — the grouped bullets, one line each, English, no
     trailing period, framed for users.
   - `## Overview`, `## Platforms`, `## Codecs`, `## Bindings`,
     `## Breaking changes`, `## Maturity bar` — carry forward from the
     previous version's section in `CHANGELOG.md`, updating each wherever the
     Unreleased bullets imply a change (new platform/codec/binding support,
     renamed or removed API, maturity shift per `docs/spec/status.md`). Keep
     the maturity bar honest — never claim production readiness.
4. **Archive.** Prepend `## [$1] - YYYY-MM-DD` (today, ISO) to
   `CHANGELOG.md` with the same content as the new `RELEASE_NOTES.md` minus
   the `# Mediaway v$1` title (Keep a Changelog style, English).
5. **Report.** In chat (user's language) summarize the final notes; remind the
   user of the runbook: workspace version must match (step 1), create the
   `release/v$1` branch, push, then after the GitHub release ships run
   `/release-notes reset` on `main` so development changes accumulate again.
6. **Reset mode.** Overwrite `RELEASE_NOTES.md` with the Unreleased template:
   `# Mediaway release notes` title, a short HTML comment pointing at this
   command and the agent rule, and empty `### Added/Changed/Fixed/Removed/
   Deprecated/Breaking` subsections under `## Unreleased`. Do not touch
   `CHANGELOG.md`.
