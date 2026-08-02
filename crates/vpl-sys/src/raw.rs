//! Generated oneVPL struct/union/typedef bindings (types only — see `build.rs`),
//! plus the handful of opaque handle types `bindgen` never sees because their
//! home headers (`mfxsession.h`) are not part of this crate's bindgen input
//! (see `vendor/README.md`).
//!
//! C naming throughout — not `snake_case`/`CamelCase` by Rust convention, matching
//! every other `-sys` crate.
#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]
// Covers every clippy tier (default/style/complexity/pedantic/nursery) for this
// crate's `bindgen`-generated code only — bitfield accessor helpers for union
// branches this crate's Stage 1 does not use (e.g. `mfxA2RGB10`'s packed
// B/G/R/A bitfields, pulled in only because they are a union member of
// `mfxFrameData`) trip `useless_transmute`/`missing_safety_doc`/pedantic
// lints that make no sense to hand-patch in generated output.
#![allow(clippy::all, clippy::pedantic, clippy::nursery, missing_docs)]
// `bindgen`'s generated bitfield accessor helpers (`__BindgenBitfieldUnit::raw_get`/`raw_set`,
// used by the `#ifdef ONEVPL_EXPERIMENTAL`-only bitfield branch bindgen still parses the layout
// of even though that macro is undefined for this crate's build) predate Rust 2024's
// `unsafe_op_in_unsafe_fn` default; this crate never calls those bitfield accessors itself
// (Stage 1 does not touch that struct branch), so the lint is silenced rather than hand-patched
// generated code.
#![allow(unsafe_op_in_unsafe_fn)]

include!(concat!(env!("OUT_DIR"), "/mfx_types.rs"));

/// Opaque oneVPL session handle (`mfxSession`, `typedef struct _mfxSession *mfxSession;` —
/// `mfxsession.h`). An opaque pointer has no fields to get wrong by hand-declaring it here;
/// `mfxsession.h` itself is not part of this crate's `bindgen` input (see `vendor/README.md`).
pub type mfxSession = *mut core::ffi::c_void;

/// Synchronization point object handle (`mfxSyncPoint`,
/// `typedef struct _mfxSyncPoint *mfxSyncPoint;` — `mfxcommon.h` line 186). Forward-declared
/// only upstream (never a defined struct body anywhere in oneVPL), and not reachable from any
/// of this crate's `bindgen`-allowlisted root types (only referenced by function *signatures*,
/// which `build.rs` ignores — see `vendor/README.md`), so hand-declared here like `mfxSession`
/// above.
pub type mfxSyncPoint = *mut core::ffi::c_void;
