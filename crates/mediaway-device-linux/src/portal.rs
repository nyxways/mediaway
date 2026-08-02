//! `xdg-desktop-portal` `ScreenCast` session handshake (D-Bus via `ashpd`).
//!
//! [`map_ashpd_error`] is pure and unit-testable without any D-Bus/portal
//! access. [`open_portal_stream`] performs real D-Bus + `PipeWire`-remote I/O
//! and cannot be exercised without a portal-capable desktop session — see
//! crate ADR-0001 (zero such verification happened in this development
//! session).

#![forbid(unsafe_code)]

use std::future::Future;
use std::os::fd::OwnedFd;
use std::pin::pin;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::task::{Context, Poll, Wake, Waker};

use ashpd::desktop::screencast::{
    CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
    StartCastOptions,
};
use ashpd::desktop::{CreateSessionOptions, PersistMode};
use mediaway_device::CaptureError;

/// `PipeWire` node id + remote fd returned by a successful portal handshake.
pub(crate) struct PortalStream {
    /// `PipeWire` node id the compositor is streaming this capture to.
    pub(crate) node_id: u32,
    /// Remote fd for [`pipewire::context::Context::connect_fd`].
    pub(crate) remote_fd: OwnedFd,
}

/// Run the `ScreenCast` portal session handshake to completion for
/// `source_type` and return the `PipeWire` node id + remote fd it hands back.
///
/// `source_type` selects which portal picker the user sees —
/// [`SourceType::Monitor`] for [`crate::screencast::LinuxScreenCapture`],
/// [`SourceType::Window`] for [`crate::window::LinuxWindowCapture`]. Both
/// share this one handshake; the portal's own UI is what actually
/// distinguishes "pick a monitor" from "pick a window", not a different
/// D-Bus call.
///
/// Blocks the calling thread until the user responds to the portal's picker /
/// permission prompt (or the request fails). Intended to run on a dedicated
/// worker thread (see `screencast.rs`), not the caller of
/// `LinuxScreenCapture::open`/`LinuxWindowCapture::open`.
///
/// # Errors
///
/// Returns the raw [`ashpd::Error`] — map with [`map_ashpd_error`] at the
/// `CaptureError` boundary.
pub(crate) fn open_portal_stream(source_type: SourceType) -> Result<PortalStream, ashpd::Error> {
    block_on(open_portal_stream_async(source_type))
}

async fn open_portal_stream_async(source_type: SourceType) -> ashpd::Result<PortalStream> {
    let proxy = Screencast::new().await?;
    let session = proxy
        .create_session(CreateSessionOptions::default())
        .await?;
    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Hidden)
                .set_sources(Some(source_type.into()))
                .set_multiple(false)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await?;

    let response = proxy
        .start(&session, None, StartCastOptions::default())
        .await?
        .response()?;
    let node_id = response
        .streams()
        .first()
        .ok_or(ashpd::Error::NoResponse)?
        .pipe_wire_node_id();

    let remote_fd = proxy
        .open_pipe_wire_remote(&session, OpenPipeWireRemoteOptions::default())
        .await?;

    Ok(PortalStream { node_id, remote_fd })
}

/// Cheaply checks whether the `ScreenCast` portal interface is reachable.
///
/// Connects to `org.freedesktop.portal.Desktop` and confirms the `ScreenCast`
/// interface exists, but creates no session — unlike [`open_portal_stream`],
/// this shows no consent dialog. Used for [`crate::capabilities::support`],
/// not [`crate::capabilities::request_permission`].
///
/// # Errors
///
/// Returns the raw [`ashpd::Error`] when the portal/D-Bus session isn't
/// reachable at all (e.g. no desktop session, no portal implementation
/// installed) — the caller only cares whether this succeeded or not.
pub(crate) fn probe_screencast() -> Result<(), ashpd::Error> {
    block_on(async { Screencast::new().await.map(|_| ()) })
}

/// Map an [`ashpd::Error`] to the facade's [`CaptureError`].
#[must_use]
pub(crate) const fn map_ashpd_error(err: &ashpd::Error) -> CaptureError {
    match err {
        ashpd::Error::Response(_) | ashpd::Error::Portal(_) => CaptureError::AccessDenied,
        ashpd::Error::PortalNotFound(_) => CaptureError::NoBackend,
        ashpd::Error::Zbus(_) | ashpd::Error::NoResponse | ashpd::Error::IO(_) => {
            CaptureError::Backend
        }
        _ => CaptureError::Unsupported,
    }
}

/// Poll-park-repoll block on a single [`Future`] to completion, on the calling
/// thread.
///
/// `ashpd`'s `async-io` feature already drives D-Bus socket readiness via
/// `async-io`'s own background reactor thread — any correct [`Waker`] wakes
/// this loop once that reactor marks the future ready, so a hand-rolled
/// executor is enough here and avoids a **third** new async-runtime dependency
/// (`async-io`'s or `futures-lite`'s own `block_on`) on top of `ashpd` +
/// `pipewire` (see ADR-0001 dependency review).
fn block_on<F: Future>(fut: F) -> F::Output {
    struct ParkSignal {
        ready: Mutex<bool>,
        cvar: Condvar,
    }

    impl Wake for ParkSignal {
        fn wake(self: Arc<Self>) {
            *self.ready.lock().unwrap_or_else(PoisonError::into_inner) = true;
            self.cvar.notify_one();
        }
    }

    let signal = Arc::new(ParkSignal {
        ready: Mutex::new(false),
        cvar: Condvar::new(),
    });
    let waker = Waker::from(Arc::clone(&signal));
    let mut cx = Context::from_waker(&waker);
    let mut fut = pin!(fut);

    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => {
                let mut ready = signal.ready.lock().unwrap_or_else(PoisonError::into_inner);
                while !*ready {
                    ready = signal
                        .cvar
                        .wait(ready)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                *ready = false;
            }
        }
    }
}

#[cfg(test)]
#[path = "portal_tests.rs"]
mod tests;
