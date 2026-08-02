# Issues and discussions

GitHub **forms** help triage. Intake stays **open**: blank issues allowed; features welcome as issues *or* discussions.

## Where to file what

| Kind | Where | Template (optional but preferred) |
|------|--------|-----------------------------------|
| Bug | **Issue** | Report a bug |
| Crash / panic / hang | **Issue** | Report a crash or hang |
| Docs / wiki / rustdoc | **Issue** | Docs issue |
| Question / inquiry | **Issue** (preferred) | Blank issue or closest form |
| Feature / improvement / design | **Issue** *or* **Discussion** | Feature request (Issue preferred when you want tracking) |
| Long open-ended brainstorm | **Discussion** (optional) | Feature discussion |
| Security vulnerability | **Not** a public issue — [`security.md`](security.md) | Private report |

## Contact

Canonical home: **[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway)**.

General questions and support requests: **open a GitHub Issue** (English). See also [`CONTRIBUTING.md`](../../CONTRIBUTING.md) § Questions.

## Tune for Mediaway (what helps triage)

When relevant, mention:

- Crate / area (`muxer`, `encoder`, …)
- Platform (Windows → Web → Linux order matters for priority)
- Streaming vs whole-buffer; sync vs async host
- Zero-Copy vs copy — GPU handle **or** shared CPU buffer vs payload `memcpy` / readback
- License: no request for FFmpeg/GPL **Cargo** deps — system oracle CLI is OK for tests

## Language

Titles and bodies: **English only** ([`AGENTS.md`](../../AGENTS.md)).

## Labels (recommended)

| Label | Use |
|-------|-----|
| `bug` | Incorrect behavior |
| `crash` | Panic, abort, hang, deadlock |
| `docs` | Documentation |
| `enhancement` | Feature / improvement |
| `design` | API / ADR / packaging discussion |
| `perf` | Performance / vectorization / alloc |
| `state:needs triage` | New inbound |
| `good first issue` | Small, well-scoped |

## Titles

Short and specific (area + symptom). Avoid empty “help” titles.

## Linking from code

`TODO(#123)` / `FIXME(#123)` only ([hooks](hooks.md)).

## Related

- PR template: [`.github/PULL_REQUEST_TEMPLATE.md`](../../.github/PULL_REQUEST_TEMPLATE.md)
- [`../contributing/pull-requests.md`](../contributing/pull-requests.md)
