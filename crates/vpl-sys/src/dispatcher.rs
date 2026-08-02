//! MVP dispatcher: load a driver-shipped oneVPL implementation at runtime.
//!
//! Resolves the ~10 entry points this crate's Stage 1 CPU-upload H.264
//! encode path needs, via `libloading`'s `GetProcAddress`/`dlsym`.
//!
//! This is a **deliberately reduced reimplementation** of Intel's own
//! dispatcher (`MFXLoad`/`MFXCreateConfig`/`MFXEnumImplementations`/
//! `MFXCreateSession`, exported by `libvpl.dll` on this machine) — see
//! `mediaway-encoder-quicksync/adr/0001-onevpl-quicksync-encode-surface.md`
//! for why: no vendored/built Intel C dispatcher, no import-lib linking.
//! **First working Intel GPU implementation wins**; multi-adapter selection,
//! full capability filtering, and CPU-implementation fallback are out of
//! scope. Do not assume official-dispatcher parity (env-var config files,
//! versioned implementation ranking, …).
//!
//! Verified on this workspace's Windows dev box (Intel UHD 770,
//! `iigd_dch.inf` driver package): the real implementation library
//! (`libmfxhw64.dll`) is present directly under `%SystemRoot%\System32` and
//! exports `MFXInit`/`MFXInitEx`/`MFXClose`/`MFXQueryIMPL`/`MFXQueryVersion`/
//! `MFXVideoCORE_*`/`MFXVideoENCODE_*` directly (confirmed via
//! `llvm-readobj --coff-exports`, not assumed) — exactly the "runtime library
//! itself directly exports the MFX* entry points" shape the ADR's license
//! research documented from Intel's own docs.

#![allow(unsafe_code)]

use std::env;
use std::ffi::c_void;
use std::path::PathBuf;

use libloading::Library;
use thiserror::Error;

use crate::consts::{MFX_ERR_NONE, mfx_succeeded};
use crate::raw::{
    mfxBitstream, mfxEncodeCtrl, mfxFrameSurface1, mfxHandleType, mfxIMPL, mfxInitParam,
    mfxSession, mfxStatus, mfxSyncPoint, mfxVersion, mfxVideoParam,
};

/// Errors opening the MVP dispatcher or a oneVPL session on it.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VplError {
    /// No candidate oneVPL implementation library loaded (see
    /// [`Loader::open`]'s search order).
    #[error("no oneVPL implementation library found (tried: {tried})")]
    NotFound {
        /// Candidate paths/names tried, joined with `", "`, for diagnostics.
        tried: String,
    },
    /// A required entry point was missing from an otherwise-loaded library —
    /// loaded the wrong DLL, or a genuinely incompatible/older runtime.
    #[error("required oneVPL entry point {symbol} missing from the loaded library")]
    MissingSymbol {
        /// The C symbol name that failed to resolve.
        symbol: &'static str,
    },
    /// A oneVPL call returned a negative (error) `mfxStatus`.
    #[error("oneVPL call {call} failed: mfxStatus {status}")]
    Status {
        /// Name of the failing `MFX*` call, for diagnostics.
        call: &'static str,
        /// The raw `mfxStatus` value returned.
        status: mfxStatus,
    },
}

type PfnMfxInitEx = unsafe extern "C" fn(par: mfxInitParam, session: *mut mfxSession) -> mfxStatus;
type PfnMfxClose = unsafe extern "C" fn(session: mfxSession) -> mfxStatus;
type PfnMfxQueryVersion =
    unsafe extern "C" fn(session: mfxSession, version: *mut mfxVersion) -> mfxStatus;
