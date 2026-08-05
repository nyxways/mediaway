# mediaway-common-ffi — roadmap

Internal `rlib` shared by `mediaway-*-ffi` crates. Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — Partial unification (this crate's only stage so far)

- [x] Design: [`docs/adr/0015-common-ffi-unification.md`](../../../docs/adr/0015-common-ffi-unification.md)
      — `rlib`-only, header-less, zero `#[no_mangle]`/`extern "C"` symbols of its own
- [x] `src/types.rs` — `Rational`/`CodecKind` `#[repr(C)]` mirrors + conversions
      (moved from `mediaway-container-ffi`, confirmed field-identical to
      `mediaway-ffi`'s independent copy)
- [x] `src/buffer.rs` — `borrow_slice`/`leak_boxed_slice`/`reclaim_boxed_slice`,
      now `pub` (crosses a crate boundary)
- [x] `mediaway-container-ffi`, `mediaway-ffi`, `mediaway-device-ffi` migrated
      to depend on this crate and re-export its types under their own local alias;
      each crate's exported C type names, header, and free-function names unchanged
- [x] `CodecKind::Vp8 = 12` added, mirroring `mediaway_common::CodecKind::Vp8`
      (closes the WebM VP8 gap — see `mediaway-container/docs/roadmap.md`
      Stage 2). Pre-1.0 discriminant append, per this type's documented
      "values may be renumbered" posture — 2026-08-05.

## Out of scope (by design, not a gap)

- Unifying the per-crate status enums (`MediawayStatus`, `MediawayPipelineStatus`,
  `MediawayDeviceStatus`) — deliberately not attempted, see ADR-0015 § Deferred.
- A shared C header / shared exported free function — this crate never gets a
  `cdylib`/`staticlib` or `include/` directory.
- `PixelFormat` mirroring — not decided by ADR-0015; `mediaway-ffi` and
  `mediaway-device-ffi` each keep their own local `MediawayPixelFormat` copy for now.
