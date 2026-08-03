# C-FFI

Canonical: [`docs/spec/c-ffi.md`](../../../spec/c-ffi.md) · ADR-0004.

- Per-capability: `mediaway-ffi`, …; unprefixed cores may ship `iso-bmff-ffi` (primary)
- Optional umbrella `mediaway-ffi` with **empty defaults** + additive features
- `mediaway-common` types OK; avoid fat all-backend links
- Node JS/TS via C ABI; browser via WASM