type PfnMfxQueryImpl = unsafe extern "C" fn(session: mfxSession, r#impl: *mut mfxIMPL) -> mfxStatus;
type PfnMfxVideoEncodeQuery = unsafe extern "C" fn(
    session: mfxSession,
    r#in: *mut mfxVideoParam,
    out: *mut mfxVideoParam,
) -> mfxStatus;
type PfnMfxVideoEncodeInit =
    unsafe extern "C" fn(session: mfxSession, par: *mut mfxVideoParam) -> mfxStatus;
type PfnMfxVideoEncodeClose = unsafe extern "C" fn(session: mfxSession) -> mfxStatus;
type PfnMfxVideoEncodeEncodeFrameAsync = unsafe extern "C" fn(
    session: mfxSession,
    ctrl: *mut mfxEncodeCtrl,
    surface: *mut mfxFrameSurface1,
    bs: *mut mfxBitstream,
    syncp: *mut mfxSyncPoint,
) -> mfxStatus;
type PfnMfxVideoCoreSyncOperation =
    unsafe extern "C" fn(session: mfxSession, syncp: mfxSyncPoint, wait: u32) -> mfxStatus;
type PfnMfxVideoCoreSetHandle = unsafe extern "C" fn(
    session: mfxSession,
    handle_type: mfxHandleType,
    hdl: *mut c_void,
) -> mfxStatus;

/// A loaded oneVPL implementation library plus its resolved Stage 1 entry points.
///
/// Owns the `libloading::Library` (kept mapped for as long as any [`Session`]
/// built from it is alive — every fn pointer field is only ever called
/// through `&self`/`&Session` methods, never after both are dropped).
pub struct Loader {
    _lib: Library,
    fn_init_ex: PfnMfxInitEx,
    fn_close: PfnMfxClose,
    fn_query_version: PfnMfxQueryVersion,
    fn_query_impl: PfnMfxQueryImpl,
    fn_encode_query: PfnMfxVideoEncodeQuery,
    fn_encode_init: PfnMfxVideoEncodeInit,
    fn_encode_close: PfnMfxVideoEncodeClose,
    fn_encode_frame_async: PfnMfxVideoEncodeEncodeFrameAsync,
    fn_core_sync_operation: PfnMfxVideoCoreSyncOperation,
    #[allow(
        dead_code,
        reason = "Stage 1 is CPU-upload only; wired for the D3D11 ZC follow-up"
    )]
    fn_core_set_handle: PfnMfxVideoCoreSetHandle,
}

/// Default search list: on Windows, `libmfxhw64.dll` is the real Intel GPU
/// implementation library shipped inside the graphics driver package
/// (confirmed present directly under `%SystemRoot%\System32` on this
/// workspace's Intel UHD 770 dev box). Bare names are resolved by the OS
/// loader's standard search order (which includes `System32`), so no full
/// path is required in the common case.
#[cfg(windows)]
const DEFAULT_CANDIDATES: &[&str] = &["libmfxhw64.dll"];

#[cfg(not(windows))]
const DEFAULT_CANDIDATES: &[&str] = &[];

impl Loader {
    /// Open the first working implementation library: `ONEVPL_SEARCH_PATH`
    /// (if set, a single directory) is tried first for each candidate name,
    /// then each candidate's bare name (OS default search order). Returns
    /// [`VplError::NotFound`] if nothing loads, or
    /// [`VplError::MissingSymbol`] if a candidate loads but is missing one of
    /// this crate's required entry points (wrong/incompatible DLL).
    ///
    /// # Errors
    ///
    /// See variants above.
    pub fn open() -> Result<Self, VplError> {
        let mut tried = Vec::new();
        let search_dir = env::var_os("ONEVPL_SEARCH_PATH").map(PathBuf::from);

        for candidate in DEFAULT_CANDIDATES {
            if let Some(dir) = &search_dir {
                let full = dir.join(candidate);
                tried.push(full.display().to_string());
                // SAFETY: `Library::new` maps a PE image at a caller-supplied
                // path; oneVPL implementation DLLs run arbitrary module-init
                // code like any other native library, an accepted risk for
                // this crate's whole reason to exist. No symbols are used
                // before `resolve` validates every one this crate needs.
                if let Ok(lib) = unsafe { Library::new(&full) } {
                    return Self::resolve(lib);
                }
            }
            tried.push((*candidate).to_string());
            // SAFETY: same as above, bare name resolved via the OS loader's
            // standard search order (includes `System32` on Windows, where
            // this crate's default candidate is confirmed present).
            if let Ok(lib) = unsafe { Library::new(candidate) } {
                return Self::resolve(lib);
            }
        }

        Err(VplError::NotFound {
            tried: tried.join(", "),
        })
    }

    fn resolve(lib: Library) -> Result<Self, VplError> {
        macro_rules! sym {
            ($name:literal) => {{
                // SAFETY: `$name` is a NUL-terminated C symbol name; the
                // resulting function pointer is only ever called with
                // arguments matching the real oneVPL C signature transcribed
                // in this module's `Pfn*` typedefs (verified against the
                // vendored headers — see `vendor/README.md`), and only for as
                // long as `self._lib` (this same `Library`) stays alive.
                match unsafe { lib.get(concat!($name, "\0").as_bytes()) } {
                    Ok(sym) => *sym,
                    Err(_) => {
                        return Err(VplError::MissingSymbol { symbol: $name });
                    }
                }
            }};
        }

        let fn_init_ex = sym!("MFXInitEx");
        let fn_close = sym!("MFXClose");
        let fn_query_version = sym!("MFXQueryVersion");
        let fn_query_impl = sym!("MFXQueryIMPL");
        let fn_encode_query = sym!("MFXVideoENCODE_Query");
        let fn_encode_init = sym!("MFXVideoENCODE_Init");
        let fn_encode_close = sym!("MFXVideoENCODE_Close");
        let fn_encode_frame_async = sym!("MFXVideoENCODE_EncodeFrameAsync");
        let fn_core_sync_operation = sym!("MFXVideoCORE_SyncOperation");
        let fn_core_set_handle = sym!("MFXVideoCORE_SetHandle");

        Ok(Self {
            _lib: lib,
            fn_init_ex,
            fn_close,
            fn_query_version,
            fn_query_impl,
            fn_encode_query,
            fn_encode_init,
            fn_encode_close,
            fn_encode_frame_async,
            fn_core_sync_operation,
            fn_core_set_handle,
        })
    }

