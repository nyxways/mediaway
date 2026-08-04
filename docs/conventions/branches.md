# Branching

Trunk-based development. Single `main`, short-lived feature branches.

| Branch | Purpose | Lifetime |
|--------|---------|----------|
| `main` | Always green | Permanent |
| `feat/<scope>-<desc>` | Feature | 1–3 days |
| `fix/<scope>-<desc>` | Bugfix | 1–3 days |
| `refactor/<scope>-<desc>` | Refactor | 1–3 days |
| `docs/<desc>` | Docs | <1 day |
| `chore/<desc>` | Chores | <1 day |

- Lowercase, `-` separators, prefer ≤30 chars
- No force-push to `main`
- Non-trivial changes via PR + squash merge
- Branches living >1 week should be split

Branch names and PR titles/descriptions: **English**.

## Starting new work

Before adding commits to the branch you happen to be on, check whether it still fits:

- **Unrelated topic** (e.g. you're asked for a new feature while sitting on a docs-only
  branch from earlier) → branch fresh off `main`, don't stack the new work on top.
- **Already landed upstream** (its own PR merged into `main` while you kept working locally)
  → the branch is stale; branch fresh off `main` (fetch first — a stale local `main` is not
  a safe base either) rather than reusing it.
- **Genuine continuation** of the current branch's stated purpose → keep committing there.

Rule of thumb: one task's changes = one branch = one PR. If unsure, ask rather than guessing
which branch a PR should target.
