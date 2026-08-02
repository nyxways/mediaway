# Vendored oneVPL headers

Source: [`intel/libvpl`](https://github.com/intel/libvpl) (MIT), pinned at commit
`674d015bcb294bc39fa276e99a652ea045423e82` (`main` branch, fetched 2026-07-29).

Files under `api/vpl/`:

- `mfxdefs.h` — base scalar typedefs (`mfxU8`..`mfxU64`, `mfxHDL`, …), `mfxStatus`, `mfxStructVersion`.
- `mfxcommon.h` — `mfxVersion`, `mfxBitstream`, `mfxSyncPoint`, `mfxInitParam`, `mfxExtBuffer`.
- `mfxstructures.h` — `mfxFrameInfo`, `mfxFrameData`, `mfxFrameSurface1`, `mfxInfoMFX`,
  `mfxVideoParam`, `mfxEncodeCtrl`, `mfxHandleType`, and the codec/profile/level/IOPattern/
  rate-control constant definitions this crate's `consts` module transcribes by hand.
- `mfxsession.h`, `mfxvideo.h`, `mfxmemory.h` — kept as the **reference source of truth** for
  this crate's hand-written `extern "system"` function-pointer signatures (`dispatcher.rs`).
  **Not fed to `bindgen`** (see below) — `mfxvideo.h` only declares functions (no types this
  crate needs), and its own `#include "mfxmemory.h"` pulls in allocator-interface types this
  crate's CPU-upload Stage 1 does not use.

`LICENSE` alongside is `intel/libvpl`'s own MIT license text, copied verbatim for attribution —
applies to these vendored header files only, not to this crate's own Rust source (see the crate
`Cargo.toml` `license` field for that).

## What `build.rs` actually parses

Only `api/vpl/mfxstructures.h` (and its transitive `#include` chain — `mfxcommon.h` →
`mfxdefs.h`) is passed to `bindgen`, with `ignore_functions()` set: this crate wants **exact
struct/union layout** (oneVPL's headers use `#pragma pack` — 4-byte packing for the plain
"usual" structs, 8-byte for the pointer-bearing ones on the `LP64`/Win64 data model this crate
targets) verified by real Clang parsing, not hand-transcribed by an agent. It deliberately does
**not** want `bindgen` to emit linked `extern "C" { fn MFXInit(...); }` declarations, since this
crate resolves every entry point at runtime via `libloading` (`dispatcher.rs`) — never a
build-time link against an Intel-provided import library. All numeric constants (`MFX_ERR_*`,
`MFX_IMPL_*`, `MFX_CODEC_AVC`, `MFX_FOURCC_NV12`, `MFX_RATECONTROL_*`, …) and every
`extern "system"` function-pointer signature are hand-transcribed in `src/consts.rs` /
`src/dispatcher.rs` from the verbatim header text vendored here, cited by file + line in each
transcription's doc comment.

## Updating the pin

Re-fetch the five files above at a new `intel/libvpl` commit, update the SHA in this file, and
re-run `cargo test -p vpl-sys` (`_or_skip` hardware test) plus `cargo check -p vpl-sys
-p mediaway-encoder-quicksync` before committing — a header change can silently shift a struct
layout `bindgen` regenerates.
