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
