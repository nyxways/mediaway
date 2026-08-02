//! Opaque device hotplug handle and its C ABI functions — poll + callback dual mode.
//!
//! Design: `adr/0002-callback-event-delivery.md`, as amended by its "Revision
//! (2026-07-31): lazy, thread-owned construction" section — authoritative over the
//! ADR's original §3/§8 concrete shapes. `DeviceHotplug` (`mediaway-device/src/hotplug.rs`)
//! stays sync-poll and unchanged; this module is the entire push mechanism, owned by
//! this crate (§1).
//!
//! **Construction is lazy and thread-owned, not eager.** [`mediaway_device_hotplug_open`]
//! only validates `kinds`; it never touches the real backend. The real
//! `Box<dyn DeviceHotplug>` is constructed directly on whichever thread first needs
//! it — the caller's own thread for pull mode ([`mediaway_device_hotplug_poll_event`]),
//! the bridging thread's own body for push mode
//! ([`mediaway_device_hotplug_register_callback`]) — and stays confined to that thread
//! for its lifetime. This is why `HotplugBackend`/`HotplugHandle` need no `Arc<Mutex<..>>`,
//! no `unsafe impl Send` on the backend, and no channel: the backend object simply never
//! crosses a thread boundary after it is built. See [`HotplugBackend`]'s doc for the
//! state machine.

use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use mediaway_device::{CaptureError, DeviceEvent, DeviceHotplug, DeviceKind};

use crate::status::MediawayDeviceStatus;
use crate::types::{MediawayDeviceEvent, MediawayDeviceEventKind, MediawayDeviceKind};

/// Poll interval for the mediaway-device-ffi-owned bridging thread while a callback is
/// registered. Hotplug events are rare and user-driven (plug/unplug, default-device
/// switch), not a per-frame stream — mirrors
/// `mediaway-device-windows/src/hotplug.rs::HOTPLUG_QUEUE_CAP`'s "generous headroom,
/// not a real backpressure concern" reasoning. Fixed for v1, not caller-configurable
/// (`adr/0002-callback-event-delivery.md` §3, § Deferred).
const HOTPLUG_CALLBACK_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// The callback function pointer type registered via
/// [`mediaway_device_hotplug_register_callback`] (`mediaway_device_hotplug_callback_fn`
/// in the C header).
///
/// `event` is **borrowed**, valid only for the duration of the call
/// (`adr/0002-callback-event-delivery.md` §2).
pub type MediawayDeviceHotplugCallbackFn =
    unsafe extern "C" fn(user_data: *mut c_void, event: *const MediawayDeviceEvent);

/// Opaque device hotplug handle (`mediaway_device_hotplug_t*` in the C header).
pub struct HotplugHandle {
    /// Shared, not a plain `bool`: the bridging thread must be able to poison the
    /// handle itself (a fatal backend `Err` or a caught panic while polling, §3) while
    /// the caller's thread independently reads/sets it on every other call — the same
    /// "poisoned" meaning as `adr/0001-capture-c-abi.md` §2/§8, extended to be safely
    /// observable from two threads for the first time in this crate.
    poisoned: Arc<AtomicBool>,
    backend: HotplugBackend,
}

/// The handle's construction/mode state — the ADR revision's central type. Exactly one
/// variant is live at a time; every transition happens on the thread that is about to
/// own the result.
enum HotplugBackend {
    /// [`mediaway_device_hotplug_open`] only validated `kinds` — nothing COM-side (or
    /// any other backend-side work) has happened yet.
    Idle {
        /// Preserved verbatim from `open()` (or restored by
        /// [`join_callback`]/[`unregister_callback`](mediaway_device_hotplug_unregister_callback)
        /// after a push-mode session ends) so any later transition, from any mode,
        /// knows what to (re)construct without the caller repeating it.
        kinds: Vec<DeviceKind>,
    },
    /// Constructed directly on whichever thread made the first `poll_event()` call.
    /// Thread-confined by convention from that point on — the same contract
    /// `adr/0001-capture-c-abi.md` §9 already documents for every other handle in this
    /// crate (moving between threads is fine; two threads touching it at once without
    /// external sync is a data race the caller must avoid, not something this crate
    /// defends against here).
    Pulling {
        /// Kept alongside `inner` (a deviation from the ADR revision's own sketch,
        /// which shows a bare `Pulling(Box<dyn DeviceHotplug>)`): `register_callback`
        /// switching mode while `Pulling` needs to know what to reconstruct for push
        /// mode after closing this object, and `HotplugHandle` itself carries no
        /// separate `kinds` field to fall back on. See this file's ADR-0002
        /// implementation-addendum for the full rationale.
        kinds: Vec<DeviceKind>,
        inner: Box<dyn DeviceHotplug>,
    },
    /// Owned exclusively by the bridging thread's own stack for its entire lifetime —
    /// constructed there, used there, closed there. The handle's own struct never
    /// holds the backend object itself while push mode is active.
    Pushing(CallbackBridge),
}

