# ADR-0001: Consolidate shared C header types into `common.h`

- **Status**: Accepted
- **Date**: 2026-08-05
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi`

## Context

After [ADR-0021](../../../../docs/adr/0021-workspace-consolidation.md) merged the four
`mediaway-*-ffi` crates into this one crate's `common`/`container`/`device`/`pipeline`
modules, the three hand-written headers (`include/mediaway/{container,device,pipeline}.h`)
still each textually redefined the value types they share
(`mediaway_rational_t`, `mediaway_pixel_format_t`, `mediaway_sample_format_t`,
`mediaway_gpu_device_kind_t`/`_handle_t`, `mediaway_gpu_buffer_kind_t`/`_handle_t`,
`mediaway_video_frame_storage_kind_t`) — a leftover from when they were three
independently-compiled crates with no shared header
(`docs/adr/0015-common-ffi-unification.md`, subsumed by ADR-0021).

Each duplicate definition was already wrapped in a matching `#ifndef
MEDIAWAY_*_T_DEFINED` guard with an identical macro name across all three headers, so a
translation unit that `#include`s more than one of them does **not** actually hit a C
redefinition error today — verified directly (`gcc -fsyntax-only`, both include orders,
both C and C++ mode) against the pre-this-ADR headers before making any change. The
"duplicate `mediaway_rational_t` tag redefinition" hazard recorded in
`docs/ai/wiki/device/ffi-c-abi.md` and `crates/mediaway-ffi/docs/device/roadmap.md` §
Deferred was accurate when written but is **stale**: the guard workaround already closed
it. What remains is a real (if non-blocking) maintenance problem — the same type
definition hand-copied into three files that must be kept byte-for-byte identical by
convention, not by the compiler.

## Decision

> Extract the eight shared value types into a new `include/mediaway/common.h`, guarded
> only by its own file-level include guard (`MEDIAWAY_COMMON_H`). `container.h`,
> `device.h`, and `pipeline.h` each `#include "common.h"` and no longer define these
> types themselves.

- **In scope**: `mediaway_rational_t`, `mediaway_pixel_format_t`,
  `mediaway_sample_format_t`, `mediaway_gpu_device_kind_t`,
  `mediaway_gpu_device_handle_t`, `mediaway_gpu_buffer_kind_t`,
  `mediaway_gpu_buffer_handle_t`, `mediaway_video_frame_storage_kind_t` — every type
  that was byte-for-byte identical across the headers that used it. Mirrors this crate's
  already-unified Rust side (`src/common/types.rs`, `src/common/gpu.rs`).
- **Out of scope, deliberately**: `mediaway_codec_kind_t` (container.h) vs.
  `mediaway_pipeline_codec_kind_t` (pipeline.h), and each module's own status enum
  (`mediaway_status_t` / `mediaway_device_status_t` / `mediaway_pipeline_status_t`) and
  buffer-free function name. These stay independently-named by prior, still-valid ADR
  decisions (`adr/pipeline/0001-auto-encode-c-abi.md` §2) — this ADR does not revisit
  that reasoning, only the types that were already meant to be identical.
- The per-type `#ifndef ..._T_DEFINED` guards are removed from the three consumer
  headers along with the duplicated bodies — `common.h`'s own file guard is now the only
  guard needed, since the type now has exactly one textual definition to guard.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Leave as-is (three copies + matching guards) | Works today, but the definitions must stay hand-synchronized across three files with no compiler check that a future edit to one is mirrored in the others two |
| Fold this into the `cbindgen` migration instead | `cbindgen` adoption (`docs/adr/0016-cbindgen-ffi-headers.md`) is separate, larger, and still-open infrastructure work; this file-level fix is small, safe, and not worth blocking on that migration's timeline. A future `cbindgen`-generated header can still emit `common.h`'s types once, generated instead of hand-written — this ADR does not foreclose that. |

## Consequences

### Positive

- One textual definition per shared type; editing one now visibly touches only
  `common.h`.
- Matches ADR-0021's stated "one header set, no cross-header struct drift" goal, which
  the crate merge alone had not yet delivered for the header text.

### Negative / Trade-offs

- One more file consumers must have on their include path (already true — `common.h`
  sits next to the other three under `include/mediaway/`, same `-I` flag covers it).

## References

- [ADR-0021](../../../../docs/adr/0021-workspace-consolidation.md) — crate merge this
  follows up on
- [`docs/adr/0015-common-ffi-unification.md`](../../../../docs/adr/0015-common-ffi-unification.md) — original (subsumed) Rust-side unification
- [`docs/adr/0016-cbindgen-ffi-headers.md`](../../../../docs/adr/0016-cbindgen-ffi-headers.md) — separate, still-open header-generation question
- `src/common/types.rs`, `src/common/gpu.rs` — the Rust-side types `common.h` mirrors
- `docs/ai/wiki/device/ffi-c-abi.md`, `docs/ai/wiki/pipeline/ffi-c-abi.md` — updated alongside this ADR

ADRs are **English**. Numbering is local to this `adr/` folder.
