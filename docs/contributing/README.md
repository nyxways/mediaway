# Contributor documentation

Start here: root [`CONTRIBUTING.md`](../../CONTRIBUTING.md).

| Audience | Start |
|----------|--------|
| **Humans** | [`CONTRIBUTING.md`](../../CONTRIBUTING.md) · guides below |
| **Contributor AI assistants** | [`for-agents.md`](for-agents.md) → root [`AGENTS.md`](../../AGENTS.md) |

Process detail stays in `docs/conventions/`; product design stays in `docs/spec/`.

## Guides

| Doc | Topic |
|-----|--------|
| [for-agents.md](for-agents.md) | **AI assistants:** mandatory reading order + constraints |
| [getting-started.md](getting-started.md) | Clone, toolchain, hooks, first test |
| [documentation.md](documentation.md) | What to write where (humans vs agents) |
| [pull-requests.md](pull-requests.md) | Branch, commits, **full PR checklist**, merge |
| Issues / discussions | [`../conventions/issues.md`](../conventions/issues.md) |

## Process (conventions)

| Doc | Topic |
|-----|--------|
| [commits.md](../conventions/commits.md) | Conventional Commits + English |
| [branches.md](../conventions/branches.md) | Branch naming |
| [hooks.md](../conventions/hooks.md) | lefthook gates |
| [code-style.md](../conventions/code-style.md) | Rust / `unsafe` |
| [testing.md](../conventions/testing.md) | Fixtures + FFmpeg oracle |
| [deps-policy.md](../conventions/deps-policy.md) | Dependencies |
| [security.md](../conventions/security.md) | Secrets + licenses |
| [docs-layout.md](../conventions/docs-layout.md) | Doc ownership |

## Design (spec)

| Doc | Topic |
|-----|--------|
| [status.md](../spec/status.md) | Maturity — not for production |
| [maturity-bar.md](../spec/maturity-bar.md) | Corpora, benches, oracles, scoped trust |
| [benchmarking.md](../conventions/benchmarking.md) | How to bench honestly |
| Machine profiles | [`docs/benchmarks/machines.md`](../benchmarks/machines.md) |
| [vision.md](../spec/vision.md) | Product pillars |
| [crate-packaging.md](../spec/crate-packaging.md) | Facade / platform / sans-io crates |
| [sans-io.md](../spec/sans-io.md) | Sans-IO policy |
| [api-layers.md](../spec/api-layers.md) | Low-level APIs first-class |
| [c-ffi.md](../spec/c-ffi.md) | Per-capability `*-ffi` + features; Node vs browser JS/TS |
| [gpu-interop.md](../spec/gpu-interop.md) | wgpu / WebGPU / Dawn GPU Zero-Copy adapters |
| [wiki/zero-copy/marks.md](../ai/wiki/zero-copy/marks.md) | README **⚡** = GPU **or** shared CPU |
| [caveats-and-clarity.md](../spec/caveats-and-clarity.md) | Costly paths + code as primary docs |
| [async-and-streaming.md](../spec/async-and-streaming.md) | Streaming-first + async policy |
| [overview.md](../spec/overview.md) | Pipeline + MVP order |
| [issues.md](../conventions/issues.md) | Issue kinds vs feature discussions |
