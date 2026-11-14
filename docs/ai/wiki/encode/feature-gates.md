# `mediaway-encoder` cargo features

Crate ADRs: [0004](../../../../crates/mediaway-encoder/adr/0004-av-feature-gates.md) (`audio` /
`video`), [0007](../../../../crates/mediaway-encoder/adr/0007-quicksync-feature.md) (`quicksync`).

- `default = ["full", "video", "quicksync"]`; `full` = `audio` + `video`.
- `quicksync` (implies `video`) is the only feature that pulls `vpl-sys`, whose build script runs
  `bindgen` and therefore needs **libclang**. Off, `quicksync::QuickSyncVideoEncoder` is the
  `Unsupported` stub (same as off Windows); `support()` says `NotImplemented` for Quick Sync.
- NVENC (`nvenc` crate) is hand-written bindings with no build step, so it is not gated: an
  NVENC/WMF-only Windows consumer uses `default-features = false, features = ["video"]` and builds
  without libclang.

## Verifying a change here

```bash
env -u LIBCLANG_PATH cargo clippy -p mediaway-encoder --no-default-features --features video -- -D warnings
cargo tree -p mediaway-encoder --no-default-features --features video -i vpl-sys   # "did not match"
cargo clippy -p mediaway-encoder --all-targets -- -D warnings                       # default set
```

Gotchas:

- `--no-default-features --features audio` (audio only) does not compile: several backend modules
  (`nvenc`, `quicksync`, `linux`, `android`, ...) are declared unconditionally and carry a
  `compile_error!` when `video` is off (pre-existing, not caused by `quicksync`).
- The web host stubs (`web/host.rs`) import audio-only types; keep those imports
  `#[cfg(feature = "audio")]` or a `video`-only build warns.