    /// `MFXInitEx` — create a session against `impl_hint` (typically
    /// [`crate::consts::MFX_IMPL_HARDWARE`]). Consumes `self`: a [`Session`]
    /// owns its `Loader` for the rest of its lifetime (every later call
    /// resolves through the same loaded library).
    ///
    /// # Errors
    ///
    /// [`VplError::Status`] when `MFXInitEx` returns a negative `mfxStatus`.
    pub fn create_session(self, impl_hint: mfxIMPL) -> Result<Session, VplError> {
        let par = mfxInitParam {
            Implementation: impl_hint,
            ..Default::default()
        };
        let mut session: mfxSession = std::ptr::null_mut();
        // SAFETY: `fn_init_ex` was resolved from `MFXInitEx`, called with a
        // `mfxInitParam` built from the real header layout (`bindgen`
        // generated) and a valid out-pointer for the session handle.
        let status = unsafe { (self.fn_init_ex)(par, &raw mut session) };
        if !mfx_succeeded(status) {
            return Err(VplError::Status {
                call: "MFXInitEx",
                status,
            });
        }
        Ok(Session {
            loader: self,
            session,
        })
    }
}

/// An open oneVPL session (`MFXInitEx` succeeded) plus the `Loader` it was created from.
///
/// `Drop` calls `MFXClose` — callers that already closed the encoder should
/// still let this run (idempotent-safe: a session is only ever closed once,
/// `Session` has no `close`-without-drop path this stage).
pub struct Session {
    loader: Loader,
    session: mfxSession,
}

// SAFETY: `mfxSession` (a raw pointer) does not implement `Send`/`Sync` by
// default; oneVPL sessions are documented as safe to move across threads as
// long as calls into the same session are not made concurrently from
// multiple threads — this crate never does (every `Session` method takes
// `&mut self`), so `Send` is sound. Not `Sync` (no shared-reference calls).
unsafe impl Send for Session {}

impl Session {
    /// `MFXQueryVersion`.
    ///
    /// # Errors
    ///
    /// [`VplError::Status`] on a negative `mfxStatus`.
    pub fn query_version(&mut self) -> Result<mfxVersion, VplError> {
        let mut version = mfxVersion::default();
        // SAFETY: `fn_query_version` was resolved from `MFXQueryVersion`;
        // `self.session` came from a successful `MFXInitEx` in `Loader::create_session`.
        let status = unsafe { (self.loader.fn_query_version)(self.session, &raw mut version) };
        Self::check("MFXQueryVersion", status)?;
        Ok(version)
    }

    /// `MFXQueryIMPL` — which implementation the runtime actually selected
    /// (hardware vs. a software fallback the caller did not ask for).
    ///
    /// # Errors
    ///
    /// [`VplError::Status`] on a negative `mfxStatus`.
    pub fn query_impl(&mut self) -> Result<mfxIMPL, VplError> {
        let mut out: mfxIMPL = 0;
        // SAFETY: see `query_version`.
        let status = unsafe { (self.loader.fn_query_impl)(self.session, &raw mut out) };
        Self::check("MFXQueryIMPL", status)?;
        Ok(out)
    }

    /// `MFXVideoENCODE_Query` — validate/adjust `par` before `Init`.
    ///
    /// # Errors
    ///
    /// [`VplError::Status`] on a negative `mfxStatus`.
    pub fn encode_query(&mut self, par: &mut mfxVideoParam) -> Result<mfxStatus, VplError> {
        let mut out = *par;
        // SAFETY: `par`/`out` are real `mfxVideoParam` values (bindgen-generated
        // layout); `fn_encode_query` was resolved from `MFXVideoENCODE_Query`.
        let status =
            unsafe { (self.loader.fn_encode_query)(self.session, &raw mut *par, &raw mut out) };
        if status < MFX_ERR_NONE {
            return Err(VplError::Status {
                call: "MFXVideoENCODE_Query",
                status,
            });
        }
        *par = out;
        Ok(status)
    }

