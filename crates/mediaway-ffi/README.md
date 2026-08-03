# mediaway-ffi

Single C ABI facade over Mediaway's container/device/pipeline capabilities
(merged from the four `mediaway-*-ffi` crates per ADR-0021).

- **One cdylib** (`mediaway_ffi.dll` / `libmediaway_ffi.so` / `mediaway_ffi.dylib`)
  exporting the combined C surface — see `include/mediaway/` for the hand-written
  headers (`container.h`, `device.h`, `pipeline.h`).
- Opaque handles + integer status codes; no panics/unwinding across the boundary.
- Internally organized as `common` (shared value-type mirrors + buffer helpers),
  `container`, `device`, and `pipeline` modules.
- Crate-local ADRs live in `adr/` per module; docs in `docs/`.

Design rules: [`docs/spec/c-ffi.md`](../../docs/spec/c-ffi.md) (ADR-0004, amended
by ADR-0021).
