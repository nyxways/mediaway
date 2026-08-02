# Documentation guide (contributors)

## Audience split

| Surface | Audience | Content |
|---------|----------|---------|
| `README.md`, `CONTRIBUTING.md`, `docs/contributing/` (except `for-agents.md`) | Humans | Setup, status, how to contribute |
| `docs/contributing/for-agents.md` | **Contributor AIs** | Onboarding brief → then `AGENTS.md` |
| `docs/spec/`, `docs/conventions/`, crate `docs/` / `adr/` | Humans + engineering agents | Design and process |
| `AGENTS.md`, `docs/ai/`, `docs/ai/wiki/` | All coding agents (maintainers + contributors) | Rules SSOT and session wiki |

Do **not** put agent “read first / Rule 0” runbooks in root README or `CONTRIBUTING.md`. Full layout: [`docs/conventions/docs-layout.md`](../conventions/docs-layout.md).

## Language

All documentation in the repo is **English**.

## What to update when

| Change | Update |
|--------|--------|
| Public API / backend choice | Crate `adr/` (+ `docs/spec/` if cross-cutting) |
| Costly compat / copy / readback path | Item rustdoc + [`caveats-and-clarity.md`](../spec/caveats-and-clarity.md) catalog (ADR-0006) |
| Stage progress | That crate’s `docs/roadmap.md` + workspace [`docs/roadmap.md`](../roadmap.md) index if members change |
| New crate | Packaging rules ([`crate-packaging.md`](../spec/crate-packaging.md)); register in workspace; add `docs/` + `adr/` |
| Process (hooks, commits) | `docs/conventions/` |
| Maturity / production readiness | [`docs/spec/status.md`](../spec/status.md) + README banner |

## Style

- Prefer short pages and links over duplication.
- Specs state decisions; roadmaps track checkboxes.
- ADRs: problem → decision → alternatives → consequences (use the local `adr/template.md`).
- Public Rust items: rustdoc must stand alone for the contract; markdown does not replace it (ADR-0006).