/// The bridging thread's join handle + stop flag, owned by [`HotplugHandle`] while a
/// callback is registered.
struct CallbackBridge {
    stop: Arc<AtomicBool>,
    thread: thread::JoinHandle<()>,
    /// Same deviation/rationale as [`HotplugBackend::Pulling`]'s `kinds` field: needed
    /// so [`join_callback`] can rebuild `HotplugBackend::Idle { kinds }` once the
    /// thread (which used its own moved copy to construct the backend, §3) is joined.
    kinds: Vec<DeviceKind>,
}

/// Wraps the caller's function pointer + opaque `user_data` so both can move into the
/// bridging thread (`thread::spawn` requires `Send + 'static`).
/// Neither an `extern "C" fn(...)` pointer nor a raw `*mut c_void` is `Send` by default
/// in Rust's type system, even though both are plain addresses at the machine level.
///
/// # Safety
///
/// `unsafe impl Send` is sound only because this crate never dereferences `user_data`
/// itself — it is opaque, caller-owned data threaded straight through to `callback` on
/// every invocation, unmodified. The **caller** is responsible for `user_data` being
/// safe to access from whatever unspecified thread `callback` actually runs on (§5) —
/// the same responsibility every callback-registration API surveyed in
/// `adr/0002-callback-event-delivery.md` § Context (libusb, `PortAudio`, `CoreAudio`)
/// already places on its own caller; this is not a new obligation invented for this
/// crate.
struct CallbackTarget {
    callback: MediawayDeviceHotplugCallbackFn,
    user_data: *mut c_void,
}

// SAFETY: see the struct-level doc comment above.
unsafe impl Send for CallbackTarget {}

/// Open a hotplug watcher for `kinds`.
///
/// **Lazy**: only validates `kinds` (mapping the raw `mediaway_device_kind_t` values to
/// [`DeviceKind`]) and stores them — the real backend is not constructed until the
/// first [`mediaway_device_hotplug_poll_event`] or
/// [`mediaway_device_hotplug_register_callback`] call touches this handle
/// (`adr/0002-callback-event-delivery.md`'s lazy-construction revision). One accepted
/// consequence, stated plainly: a hotplug event occurring in the gap between `open()`
/// returning and that first call could be missed — hotplug events are rare/user-driven,
/// not a stream a caller needs to catch from the very first instant `open()` returns.
///
/// # Safety
///
/// `kinds` must be valid for reads of `kinds_len` [`MediawayDeviceKind`] elements, or
/// null with `kinds_len == 0`. `out_hotplug` must be a valid, writable, non-null
/// out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_device_hotplug_open(
    kinds: *const MediawayDeviceKind,
    kinds_len: usize,
    out_hotplug: *mut *mut HotplugHandle,
) -> MediawayDeviceStatus {
    if out_hotplug.is_null() || (kinds.is_null() && kinds_len != 0) {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: `out_hotplug` is checked non-null above; caller guarantees it is
    // writable (function contract).
    unsafe { out_hotplug.write(std::ptr::null_mut()) };

    // SAFETY: caller guarantees `kinds` is valid for reads of `kinds_len` elements, or
    // null with `kinds_len == 0` (function contract).
    let kinds_slice: &[MediawayDeviceKind] = if kinds.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(kinds, kinds_len) }
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut rust_kinds = Vec::with_capacity(kinds_slice.len());
        for &kind in kinds_slice {
            let Some(kind) = c_kind_to_device_kind(kind) else {
                return Err(MediawayDeviceStatus::InvalidInput);
            };
            rust_kinds.push(kind);
        }
        Ok(rust_kinds)
    }));

    match result {
        Ok(Ok(kinds)) => {
            let handle = Box::new(HotplugHandle {
                poisoned: Arc::new(AtomicBool::new(false)),
                backend: HotplugBackend::Idle { kinds },
            });
            // SAFETY: `out_hotplug` is checked non-null above (function contract).
            unsafe { out_hotplug.write(Box::into_raw(handle)) };
            MediawayDeviceStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => MediawayDeviceStatus::InternalPanic,
    }
}

