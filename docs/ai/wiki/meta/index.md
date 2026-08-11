# Meta

| Doc | Summary |
|-----|---------|
| [vision](vision.md) | License-safe · perf-first · high→low abstraction |
| [status](status.md) | Early development · not for production |
| [maturity-bar](maturity-bar.md) | What to earn: correctness · stability · perf |
| [benchmarking](benchmarking.md) | Bench labels · baselines · honesty |
| [local-workspace](local-workspace.md) | Gitignored `local/` scratch |
| [external-standards](external-standards.md) | Standards by URL · fetch under `local/standards/` |
| [caveats-clarity](caveats-clarity.md) | Costly-path docs · code carries the contract |
| [alloc-discipline](alloc-discipline.md) | Careful clone / alloc / copy on hot paths |
| [zca](zca.md) | Zero-cost abstractions · plan before code · minimize `Box` · SmallVec |
| [errors](errors.md) | Library errors via `thiserror` · English · no anyhow in libs |
| [perf-crates](perf-crates.md) | `memchr` / `smallvec` / deferred rayon·bytemuck |
| [hot-path-opts](hot-path-opts.md) | Vectorization · non-alloc · ⚡ GPU or shared CPU |
| [source-file-length](source-file-length.md) | Source ≤1000 lines (pre-commit) |
| [ci](ci.md) | GitHub Actions (fmt / clippy / test / deny) |
| [release](release.md) | Release workflow — release branch → publish + GitHub release · secrets TUI |
| [release-notes](release-notes.md) | Unreleased → versioned notes · agent rule § 10 + `/release-notes` |
| [scripts-bun](scripts-bun.md) | Light utils = Bun + TypeScript |
| [issues](issues.md) | Bug/crash/docs issues · features as discussions |
| [contributing](contributing.md) | Human CONTRIBUTING + docs/contributing map |
| [pull-requests](pull-requests.md) | PR author checklist · doc sync with code |
| [deps](deps.md) | Careful Cargo dependency adds |
| [agent-docs](agent-docs.md) | AGENTS.md SSOT · wiki · Claude/Cursor entrypoints |
| [language](language.md) | English artifacts · user-language chat |
| [test-media](test-media.md) | Rust-generated fixtures · local cache · no git binaries |
| [testing](testing.md) | Tiers · sibling `*_tests.rs` · nextest · oracle |
| [crypto](crypto.md) | ClearKey CENC · `iso-cenc` · no DRM CDM |
| [system-oracle](system-oracle.md) | PATH oracle (typically ffmpeg/ffprobe) · no Cargo link |
| [crate-map](crate-map.md) | Workspace members + facade / platform / sans-io |
| [crate-packaging](crate-packaging.md) | Naming: device + device-windows + device-web |
| [docs-layout](docs-layout.md) | Crate-local vs workspace documentation |
| [language-bindings](language-bindings.md) | `bindings/` aspirational examples per planned Tier B/C language |
| [nodejs-gpu-device](nodejs-gpu-device.md) | Node.js GPU device factory + real Screen capture + capture-encode bridge |
| [csharp-gpu-device](csharp-gpu-device.md) | C# GPU device factory + real Screen capture + capture-encode bridge |
| [python-gpu-device](python-gpu-device.md) | Python GPU device factory + real Screen capture + capture-encode bridge |
| [docs-book](docs-book.md) | `docs/book/` mdBook site · README anchor includes · GitHub Pages CI |
| [examples-layout](examples-layout.md) | `examples/` sectors (container/encode/decode/device/pipeline) · `harness = false` cfg-gate gotcha |
