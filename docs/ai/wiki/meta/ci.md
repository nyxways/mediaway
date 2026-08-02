# GitHub Actions CI

Canonical: [`docs/conventions/hooks.md`](../../../conventions/hooks.md) § CI · workflow [`.github/workflows/ci.yml`](../../../../.github/workflows/ci.yml).

- `rust` job: Windows + Ubuntu — fmt, clippy, test, source ≤1000 lines
- `deny` job: Ubuntu — `cargo deny`
- `wasm` job: Ubuntu — builds `iso-bmff-wasm` / `mediaway-encoder-web` / `mediaway-device-web` for `wasm32-unknown-unknown`
- No GPU / system FFmpeg in default CI

## Lessons from the first push (2026-08-02)

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