/// A raw `mediaway_device_kind_t` value this crate does not recognize as a real
/// [`DeviceKind`] (i.e. [`MediawayDeviceKind::Unknown`]) has no `open()`-time meaning —
/// `Unknown` only exists as a *decode-side* catch-all for a future Rust variant this
/// crate hasn't caught up with yet, never a value a caller should construct. Every
/// other value maps 1:1.
const fn c_kind_to_device_kind(kind: MediawayDeviceKind) -> Option<DeviceKind> {
    match kind {
        MediawayDeviceKind::Screen => Some(DeviceKind::Screen),
        MediawayDeviceKind::Window => Some(DeviceKind::Window),
        MediawayDeviceKind::Camera => Some(DeviceKind::Camera),
        MediawayDeviceKind::Microphone => Some(DeviceKind::Microphone),
        MediawayDeviceKind::Loopback => Some(DeviceKind::Loopback),
        MediawayDeviceKind::ProcessLoopback => Some(DeviceKind::ProcessLoopback),
        MediawayDeviceKind::Unknown => None,
    }
}

/// Local `#[cfg(windows)]`/`#[cfg(target_os = "linux")]` hotplug dispatch — mirrors
/// `video.rs::open_camera_capture`/`audio.rs::open_audio_capture`'s shape
/// (`adr/0001-capture-c-abi.md` §1). Called lazily, on whichever thread first needs a
/// live backend — the caller's own thread for pull mode ([`poll_event_impl`]), the
/// bridging thread's own body for push mode ([`bridging_loop`]) — never eagerly at
/// [`mediaway_device_hotplug_open`] time.
///
/// **Windows now dispatches to the real `WindowsDeviceHotplug`.** The earlier
/// `WindowsDeviceHotplug: Send` blocker (its `IMMDeviceEnumerator` field does not
/// implement `IAgileObject`, confirmed empirically —
/// `mediaway-device-windows::lib_tests::mmdevice_enumerator_does_not_implement_iagileobject_or_skip`)
/// no longer applies to this design: the returned `Box<dyn DeviceHotplug>` never
/// crosses a thread boundary — it is used exclusively by whichever thread called this
/// function, for the rest of its lifetime. See the module doc and
/// `adr/0002-callback-event-delivery.md`'s lazy-construction revision for the full
/// design, and this file's own ADR-0002 implementation addendum for the verification
/// that this now actually compiles and dispatches.
///
/// No `mediaway-device-linux` hotplug backend exists yet (only `mediaway-device-windows`
/// ships `WindowsDeviceHotplug`, `adr/0002-callback-event-delivery.md` § Context) — the
/// Linux arm stays `NoBackend`, not a dispatch to a real type; unrelated to this
/// function's lazy-construction redesign.
fn open_hotplug(kinds: &[DeviceKind]) -> Result<Box<dyn DeviceHotplug>, CaptureError> {
    #[cfg(windows)]
    {
        let hotplug = mediaway_device_windows::WindowsDeviceHotplug::open(kinds)?;
        Ok(Box::new(hotplug))
    }

    #[cfg(target_os = "linux")]
    {
        let _ = kinds;
        Err(CaptureError::NoBackend)
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = kinds;
        Err(CaptureError::NoBackend)
    }
}

/// Shared poll-mode logic: on [`HotplugBackend::Idle`], construct via `construct`
/// (production always passes [`open_hotplug`]; tests inject a mock — see
/// `hotplug_tests.rs`) and transition to [`HotplugBackend::Pulling`]; on
/// [`HotplugBackend::Pulling`], poll the already-live object directly (the "zero
/// regression after the first call" case the ADR revision calls out); on
/// [`HotplugBackend::Pushing`], mode exclusivity applies (§4) — `construct` is not
/// called.
fn poll_event_impl(
    handle: &mut HotplugHandle,
    construct: impl FnOnce(&[DeviceKind]) -> Result<Box<dyn DeviceHotplug>, CaptureError>,
) -> Result<Option<DeviceEvent>, MediawayDeviceStatus> {
    if matches!(handle.backend, HotplugBackend::Pushing(_)) {
        return Err(MediawayDeviceStatus::CallbackModeActive);
    }
    if let HotplugBackend::Idle { kinds } = &handle.backend {
        // clone: `construct` only needs to borrow `kinds` for the duration of this
        // call, but `handle.backend` is reassigned immediately after — cloning the
        // small `Vec<DeviceKind>` (`Copy` elements) avoids holding a live borrow of
        // `handle.backend` across that reassignment. Runs at most once per handle
        // (`Idle` -> `Pulling`), not a hot path.
        let kinds = kinds.clone();
        let inner = construct(&kinds).map_err(MediawayDeviceStatus::from)?;
        handle.backend = HotplugBackend::Pulling { kinds, inner };
    }
    match &mut handle.backend {
        HotplugBackend::Pulling { inner, .. } => {
            inner.poll_event().map_err(MediawayDeviceStatus::from)
        }
        // Unreachable: `Pushing` returned above, and the `Idle` branch above always
        // leaves `Pulling` behind (or returns early via `?`). Defensive fallback status,
        // not a panic — this crate does not panic outside tests (AGENTS.md).
        HotplugBackend::Idle { .. } | HotplugBackend::Pushing(_) => {
            Err(MediawayDeviceStatus::InternalPanic)
        }
    }
}

