//! Parses vendored oneVPL headers (types only) into `OUT_DIR/mfx_types.rs`.
//!
//! Only `vendor/api/vpl/mfxstructures.h` (and its transitive `#include` chain —
//! `mfxcommon.h` -> `mfxdefs.h`) is fed to `bindgen`, with `ignore_functions()`
//! set: this crate wants byte-exact struct/union layout (oneVPL's headers use
//! `#pragma pack`) from real Clang parsing, not a hand-transcribed guess — but
//! never a build-time-linked `extern "C"` function declaration, since every
//! entry point is resolved at runtime via `libloading` (`src/dispatcher.rs`).
//! See `vendor/README.md` for the full rationale and the commit pin.
//!
//! Returns `Result` (never `unwrap`/`expect`/`panic!`) so a `bindgen` failure
//! surfaces as a normal cargo build-script error message, per this
//! workspace's "no new unwrap/expect/panic! outside tests" rule.

use std::env;
use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let vendor_dir = manifest_dir.join("vendor/api/vpl");
    let header = vendor_dir.join("mfxstructures.h");

    println!("cargo:rerun-if-changed={}", vendor_dir.display());

    let bindings = bindgen::Builder::default()
        .header(header.to_string_lossy().into_owned())
        .clang_arg(format!("-I{}", vendor_dir.display()))
        // Types only — no build-time-linked function declarations. See module docs.
        .ignore_functions()
        // Only the root types this crate's Stage 1 dispatcher/encoder actually
        // reference — `bindgen`'s default recursive allowlisting still pulls in
        // every type *they* depend on (e.g. `mfxFrameId` for `mfxFrameInfo`,
        // `mfxPayload`/`mfxEncryptedData` forward decls), so this stays complete
        // without generating oneVPL's hundreds of unrelated VPP/SEI ext-buffer
        // structs (smaller build, far fewer bindgen doc-comment warnings).
        .allowlist_type(
            "^mfx(FrameInfo|FrameData|FrameSurface1|InfoMFX|InfoVPP|VideoParam|Version\
|StructVersion|ExtBuffer|InitParam|Bitstream|EncodeCtrl|IMPL|Status|HandleType|FrameId)$",
        )
        .derive_default(true)
        .derive_debug(true)
        .layout_tests(false)
        .generate()?;

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    bindings.write_to_file(out_dir.join("mfx_types.rs"))?;
    Ok(())
}
