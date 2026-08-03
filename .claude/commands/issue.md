---
description: Create a GitHub issue (or point to Discussion) using Mediaway templates.
argument-hint: <title>
---

Create a GitHub issue — "$1".

1. Prefer a form when it fits (blank issues are also OK):
   - Bug → `10_bug_report.yml`
   - Crash/hang → `11_crash_report.yml`
   - Docs → `20_docs.yml`
   - Feature → `30_feature.yml` **or** Discussion `feature-requests`
2. `gh issue create` with English title/body, or open the web form.
3. Labels: `state:needs triage` plus `bug` / `crash` / `docs` / `enhancement` as
   appropriate. Binding issues additionally get `area:bindings` + `binding:<lang>`
   (rust|c|cpp|csharp|python|node|browser); platform-scoped issues get
   `platform:<os>` (see [`docs/conventions/issues.md`](../../docs/conventions/issues.md) § Labels).
4. Link related ADR/spec paths.
5. Return the issue number for `TODO(#N)`.

Policy: [`docs/conventions/issues.md`](../../docs/conventions/issues.md).

**Title and body: English only.** Confirm direction with the user in their language first if the title is ambiguous.