/// Register a push-mode callback for hotplug events.
///
/// Spawns a dedicated bridging thread owned by this crate that **constructs the real
/// backend itself, on itself** (`adr/0002-callback-event-delivery.md`'s
/// lazy-construction revision), then polls it at [`HOTPLUG_CALLBACK_POLL_INTERVAL`] and
/// invokes `callback` once per drained event (§3). From the caller's point of view
/// this is genuine push — no polling loop in their own code — but delivery has a
/// bounded added latency of up to one poll interval versus true OS-thread-direct
/// delivery: **not** zero-latency (§1).
///
/// # Thread-safety contract (§5)
///
/// - `callback` may be invoked from an unspecified, Mediaway-owned thread — never the
///   thread that called this function, and not necessarily the platform backend's own
///   raw OS-callback thread either. A binding must not assume any particular thread
///   identity, priority, or COM apartment state.
/// - `callback` **must not block**: it runs on the one bridging thread this handle
///   owns; blocking it delays every subsequent event and blocks
///   [`mediaway_device_hotplug_unregister_callback`]/[`mediaway_device_hotplug_close`]
///   for as long as it blocks (§4).
/// - `callback` **must not call back into any `mediaway_device_*` function on this same
///   handle synchronously.** `WindowsDeviceHotplug`'s own doc already states the
///   underlying constraint this propagates: MSDN documents that
///   `IMMNotificationClient` methods "must be nonblocking," and
///   `WindowsDeviceHotplug::close`'s own comment notes calling
///   `UnregisterEndpointNotificationCallback` from inside a live notification callback
///   would deadlock. Calling `mediaway_device_hotplug_close`/`unregister_callback` on
///   the same handle from inside `callback` is the direct C-ABI analog of that exact
///   deadlock and is forbidden for the same reason, transitively.
/// - `callback` **must not unwind/panic across the FFI boundary.** This crate's own
///   `catch_unwind` wrapping covers panics inside this crate's Rust code up to and
///   including the call into `callback` — it does **not**, and cannot, cover a foreign
///   exception (a C++ exception, a Go panic escaping a `cgo` export, a Swift/JNI
///   exception) unwinding *back* into this crate's Rust frames from inside the
///   caller's own callback body. This is the caller's responsibility to prevent (e.g.
///   catch and convert to an error code *before* returning from their callback).
///
/// Mutually exclusive with poll mode on the same handle (§4): returns
/// [`MediawayDeviceStatus::CallbackAlreadyRegistered`] if a callback is already
/// registered — call [`mediaway_device_hotplug_unregister_callback`] first to replace
/// it (no hidden extra thread-join cost versus an implicit replace, since replacing
/// would still have to join the old thread first internally).
///
/// Calling this while [`HotplugBackend::Pulling`] (the caller polled first, then
/// switched to push mode) closes the existing polled object first — safe by the same
/// thread-confinement convention as every other handle in this crate: the thread
/// calling this function now is the same one that owns the `Pulling` object.
///
/// # Safety
///
/// `hotplug` must be a live pointer returned by [`mediaway_device_hotplug_open`].
/// `callback` must be a valid, non-null function pointer safely callable for as long as
/// it stays registered. `user_data` is opaque, caller-owned data threaded through to
/// `callback` unmodified; the caller is responsible for it being safe to access from
/// whatever thread `callback` actually runs on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_device_hotplug_register_callback(
    hotplug: *mut HotplugHandle,
    callback: Option<MediawayDeviceHotplugCallbackFn>,
    user_data: *mut c_void,
) -> MediawayDeviceStatus {
    if hotplug.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    let Some(callback) = callback else {
        return MediawayDeviceStatus::InvalidArgument;
    };
    // SAFETY: caller guarantees `hotplug` is a valid, live handle pointer (function
    // contract).
    if unsafe { &*hotplug }.poisoned.load(Ordering::Relaxed) {
        return MediawayDeviceStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `hotplug` is a valid, live, unaliased handle
        // pointer for the duration of this call (function contract).
        let handle = unsafe { &mut *hotplug };
        let target = CallbackTarget {
            callback,
            user_data,
        };
        register_callback_impl(handle, target, open_hotplug)
    }));

    let Ok(status) = result else {
        // SAFETY: same pointer, re-derived fresh — the panicking closure's `&mut`
        // reference no longer exists after unwinding.
        unsafe { &*hotplug }.poisoned.store(true, Ordering::Relaxed);
        return MediawayDeviceStatus::InternalPanic;
    };
    status
}