    /// `MFXVideoENCODE_Init`.
    ///
    /// # Errors
    ///
    /// [`VplError::Status`] on a negative `mfxStatus`.
    pub fn encode_init(&mut self, par: &mut mfxVideoParam) -> Result<(), VplError> {
        // SAFETY: see `encode_query`.
        let status = unsafe { (self.loader.fn_encode_init)(self.session, &raw mut *par) };
        Self::check("MFXVideoENCODE_Init", status)
    }

    /// `MFXVideoENCODE_Close`.
    ///
    /// # Errors
    ///
    /// [`VplError::Status`] on a negative `mfxStatus`.
    pub fn encode_close(&mut self) -> Result<(), VplError> {
        // SAFETY: `fn_encode_close` resolved from `MFXVideoENCODE_Close`.
        let status = unsafe { (self.loader.fn_encode_close)(self.session) };
        Self::check("MFXVideoENCODE_Close", status)
    }

    /// `MFXVideoENCODE_EncodeFrameAsync`. Pass `surface = None` to signal
    /// end-of-stream (drain buffered frames during [flush][crate]).
    ///
    /// Returns the raw `mfxStatus` (not just success/error) so the caller can
    /// distinguish `MFX_ERR_NONE` (packet ready, `syncp` populated) from
    /// `MFX_ERR_MORE_DATA` (no packet yet — not a failure) and
    /// `MFX_WRN_DEVICE_BUSY` (retry).
    ///
    /// # Errors
    ///
    /// [`VplError::Status`] only for status values this crate does not know
    /// how to interpret as non-fatal (i.e. a genuine negative status other
    /// than `MFX_ERR_MORE_DATA`).
    pub fn encode_frame_async(
        &mut self,
        surface: Option<&mut mfxFrameSurface1>,
        bs: &mut mfxBitstream,
    ) -> Result<(mfxStatus, mfxSyncPoint), VplError> {
        let surface_ptr = surface.map_or(std::ptr::null_mut(), std::ptr::from_mut);
        let mut syncp: mfxSyncPoint = std::ptr::null_mut();
        // SAFETY: `bs`/`surface` (when present) are real, live oneVPL structs;
        // `fn_encode_frame_async` resolved from `MFXVideoENCODE_EncodeFrameAsync`.
        let status = unsafe {
            (self.loader.fn_encode_frame_async)(
                self.session,
                std::ptr::null_mut(),
                surface_ptr,
                std::ptr::from_mut(bs),
                &raw mut syncp,
            )
        };
        Ok((status, syncp))
    }

    /// `MFXVideoCORE_SyncOperation` — block (up to `wait_ms`) for `syncp` to
    /// complete, after a `MFX_ERR_NONE` [`Self::encode_frame_async`].
    ///
    /// # Errors
    ///
    /// [`VplError::Status`] on a negative `mfxStatus`.
    #[allow(
        clippy::not_unsafe_ptr_arg_deref,
        reason = "syncp (mfxSyncPoint) is an opaque handle this crate never dereferences itself \
                  — it is only ever forwarded verbatim to MFXVideoCORE_SyncOperation, which owns \
                  it; the raw pointer type just mirrors oneVPL's own opaque-handle C API"
    )]
    pub fn sync_operation(&mut self, syncp: mfxSyncPoint, wait_ms: u32) -> Result<(), VplError> {
        // SAFETY: `syncp` came from a `MFX_ERR_NONE` `encode_frame_async` on
        // this same session; `fn_core_sync_operation` resolved from
        // `MFXVideoCORE_SyncOperation`.
        let status = unsafe { (self.loader.fn_core_sync_operation)(self.session, syncp, wait_ms) };
        Self::check("MFXVideoCORE_SyncOperation", status)
    }

    const fn check(call: &'static str, status: mfxStatus) -> Result<(), VplError> {
        if mfx_succeeded(status) {
            Ok(())
        } else {
            Err(VplError::Status { call, status })
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: `self.session` is a valid handle from a successful
        // `MFXInitEx` (or already-nulled by nothing else — `Session` never
        // exposes a way to null it before `Drop`); `self.loader` is still
        // alive (it is a field of `self`, dropped only after this fn
        // returns). Ignoring the status here matches every other Windows/
        // Linux backend's teardown-`Drop` convention in this workspace (no
        // `unwrap`/`panic!` in a destructor).
        let _status: mfxStatus = unsafe { (self.loader.fn_close)(self.session) };
    }
}

#[cfg(test)]
#[path = "dispatcher_tests.rs"]
mod tests;
