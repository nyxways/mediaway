# Security Policy

Mediaway is **pre-1.0 early development** ([`docs/spec/status.md`](docs/spec/status.md))
— not production-ready. Report security issues the same way regardless of maturity.

## Reporting a vulnerability

**Do not open a public issue.** Report privately:

- Open a private advisory: GitHub → **Security → Report a vulnerability**.
- Include: crate/area, affected version (or commit), minimal repro, impact.

Expected handling:

- Acknowledgement within 3 business days.
- A fix target agreed with the reporter; coordinated disclosure preferred.
- Public issues that turn out to be security-relevant are moved to a private
  channel before details are discussed.

## Scope

- Secrets in commits (`.env`, tokens, keys) — see
  [`docs/conventions/security.md`](docs/conventions/security.md)
- Unsafe code / FFI boundary bugs (`// SAFETY:` violations, handle
  consumption, buffer ownership at the C ABI)
- Dependency supply-chain issues (`cargo deny` / `audit`)
- Panic-across-FFI or crash-an-embedder classes
- Out of scope: GPL/FFmpeg licensing (that is a policy question, not a
  vulnerability — see [`docs/spec/vision.md`](docs/spec/vision.md) § License)

## Supported versions

Pre-1.0: only the latest `main` is supported. No backports until 1.0.
