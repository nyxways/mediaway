# Commit and PR convention

[Conventional Commits 1.0.0](https://www.conventionalcommits.org/).

## Language (absolute — policy, not a hook)

**Commits and pull requests are English only** (title, subject, body, review comments you author in-repo).

| Surface | Language |
|---------|----------|
| Agent ↔ user chat | User's language (e.g. Korean, Japanese, Spanish, etc.) |
| `git commit` message | **English** |
| PR title / description | **English** |
| Issue title / body | **English** |

Hooks check **Conventional Commits shape** only. English is an agent/human rule (`AGENTS.md` § Language policy) — do not add locale/ASCII linters to git hooks.

## Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

## type

| Type | Use |
|------|-----|
| `feat` | New feature |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `refactor` | Structure change with no behavior change |
| `perf` | Performance |
| `test` | Tests |
| `build` | Build system / dependencies |
| `ci` | CI |
| `chore` | Misc |
| `revert` | Revert |

## scope (Mediaway)

| Category | Examples |
|----------|----------|
| Core | `common`, `encoder`, `decoder`, `muxer`, `demuxer`, `device`, `sw` |
| Platform crate | `encoder-windows`, `device-web`, `decoder-linux`, … |
| Backend tech | `wmf`, `videotoolbox`, `mediacodec`, `webcodecs`, `vulkan`, `vaapi` |
| Bindings | `bindings` (cross-cutting), `binding-rust`, `binding-c`, `binding-cpp`, `binding-csharp`, `binding-python`, `binding-node`, `binding-browser` |
| Tools | `avcli`, `avprobe`, `tools` |
| Meta | `spec`, `adr`, `ci`, `deps`, `hooks`, `docs` |

### Binding scopes

Bindings are split **per language** so each binding's history is reviewable on
its own: `fix(binding-python): demuxer streams() crash on short moov` is a
Python-only change. Use the **umbrella `bindings`** scope when a change spans
languages or the shared packaging/scripts layer
(`feat(bindings): publish-ready packages for all languages`), and
`binding-rust` for the Rust API surface itself (examples, docs, API shape).
The `-ffi` crates stay under their core/backend scopes
(`pipeline-ffi`, `container-ffi`, `device-ffi`) — they are Rust, not bindings.

## subject

- Imperative mood ("add", "fix" — not "added")
- Prefer ≤50 chars; soft limit 72
- Lowercase first letter; no trailing period
- **English**

## body

- Explain *why* when needed; English
- Omit for trivial changes

## footer

- `BREAKING CHANGE: ...` or `feat(scope)!: ...`
- `Refs: #123`

## Pull requests

- Title: Conventional Commits style or short English summary
- Body: English — Summary + Test plan (checklist)
- Do not mention AI agent names in PR text

## Templates

- Commit: `.gitmessage` at the repo root
  (`git commit --template=.gitmessage`; the commit-msg hook enforces the shape).
- PR: [`.github/PULL_REQUEST_TEMPLATE.md`](../../.github/PULL_REQUEST_TEMPLATE.md).
- Issues: `.github/ISSUE_TEMPLATE/` forms.
- Labels: [`docs/conventions/issues.md`](issues.md) § Labels, applied by
  `bun tools/scripts/sync-labels.ts`.
- Repo operations (public): [`docs/contributing/repo-operations.md`](../contributing/repo-operations.md).
