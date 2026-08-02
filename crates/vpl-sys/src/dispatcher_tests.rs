//! Tests for [`super::Loader`] / [`super::Session`] — see
//! `docs/conventions/testing.md` Tier 1.
//!
//! Hardware-verified 2026-07-29 against this workspace's reference Windows
//! box's real Intel UHD 770 (`libmfxhw64.dll`, confirmed present under
//! `%SystemRoot%\System32` and exporting every entry point this crate needs —
//! see `dispatcher` module docs). Written to skip honestly (never panic) on a
//! host with no oneVPL runtime, since this crate must also stay usable there.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;
use crate::consts::MFX_IMPL_HARDWARE_ANY;

/// Opens the MVP dispatcher, creates a real `MFXInitEx` session against
/// [`MFX_IMPL_HARDWARE_ANY`] (see that constant's docs for why not the
/// narrower `MFX_IMPL_HARDWARE`), and queries its version/implementation —
/// the same session-open step `mediaway-encoder-quicksync`'s real encode
/// test builds on. Skips (does not fail) when no oneVPL implementation
/// library is found.
#[test]
fn session_opens_or_skips_without_onevpl_runtime() {
    let loader = match Loader::open() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("skip: Loader::open failed ({e}) — no oneVPL runtime on this host?");
            return;
        }
    };

    let mut session = match loader.create_session(MFX_IMPL_HARDWARE_ANY) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skip: create_session(MFX_IMPL_HARDWARE_ANY) failed ({e})");
            return;
        }
    };

    let version = session
        .query_version()
        .expect("MFXQueryVersion on a just-opened session should succeed");
    // SAFETY: reading a `mfxVersion` union field is only "unsafe" in the
    // Rust-language sense (any union read is); `MFXQueryVersion` above wrote
    // a real value through this same union, so both branches (`Version` and
    // `__bindgen_anon_1.{Major,Minor}`) are equally valid re-interpretations
    // of the same 4 bytes.
    let (raw_version, major, minor) = unsafe {
        (
            version.Version,
            version.__bindgen_anon_1.Major,
            version.__bindgen_anon_1.Minor,
        )
    };
    assert_ne!(
        raw_version, 0,
        "a real oneVPL runtime reports a nonzero version"
    );

    let impl_selected = session
        .query_impl()
        .expect("MFXQueryIMPL on a just-opened session should succeed");

    eprintln!(
        "vpl-sys: oneVPL session opened — Major={major} Minor={minor} raw_version=0x{raw_version:08x} impl=0x{impl_selected:08x}",
    );
}