/// Shared register-callback logic (`adr/0002-callback-event-delivery.md`'s
/// lazy-construction revision) — factored out so tests can inject a mock `construct`
/// instead of the real [`open_hotplug`] dispatch (`hotplug_tests.rs`).
fn register_callback_impl(
    handle: &mut HotplugHandle,
    target: CallbackTarget,
    construct: impl FnOnce(&[DeviceKind]) -> Result<Box<dyn DeviceHotplug>, CaptureError>
    + Send
    + 'static,
) -> MediawayDeviceStatus {
    if matches!(handle.backend, HotplugBackend::Pushing(_)) {
        return MediawayDeviceStatus::CallbackAlreadyRegistered;
    }
    if let HotplugBackend::Pulling { inner, .. } = &mut handle.backend {
        // Close the existing `Pulling` object first — safe by the same
        // thread-confinement convention as every other handle in this crate: this
        // function runs on the same thread that owns the `Pulling` object.
        let _ = inner.close();
    }
    let kinds = match &handle.backend {
        // clone: `handle.backend` is reassigned to `Pushing` below, so this needs its
        // own owned copy rather than a borrow held across that reassignment — same
        // reasoning as `poll_event_impl`'s `Idle` arm. Cheap: small `Vec<DeviceKind>`
        // of `Copy` elements.
        HotplugBackend::Idle { kinds } | HotplugBackend::Pulling { kinds, .. } => kinds.clone(),
        // Unreachable: `Pushing` returned above. Defensive fallback, not a panic.
        HotplugBackend::Pushing(_) => return MediawayDeviceStatus::CallbackAlreadyRegistered,
    };

    let stop = Arc::new(AtomicBool::new(false));
    // clone: Arc share — the bridging thread outlives this function call and needs its
    // own strong ref to `stop`, independent of the one kept below for `CallbackBridge`.
    let thread_stop = Arc::clone(&stop);
    // clone: Arc share — the bridging thread outlives this function call and needs its
    // own strong ref to the shared `poisoned` flag.
    let thread_poisoned = Arc::clone(&handle.poisoned);
    // clone: the bridging thread needs its own owned, `'static` copy of `kinds` to
    // construct the backend on itself; `CallbackBridge` (below) keeps a second copy to
    // restore `Idle { kinds }` once `unregister_callback` joins this thread — both
    // trace back to the same small `Vec<DeviceKind>` of `Copy` elements, cheap either
    // way.
    let thread_kinds = kinds.clone();
    let spawn_result = thread::Builder::new()
        .name("mediaway-device-hotplug-bridge".to_owned())
        .spawn(move || {
            bridging_loop(
                &thread_stop,
                &thread_poisoned,
                &thread_kinds,
                &target,
                construct,
            );
        });

    match spawn_result {
        Ok(thread) => {
            handle.backend = HotplugBackend::Pushing(CallbackBridge {
                stop,
                thread,
                kinds,
            });
            MediawayDeviceStatus::Ok
        }
        Err(_) => MediawayDeviceStatus::BackendFailure,
    }
}

/// The bridging thread body (`adr/0002-callback-event-delivery.md` §3, lazy-construction
/// revision). Constructs the real backend itself via `construct` before entering the
/// loop — a fatal construction error poisons the handle immediately, the same as any
/// other fatal condition below. Runs until `stop` is set
/// ([`mediaway_device_hotplug_unregister_callback`]/[`mediaway_device_hotplug_close`])
/// or a fatal condition (a real backend `Err`, or a caught panic) sets `poisoned` and
/// returns. Every loop iteration — the `poll_event()` call, building the FFI event, and
/// invoking the caller's callback — is wrapped in one `catch_unwind`: a panic anywhere
/// in that sequence poisons the handle and stops the thread rather than looping in a
/// broken state silently. Closes the backend object itself, on itself, as its last act
/// before returning — the object was never shared with any other thread.
fn bridging_loop(
    stop: &Arc<AtomicBool>,
    poisoned: &Arc<AtomicBool>,
    kinds: &[DeviceKind],
    target: &CallbackTarget,
    construct: impl FnOnce(&[DeviceKind]) -> Result<Box<dyn DeviceHotplug>, CaptureError>,
) {
    let construct_result = catch_unwind(AssertUnwindSafe(|| construct(kinds)));
    let Ok(Ok(mut inner)) = construct_result else {
        poisoned.store(true, Ordering::Relaxed);
        return;
    };

    while !stop.load(Ordering::Relaxed) {
        let outcome = catch_unwind(AssertUnwindSafe(|| poll_and_invoke(inner.as_mut(), target)));
        match outcome {
            Ok(PollOutcome::Delivered) => {}
            Ok(PollOutcome::Idle) => thread::sleep(HOTPLUG_CALLBACK_POLL_INTERVAL),
            Ok(PollOutcome::BackendError) | Err(_) => {
                // A normal (non-panic) `Err` from `poll_event()` is also treated as
                // fatal to this bridging session — there is no error-reporting channel
                // through the callback function pointer (`adr/0002-callback-event-delivery.md`
                // §3): the handle is poisoned and the next explicit call the caller
                // makes (`poll_event`, `unregister_callback`, `close`) observes the
                // real status then.
                poisoned.store(true, Ordering::Relaxed);
                break;
            }
        }
    }
    let _ = inner.close();
}

