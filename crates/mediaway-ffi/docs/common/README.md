# mediaway-common-ffi

Internal `rlib`-only helper shared by `mediaway-*-ffi` crates ([`mediaway-container-ffi`](../mediaway-container-ffi/README.md), [`mediaway-ffi`](../mediaway-ffi/README.md), [`mediaway-device-ffi`](../mediaway-device-ffi/README.md)).

**Not a public C ABI.** No `include/` header, no `cdylib`/`staticlib`, zero `#[no_mangle]`/`extern "C"` symbols of its own. It shares only the `Rational`/`CodecKind` `#[repr(C)]` value-type mirrors and the private buffer leak/reclaim helper implementation — each consuming crate keeps its own status enum, exported free-function names, and header.

Design decision: [`docs/adr/0015-common-ffi-unification.md`](../../docs/adr/0015-common-ffi-unification.md). See `docs/roadmap.md` for scope and `adr/` for the pointer back to that ADR.
