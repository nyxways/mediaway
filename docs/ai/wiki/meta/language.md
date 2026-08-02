# Language policy

| Surface | Language |
|---------|----------|
| Agent ↔ user chat | **User's language** (e.g. Korean, Japanese, Spanish, etc.) |
| Repo artifacts | **English only** |
| Commits / PRs / issues | **English only** (policy — not hook-enforced) |

Artifacts include: code comments, wiki, specs, ADRs, conventions, README, commit/PR/issue text, committed agent prompts under `.claude/` / `.agents/`.

`commit-msg` hook validates Conventional Commits **format** only. English is upheld by agents and reviewers (`AGENTS.md`, `mediaway-reviewer`).

Canonical: [`AGENTS.md`](../../../../AGENTS.md) § Language policy · [`commits.md`](../../../conventions/commits.md).