/// One bridging-thread iteration's outcome.
enum PollOutcome {
    /// An event was drained and (if representable, §6) delivered to the callback.
    Delivered,
    /// Nothing pending this poll.
    Idle,
    /// `poll_event()` returned a real backend `Err` — fatal to this session.
    BackendError,
}

/// Poll once and, if an event is pending and representable, invoke `target.callback`
/// with it. `inner` is exclusively owned by the calling (bridging) thread — no lock to
/// take or drop around the callback invocation, unlike the pre-revision design.
fn poll_and_invoke(inner: &mut dyn DeviceHotplug, target: &CallbackTarget) -> PollOutcome {
    let event = match inner.poll_event() {
        Ok(Some(event)) => event,
        Ok(None) => return PollOutcome::Idle,
        Err(_) => return PollOutcome::BackendError,
    };

    if let Some(ffi_event) = build_event(&event) {
        // SAFETY: `target.callback` is a valid, non-null function pointer supplied by
        // the caller at `mediaway_device_hotplug_register_callback` time (function
        // contract). `&ffi_event` is borrowed for the duration of this call only —
        // freed immediately below, matching `adr/0002-callback-event-delivery.md` §2's
        // callback-mode ownership contract.
        unsafe { (target.callback)(target.user_data, &raw const ffi_event) };
        free_device_id(ffi_event.device_id);
    }
    PollOutcome::Delivered
}

/// Unregister a previously registered callback and return the handle to poll mode.
///
/// **Blocks** — joins the bridging thread; can take up to
/// [`HOTPLUG_CALLBACK_POLL_INTERVAL`] plus the time any currently in-flight callback
/// invocation takes to return (`adr/0002-callback-event-delivery.md` §4). A callback
/// that blocks indefinitely (its own §5 contract violation) makes this call block
/// indefinitely too — a hang surfacing the caller's own violation, not a bug here.
/// **Idempotent**: a no-op returning `Ok` when no callback is registered, matching this
/// crate's existing "safe to call when already in the target state" convention
/// (`mediaway_camera_capture_close`/`mediaway_desktop_capture_close` on `NULL`).
///
/// Deliberately does **not** short-circuit on a poisoned handle (unlike every other
/// function in this module except [`mediaway_device_hotplug_close`]): its purpose is
/// exactly to join and reclaim a bridging thread that may itself be the thing that set
/// `poisoned` (the bridging loop's fatal-`Err`/panic path, §3) — refusing to run here
/// would leave the handle stuck in `Pushing` (and the dead thread's `JoinHandle`)
/// forever, the same "always safe to call, including on a poisoned handle" exemption
/// [`mediaway_device_hotplug_close`]/`mediaway_*_capture_close` already document.
///
/// # Safety
///
/// `hotplug` must be a live pointer returned by [`mediaway_device_hotplug_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_device_hotplug_unregister_callback(
    hotplug: *mut HotplugHandle,
) -> MediawayDeviceStatus {
    if hotplug.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `hotplug` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *hotplug };
    join_callback(handle)
}

