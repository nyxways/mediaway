# vpl-sys

<p align="center">
  <a href="https://docs.rs/vpl-sys"><img src="https://img.shields.io/docsrs/vpl-sys" alt="docs.rs"></a>
  <a href="https://crates.io/crates/vpl-sys"><img src="https://img.shields.io/crates/v/vpl-sys.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Low-level FFI bindings to a subset of Intel oneVPL (`libvpl`): `bindgen`-generated
struct/union layouts from vendored headers, hand-cited `MFX_*` constants, and a
deliberately reduced dispatcher that resolves a driver-shipped implementation library
(`libmfxhw64.dll` on Windows) at runtime via `libloading` — no build-time link against an
Intel import library.

## Quick start

```rust
use vpl_sys::Loader;

let loader = Loader::open()?; // resolve oneVPL at runtime (no build-time link)
let mut session = loader.create_session(0)?; // MFX_IMPL_HARDWARE
let version = session.query_version()?;
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Raw oneVPL type layouts (vendored headers, Clang-verified) | ✅ | `raw` module |
| `MFX_*` constants | ✅ | `consts` module, each cited to its header |
| Runtime dispatcher (`Loader` / `Session`) | ✅ | Reduced reimplementation; first working Intel GPU impl wins |
| Encode entry points | ✅ | ~10 `MFX*` fns for the H.264 CPU-upload encode path |
| Multi-implementation ranking / capability filtering | 🛠️ | Not part of this Stage-1 dispatcher |

## Docs

- Consumer: Intel Quick Sync encode (`mediaway-encoder` `quicksync` module)
- Root [README](../../README.md) — workspace overview

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
