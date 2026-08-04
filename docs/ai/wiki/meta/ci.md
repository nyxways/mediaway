# GitHub Actions CI

Canonical: [`docs/conventions/hooks.md`](../../../conventions/hooks.md) § CI · workflow [`.github/workflows/ci.yml`](../../../../.github/workflows/ci.yml).

- `source-length` job: first job — whole-workspace ≤1000-line check; no Rust
  toolchain, runs (and fails fast) on every change regardless of the affected set
- `affected` job: computes the dependency-tree reachability of the pushed/PR diff
  (`tools/scripts/ci-affected.ts`, bun + `cargo metadata` reverse graph) — outputs
  `NONE` / `ALL` / space-separated crate set. Every Rust-consuming job gates on it:
  `NONE` (docs/bindings-only) skips the rust family entirely
- `rust` job: Windows + Ubuntu — fmt, clippy, test scoped to the affected set
  (`ALL` → full workspace); whole job skipped on `NONE`
- `deny` job: Ubuntu — `cargo deny`; skipped on `NONE` (lockfile/manifests
  untouched ⇒ result identical to main)
- `wasm` job: Ubuntu — builds `iso-bmff-wasm` / `mediaway-encoder` / `mediaway-decoder`
  / `mediaway-device` for `wasm32-unknown-unknown`; runs on `ALL` or any wasm crate in
  the set
- `e2e-web` job: optional (continue-on-error); cascades from `wasm`
- No GPU / system FFmpeg in default CI

Bindings: only the browser binding has CI coverage today (wasm compile + `e2e-web`).
`bindings/` paths map to `NONE` in the affected set, and C# / Python / Node / C have no
CI test jobs — see `docs/ai/wiki/meta/language-bindings.md`.

## Lessons from the first pushes (2026-08-02/03)

- **wasm needs `--cfg=web_sys_unstable_apis`**: the WebCodecs/WebGPU web-sys bindings
  (`AudioEncoder`, `VideoEncoder`, `Gpu*`, …) are cfg-gated; without it every import in
  `mediaway-encoder-web/src/wasm.rs` fails with E0432. Committed as
  `.cargo/config.toml` (`[target.wasm32-unknown-unknown] rustflags`), un-ignored in
  `.gitignore` — it was local-only and gitignored before the first push, so CI broke.
- **Ubuntu rust job needs pipewire system deps**: `mediaway-device-linux`'s `pipewire`
  crate → `libpipewire-0.3-dev` + `libspa-0.2-dev` via apt (`libspa-sys` build script).
- **NVENC tests panic without the driver**: the `nvenc` crate unwraps its DLL load, so
  `_or_skip_without_hw` tests panicked on runners without NVENC. `NvencSession::open` now
  probes `nvEncodeAPI64.dll` first and returns `Err` (see `mediaway-encoder-nvenc`).
- **Stale fixture cache hides broken BLAKE3 constants**: a locally cached test-media
  fixture whose bytes still match an outdated constant passes pre-push and fails CI
  (which regenerates from scratch). Pre-push now clears `local/.cache/test-media`
  first; the avprobe fixture constant was recomputed after the MP4 mux fixes.
- **Hook scripts must be BusyBox-ash safe**: the scoop-shimmed `bash` on Windows
  rejects arrays (`PKGS=()`) and `${BASH_SOURCE[0]}` — hooks stay POSIX (`$0`, word
  splitting).
- **e2e-web wasm build**: `mediaway-device` is rlib-only (no cdylib → no `.wasm`
  artifact) — building it in the e2e `build-wasm.ts` list always threw "Missing wasm
  artifact". Dropped from the list; the CI wasm job still compile-gates it.