/// Join `handle`'s bridging thread, if any, and return it to `Idle { kinds }` — the
/// implicit-unregister step shared by [`mediaway_device_hotplug_unregister_callback`]
/// and a callback-mode [`mediaway_device_hotplug_close`]
/// (`adr/0002-callback-event-delivery.md` §4). A no-op returning `Ok` when the handle
/// is already `Idle`/`Pulling` (nothing to join).
fn join_callback(handle: &mut HotplugHandle) -> MediawayDeviceStatus {
    if !matches!(handle.backend, HotplugBackend::Pushing(_)) {
        return MediawayDeviceStatus::Ok;
    }
    // `kinds: Vec::new()` is a placeholder immediately overwritten below — only ever
    // observable if a panic occurred between this line and the write, which cannot
    // happen (no fallible code runs in between).
    let HotplugBackend::Pushing(bridge) = std::mem::replace(
        &mut handle.backend,
        HotplugBackend::Idle { kinds: Vec::new() },
    ) else {
        // Unreachable: the `matches!` check above already confirmed `Pushing`.
        // Defensive fallback, not a panic.
        return MediawayDeviceStatus::Ok;
    };
    let CallbackBridge {
        stop,
        thread,
        kinds,
    } = bridge;
    stop.store(true, Ordering::Relaxed);
    if thread.join().is_ok() {
        handle.backend = HotplugBackend::Idle { kinds };
        MediawayDeviceStatus::Ok
    } else {
        // The bridging thread itself panicked despite its own per-iteration
        // catch_unwind (not expected in practice) — poison the handle rather than
        // silently reporting success. `kinds` is still recovered so the handle stays
        // in a well-formed (if poisoned) state.
        handle.poisoned.store(true, Ordering::Relaxed);
        handle.backend = HotplugBackend::Idle { kinds };
        MediawayDeviceStatus::InternalPanic
    }
}

/// Pull the next hotplug event if ready. Only valid in poll mode.
///
/// On [`HotplugBackend::Idle`], constructs the real backend on **this** (the calling)
/// thread first (`adr/0002-callback-event-delivery.md`'s lazy-construction revision) —
/// a one-time cost on first use, not per call. Returns
/// [`MediawayDeviceStatus::CallbackModeActive`] and drains nothing while a callback is
/// registered (§4): poll and callback delivery both ultimately drain the same
/// single-consumer queue, and mixing them would nondeterministically split delivery of
/// one logical event stream between two consumers.
///
/// `*out_has_event == false` is a valid "no event yet" result, not an error;
/// `*out_event` is only meaningful when `*out_has_event == true`, and must then be
/// released with [`mediaway_device_hotplug_event_free`].
///
/// # Safety
///
/// `hotplug` must be a live pointer returned by [`mediaway_device_hotplug_open`].
/// `out_event` must be a valid, writable pointer to a [`MediawayDeviceEvent`].
/// `out_has_event` must be a valid, writable `bool` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_device_hotplug_poll_event(
    hotplug: *mut HotplugHandle,
    out_event: *mut MediawayDeviceEvent,
    out_has_event: *mut bool,
) -> MediawayDeviceStatus {
    if hotplug.is_null() || out_event.is_null() || out_has_event.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `hotplug` is a valid, live handle pointer (function
    // contract).
    if unsafe { &*hotplug }.poisoned.load(Ordering::Relaxed) {
        return MediawayDeviceStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `hotplug` is a valid, live, unaliased handle
        // pointer for the duration of this call (function contract).
        let handle = unsafe { &mut *hotplug };
        poll_event_impl(handle, open_hotplug)
    }));

    match result {
        // SAFETY: `out_event`/`out_has_event` are checked non-null above (function
        // contract).
        Ok(Ok(maybe_event)) => unsafe {
            write_polled_event(maybe_event.as_ref(), out_event, out_has_event)
        },
        Ok(Err(status)) => status,
        Err(_) => {
            // SAFETY: same pointer, re-derived fresh — the panicking closure's `&mut`
            // reference no longer exists after unwinding.
            unsafe { &*hotplug }.poisoned.store(true, Ordering::Relaxed);
            MediawayDeviceStatus::InternalPanic
        }
    }
}

/// Write `maybe_event` (`None` = idle, or a real event this crate cannot represent —
/// §6's defensive drop) to `out_event`/`out_has_event`.
///
/// # Safety
///
/// `out_event`/`out_has_event` must be valid, writable, non-null pointers.
unsafe fn write_polled_event(
    maybe_event: Option<&DeviceEvent>,
    out_event: *mut MediawayDeviceEvent,
    out_has_event: *mut bool,
) -> MediawayDeviceStatus {
    let ffi_event = maybe_event.and_then(build_event);
    let Some(ffi_event) = ffi_event else {
        // SAFETY: caller guarantees `out_has_event` is valid/writable (function
        // contract).
        unsafe { out_has_event.write(false) };
        return MediawayDeviceStatus::Ok;
    };
    // SAFETY: caller guarantees `out_event`/`out_has_event` are valid/writable
    // (function contract).
    unsafe {
        out_event.write(ffi_event);
        out_has_event.write(true);
    }
    MediawayDeviceStatus::Ok
}

/// Close a hotplug watcher, freeing its handle.
///
/// If a callback is registered, performs the same join as
/// [`mediaway_device_hotplug_unregister_callback`] first (implicit unregister,
/// `adr/0002-callback-event-delivery.md` §4), then closes the underlying
/// `DeviceHotplug` if one was ever constructed (a poll-mode handle that never made a
/// single `poll_event()` call has nothing to close) and frees the handle — one call
/// does the full teardown. Always safe to call, including on a poisoned handle (unlike
/// every other function in this module, `close` does **not** short-circuit on
/// `poisoned`, `adr/0001-capture-c-abi.md` §8), or with `hotplug == NULL` (a no-op,
/// reported as `Ok`).
///
/// # Safety
///
/// `hotplug` must be null or a pointer previously returned by
/// [`mediaway_device_hotplug_open`] and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_device_hotplug_close(
    hotplug: *mut HotplugHandle,
) -> MediawayDeviceStatus {
    if hotplug.is_null() {
        return MediawayDeviceStatus::Ok;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `hotplug` is a valid, not-yet-freed handle pointer
        // (function contract).
        let mut handle = unsafe { Box::from_raw(hotplug) };
        let join_status = join_callback(&mut handle);
        let close_status = match &mut handle.backend {
            HotplugBackend::Pulling { inner, .. } => {
                inner.close().map_err(MediawayDeviceStatus::from)
            }
            HotplugBackend::Idle { .. } | HotplugBackend::Pushing(_) => Ok(()),
        };
        (join_status, close_status)
    }));

    match result {
        Ok((MediawayDeviceStatus::Ok, Ok(()))) => MediawayDeviceStatus::Ok,
        Ok((join_status, Ok(()))) => join_status,
        Ok((_, Err(status))) => status,
        Err(_) => MediawayDeviceStatus::InternalPanic,
    }
}

/// Free an event returned by [`mediaway_device_hotplug_poll_event`].
///
/// Must **not** be called on the borrowed event a registered callback receives — that
/// event is freed automatically by the bridging thread immediately after the callback
/// returns (`adr/0002-callback-event-delivery.md` §2).
///
/// Nulls the event's `device_id` pointer afterward, making a double-free a visible
/// no-op instead of undefined behavior.
///
/// # Safety
///
/// `event` must be null or a valid, writable pointer to a [`MediawayDeviceEvent`] whose
/// `device_id` was produced by [`mediaway_device_hotplug_poll_event`] and not already
/// freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_device_hotplug_event_free(event: *mut MediawayDeviceEvent) {
    if event.is_null() {
        return;
    }
    // SAFETY: caller guarantees `event` is a valid, writable pointer (function
    // contract).
    let event = unsafe { &mut *event };
    free_device_id(event.device_id);
    event.device_id = std::ptr::null_mut();
}

/// Build the C-ABI event representation for `event`, allocating an owned,
/// NUL-terminated device-id string (`adr/0002-callback-event-delivery.md` §6). `None`
/// id, or a defensive `CString::new` failure (an embedded NUL — practically
/// unreachable), both produce a `NULL` `device_id` rather than dropping the whole
/// event.
///
/// `DeviceEvent` is `#[non_exhaustive]`; a future variant this crate does not know
/// about yet has no `mediaway_device_event_kind_t` value to translate into, so it is
/// defensively dropped (`None`) rather than fabricating one — not reachable today
/// (every real backend only emits the four variants matched below).
fn build_event(event: &DeviceEvent) -> Option<MediawayDeviceEvent> {
    let (event_kind, device_kind, id) = match event {
        DeviceEvent::Added { id, kind } => (MediawayDeviceEventKind::Added, *kind, Some(id)),
        DeviceEvent::Removed { id, kind } => (MediawayDeviceEventKind::Removed, *kind, Some(id)),
        DeviceEvent::DefaultChanged { kind, id } => {
            (MediawayDeviceEventKind::DefaultChanged, *kind, id.as_ref())
        }
        DeviceEvent::StateChanged { id, kind } => {
            (MediawayDeviceEventKind::StateChanged, *kind, Some(id))
        }
        _ => return None,
    };
    let device_id = id
        .and_then(|id| CString::new(id.to_string()).ok())
        .map_or(std::ptr::null_mut(), CString::into_raw);
    Some(MediawayDeviceEvent {
        event_kind,
        device_kind: device_kind.into(),
        device_id,
    })
}

/// Reclaim and drop a `device_id` string previously produced by [`build_event`] (via
/// [`CString::into_raw`]). A `NULL` pointer is a no-op.
fn free_device_id(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: `ptr` was produced by `CString::into_raw` in `build_event` and is
    // reclaimed at most once per event (`mediaway_device_hotplug_event_free` nulls it
    // out; the bridging thread's own borrowed-event path calls this exactly once per
    // delivered event, § Safety contract).
    drop(unsafe { CString::from_raw(ptr) });
}

#[cfg(test)]
#[path = "hotplug_tests.rs"]
mod tests;
