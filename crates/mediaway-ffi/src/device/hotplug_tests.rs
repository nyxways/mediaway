//! Unit tests for `hotplug.rs` (sibling of the implementation).
//!
//! No real backend is exercised here — these tests target the C ABI mechanics (mode
//! exclusivity, lazy construction, the bridging thread, panic/poison handling, event
//! building) against a test-only [`MockHotplug`]/[`PanickingHotplug`]. Since
//! construction is now lazy and thread-owned (`adr/0002-callback-event-delivery.md`'s
//! revision), the real [`open_hotplug`] dispatch cannot simply be swapped out at the
//! public C ABI boundary — [`poll_event_impl`]/[`register_callback_impl`] are called
//! directly (private, same-crate) with an injected mock `construct` closure wherever a
//! test needs control over what gets constructed. Tests that only need an
//! already-constructed (`Pulling`) backend still go through the public
//! `mediaway_device_hotplug_*` functions via [`handle_with`], since those never touch
//! `construct` at all.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test modules may unwrap/panic-on-unexpected-variant; PanickingHotplug's \
              panic! is the deliberate test fixture for this module's catch_unwind path; \
              print_stderr matches this crate's `_or_skip` hardware-test convention \
              (mediaway-device-windows::lib_tests)"
)]

use std::collections::VecDeque;
use std::ffi::CStr;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use mediaway_device::DeviceId;

use super::*;

/// A [`DeviceHotplug`] backend that replays a fixed script of `poll_event` results,
/// then idles (`Ok(None)`) forever once the script is exhausted.
struct MockHotplug {
    script: VecDeque<Result<Option<DeviceEvent>, CaptureError>>,
    closed: Arc<AtomicBool>,
}

impl DeviceHotplug for MockHotplug {
    fn poll_event(&mut self) -> Result<Option<DeviceEvent>, CaptureError> {
        self.script.pop_front().unwrap_or(Ok(None))
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        self.closed.store(true, Ordering::Relaxed);
        Ok(())
    }
}

/// A [`DeviceHotplug`] backend whose `poll_event` always panics — exercises this
/// module's `catch_unwind`/poisoned-handle path without needing a real backend fault.
struct PanickingHotplug;

impl DeviceHotplug for PanickingHotplug {
    fn poll_event(&mut self) -> Result<Option<DeviceEvent>, CaptureError> {
        panic!("mock backend panic");
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        Ok(())
    }
}

/// Build a [`HotplugHandle`] already in [`HotplugBackend::Pulling`] around `backend`,
/// bypassing `open_hotplug`/lazy construction entirely, and leak it into a raw pointer
/// the way [`mediaway_device_hotplug_open`] followed by a first `poll_event` would.
/// Valid for tests that only need an already-constructed backend — `kinds` is an
/// arbitrary placeholder since nothing here reconstructs from it.
fn handle_with(backend: impl DeviceHotplug + 'static) -> *mut HotplugHandle {
    Box::into_raw(Box::new(HotplugHandle {
        poisoned: Arc::new(AtomicBool::new(false)),
        backend: HotplugBackend::Pulling {
            kinds: vec![DeviceKind::Microphone],
            inner: Box::new(backend),
        },
    }))
}

/// Build a [`HotplugHandle`] in [`HotplugBackend::Idle`] — the state
/// [`mediaway_device_hotplug_open`] itself produces — for tests exercising the lazy
/// construction transition directly via [`poll_event_impl`]/[`register_callback_impl`].
fn handle_idle(kinds: Vec<DeviceKind>) -> *mut HotplugHandle {
    Box::into_raw(Box::new(HotplugHandle {
        poisoned: Arc::new(AtomicBool::new(false)),
        backend: HotplugBackend::Idle { kinds },
    }))
}

/// A one-shot mock `construct` closure for [`poll_event_impl`]/[`register_callback_impl`]:
/// returns a fresh [`MockHotplug`] replaying `script`, recording its `close()` via
/// `closed`. Stands in for the real `open_hotplug` dispatch this same call site uses in
/// production.
fn mock_constructor(
    script: impl Into<VecDeque<Result<Option<DeviceEvent>, CaptureError>>>,
    closed: Arc<AtomicBool>,
) -> impl FnOnce(&[DeviceKind]) -> Result<Box<dyn DeviceHotplug>, CaptureError> + Send + 'static {
    let script = script.into();
    move |_kinds| Ok(Box::new(MockHotplug { script, closed }) as Box<dyn DeviceHotplug>)
}

/// A `construct` closure that panics if actually invoked — used to assert that mode
/// exclusivity (§4) truly short-circuits *before* ever touching `construct`, not just
/// that the returned status happens to be right.
fn construct_must_not_be_called()
-> impl FnOnce(&[DeviceKind]) -> Result<Box<dyn DeviceHotplug>, CaptureError> {
    |_kinds| panic!("construct should not be called when mode exclusivity short-circuits")
}

fn added(tag: &str) -> DeviceEvent {
    DeviceEvent::Added {
        id: DeviceId::from_wasapi_endpoint_id(tag),
        kind: DeviceKind::Microphone,
    }
}

/// Spin-wait (bounded) for `check` to become true — used instead of a fixed `sleep` to
/// observe the bridging thread's effects without a flaky race against
/// [`HOTPLUG_CALLBACK_POLL_INTERVAL`].
fn wait_until(mut check: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        if check() {
            return true;
        }
        if start.elapsed() > timeout {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

// ── c_kind_to_device_kind / build_event ─────────────────────────────────────────────

#[test]
fn c_kind_to_device_kind_maps_every_known_variant() {
    assert_eq!(
        c_kind_to_device_kind(MediawayDeviceKind::Screen),
        Some(DeviceKind::Screen)
    );
    assert_eq!(
        c_kind_to_device_kind(MediawayDeviceKind::Window),
        Some(DeviceKind::Window)
    );
    assert_eq!(
        c_kind_to_device_kind(MediawayDeviceKind::Camera),
        Some(DeviceKind::Camera)
    );
    assert_eq!(
        c_kind_to_device_kind(MediawayDeviceKind::Microphone),
        Some(DeviceKind::Microphone)
    );
    assert_eq!(
        c_kind_to_device_kind(MediawayDeviceKind::Loopback),
        Some(DeviceKind::Loopback)
    );
    assert_eq!(
        c_kind_to_device_kind(MediawayDeviceKind::ProcessLoopback),
        Some(DeviceKind::ProcessLoopback)
    );
    assert_eq!(c_kind_to_device_kind(MediawayDeviceKind::Unknown), None);
}

fn device_id_string(event: &MediawayDeviceEvent) -> Option<String> {
    if event.device_id.is_null() {
        return None;
    }
    // SAFETY: test-only read of a `build_event`-produced C string, freed by the caller
    // via `free_device_id` right after.
    Some(
        unsafe { CStr::from_ptr(event.device_id) }
            .to_string_lossy()
            .into_owned(),
    )
}

#[test]
fn build_event_added_carries_device_id() {
    let event = DeviceEvent::Added {
        id: DeviceId::from_wasapi_endpoint_id("mic-1"),
        kind: DeviceKind::Microphone,
    };
    let ffi_event = build_event(&event).expect("known variant");
    assert_eq!(ffi_event.event_kind, MediawayDeviceEventKind::Added);
    assert_eq!(ffi_event.device_kind, MediawayDeviceKind::Microphone);
    assert_eq!(
        device_id_string(&ffi_event).as_deref(),
        Some("wasapi:mic-1")
    );
    free_device_id(ffi_event.device_id);
}

#[test]
fn build_event_default_changed_with_no_id_is_null() {
    let event = DeviceEvent::DefaultChanged {
        kind: DeviceKind::Loopback,
        id: None,
    };
    let ffi_event = build_event(&event).expect("known variant");
    assert_eq!(
        ffi_event.event_kind,
        MediawayDeviceEventKind::DefaultChanged
    );
    assert_eq!(ffi_event.device_kind, MediawayDeviceKind::Loopback);
    assert!(ffi_event.device_id.is_null());
}

#[test]
fn build_event_removed_and_state_changed_map_kinds() {
    let removed = DeviceEvent::Removed {
        id: DeviceId::from_wasapi_endpoint_id("mic-2"),
        kind: DeviceKind::Microphone,
    };
    let ffi_removed = build_event(&removed).expect("known variant");
    assert_eq!(ffi_removed.event_kind, MediawayDeviceEventKind::Removed);
    free_device_id(ffi_removed.device_id);

    let state_changed = DeviceEvent::StateChanged {
        id: DeviceId::from_wasapi_endpoint_id("mic-3"),
        kind: DeviceKind::Microphone,
    };
    let ffi_state_changed = build_event(&state_changed).expect("known variant");
    assert_eq!(
        ffi_state_changed.event_kind,
        MediawayDeviceEventKind::StateChanged
    );
    free_device_id(ffi_state_changed.device_id);
}

// ── mediaway_device_hotplug_open ─────────────────────────────────────────────────────

#[test]
fn open_rejects_null_out_param() {
    let kinds = [MediawayDeviceKind::Microphone];
    let status =
        unsafe { mediaway_device_hotplug_open(kinds.as_ptr(), kinds.len(), std::ptr::null_mut()) };
    assert_eq!(status, MediawayDeviceStatus::InvalidArgument);
}

#[test]
fn open_rejects_unknown_kind_as_invalid_input() {
    let kinds = [MediawayDeviceKind::Unknown];
    let mut out: *mut HotplugHandle = std::ptr::null_mut();
    let status = unsafe { mediaway_device_hotplug_open(kinds.as_ptr(), kinds.len(), &raw mut out) };
    assert_eq!(status, MediawayDeviceStatus::InvalidInput);
    assert!(out.is_null());
}

#[test]
fn open_with_valid_kind_succeeds_and_stays_idle() {
    // Construction is lazy (`adr/0002-callback-event-delivery.md`'s revision): `open()`
    // only validates `kinds` and stores them — it never touches the real backend, so
    // this succeeds deterministically on every platform, with or without a real
    // backend compiled in. The first `poll_event`/`register_callback` call is what
    // actually attempts construction (see `mode_switch_*`/`bridging_thread_*` tests
    // below, which inject a mock constructor for exactly that step).
    let kinds = [MediawayDeviceKind::Microphone];
    let mut out: *mut HotplugHandle = std::ptr::null_mut();
    let status = unsafe { mediaway_device_hotplug_open(kinds.as_ptr(), kinds.len(), &raw mut out) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
    assert!(!out.is_null());
    // SAFETY: `out` was just returned by `mediaway_device_hotplug_open` above.
    assert!(matches!(
        unsafe { &*out }.backend,
        HotplugBackend::Idle { .. }
    ));
    unsafe { mediaway_device_hotplug_close(out) };
}

/// Real-hardware check that the Windows dispatch actually reaches
/// `WindowsDeviceHotplug` now (`open_hotplug`, `adr/0002-callback-event-delivery.md`'s
/// implementation addendum) instead of the old unconditional `NoBackend`, **and** that
/// `close()` on a successfully-constructed real handle no longer crashes.
///
/// This test used to leak the handle (`std::mem::forget`) instead of calling `close()`,
/// because `mediaway_device::windows::WindowsDeviceHotplug::close()` reliably crashed the
/// whole process with `STATUS_ACCESS_VIOLATION` — root-caused (via a real SEH exception
/// filter pinpointing the fault inside
/// `IMMDeviceEnumerator::UnregisterEndpointNotificationCallback`) to `open()` calling
/// `CoUninitialize()` on its own calling thread *before returning*, leaving the stored
/// `enumerator`/`client` referencing a torn-down COM apartment for `close()` to later use.
/// Fixed in `mediaway-device-windows/src/hotplug.rs` by having `HotplugSession` itself own
/// the `ComGuard`, keeping the apartment alive from `open()` through `close()`'s own
/// teardown call — see that file's type-level doc for the full write-up. `close()` is
/// exercised for real below now that the fix is in place.
///
/// Soft-skips (does not fail the suite) on any status *other than* `NoBackend` besides
/// `Ok` — those indicate a real but environment-specific condition (no default
/// microphone/loopback endpoint, COM failure, access denied), not a dispatch-wiring
/// regression. `NoBackend` specifically is a **hard failure**: that is exactly the
/// regression this test exists to catch (the Windows arm silently falling back to "no
/// backend compiled in" again).
#[cfg(windows)]
#[test]
fn open_hotplug_real_windows_backend_wires_through_or_skip() {
    let kinds = [MediawayDeviceKind::Microphone, MediawayDeviceKind::Loopback];
    let mut out: *mut HotplugHandle = std::ptr::null_mut();
    let status = unsafe { mediaway_device_hotplug_open(kinds.as_ptr(), kinds.len(), &raw mut out) };
    assert_eq!(
        status,
        MediawayDeviceStatus::Ok,
        "open() itself is lazy and never touches the backend"
    );
    assert!(!out.is_null());

    let mut event = MediawayDeviceEvent {
        event_kind: MediawayDeviceEventKind::Added,
        device_kind: MediawayDeviceKind::Unknown,
        device_id: std::ptr::null_mut(),
    };
    let mut has_event = false;
    let status =
        unsafe { mediaway_device_hotplug_poll_event(out, &raw mut event, &raw mut has_event) };

    if status == MediawayDeviceStatus::NoBackend {
        unsafe { mediaway_device_hotplug_close(out) };
        panic!(
            "real Windows hotplug dispatch regressed to NoBackend — open_hotplug is \
             supposed to reach WindowsDeviceHotplug now"
        );
    }
    if status == MediawayDeviceStatus::Ok {
        eprintln!("real WindowsDeviceHotplug construction succeeded; has_event={has_event}");
        if has_event {
            unsafe { mediaway_device_hotplug_event_free(&raw mut event) };
        }
    } else {
        eprintln!(
            "skip: real backend construction reported {status:?} on this machine (an \
             environment condition — e.g. no default microphone/loopback endpoint, COM \
             failure — not a dispatch-wiring regression)"
        );
    }
    // Exercised for real in every branch above (including the environment-condition one —
    // `close()` on a handle that never left `Idle` is also a real, meaningful path to
    // check). This is the exact call that used to crash the process.
    let close_status = unsafe { mediaway_device_hotplug_close(out) };
    assert_eq!(
        close_status,
        MediawayDeviceStatus::Ok,
        "close() should not fail, let alone crash"
    );
}

// ── poll mode ─────────────────────────────────────────────────────────────────────

#[test]
fn poll_event_drains_script_in_order_then_reports_idle() {
    let closed = Arc::new(AtomicBool::new(false));
    let handle = handle_with(MockHotplug {
        script: VecDeque::from([Ok(Some(added("a"))), Ok(None)]),
        closed: Arc::clone(&closed),
    });

    let mut event = MediawayDeviceEvent {
        event_kind: MediawayDeviceEventKind::Added,
        device_kind: MediawayDeviceKind::Unknown,
        device_id: std::ptr::null_mut(),
    };
    let mut has_event = false;

    let status =
        unsafe { mediaway_device_hotplug_poll_event(handle, &raw mut event, &raw mut has_event) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
    assert!(has_event);
    assert_eq!(device_id_string(&event).as_deref(), Some("wasapi:a"));
    unsafe { mediaway_device_hotplug_event_free(&raw mut event) };
    assert!(event.device_id.is_null());

    let status =
        unsafe { mediaway_device_hotplug_poll_event(handle, &raw mut event, &raw mut has_event) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
    assert!(!has_event);

    let status = unsafe { mediaway_device_hotplug_close(handle) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
    assert!(closed.load(Ordering::Relaxed));
}

#[test]
fn poll_event_surfaces_backend_error_as_status() {
    let handle = handle_with(MockHotplug {
        script: VecDeque::from([Err(CaptureError::Backend)]),
        closed: Arc::new(AtomicBool::new(false)),
    });

    let mut event = MediawayDeviceEvent {
        event_kind: MediawayDeviceEventKind::Added,
        device_kind: MediawayDeviceKind::Unknown,
        device_id: std::ptr::null_mut(),
    };
    let mut has_event = false;
    let status =
        unsafe { mediaway_device_hotplug_poll_event(handle, &raw mut event, &raw mut has_event) };
    assert_eq!(status, MediawayDeviceStatus::BackendFailure);

    unsafe { mediaway_device_hotplug_close(handle) };
}

// ── lazy construction ────────────────────────────────────────────────────────────────

#[test]
fn idle_handle_does_not_construct_until_first_touch() {
    let construction_count = Arc::new(AtomicUsize::new(0));
    let handle_ptr = handle_idle(vec![DeviceKind::Microphone]);
    // SAFETY: `handle_ptr` was just built above and is not shared with any other
    // thread.
    let handle = unsafe { &mut *handle_ptr };

    assert_eq!(construction_count.load(Ordering::Relaxed), 0);

    // clone: the constructor closure needs its own strong ref, independent of the one
    // this test keeps to assert on afterward.
    let counting_construct = {
        let construction_count = Arc::clone(&construction_count);
        move |_: &[DeviceKind]| {
            construction_count.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(MockHotplug {
                script: VecDeque::new(),
                closed: Arc::new(AtomicBool::new(false)),
            }) as Box<dyn DeviceHotplug>)
        }
    };

    let result = poll_event_impl(handle, counting_construct);
    assert_eq!(result, Ok(None));
    assert_eq!(
        construction_count.load(Ordering::Relaxed),
        1,
        "backend must be constructed exactly once, on the first poll_event"
    );
    assert!(matches!(handle.backend, HotplugBackend::Pulling { .. }));

    unsafe { mediaway_device_hotplug_close(handle_ptr) };
}

#[test]
fn poll_event_poisons_handle_on_panic_but_close_still_succeeds() {
    let handle = handle_with(PanickingHotplug);

    let mut event = MediawayDeviceEvent {
        event_kind: MediawayDeviceEventKind::Added,
        device_kind: MediawayDeviceKind::Unknown,
        device_id: std::ptr::null_mut(),
    };
    let mut has_event = false;

    let status =
        unsafe { mediaway_device_hotplug_poll_event(handle, &raw mut event, &raw mut has_event) };
    assert_eq!(status, MediawayDeviceStatus::InternalPanic);

    let status =
        unsafe { mediaway_device_hotplug_poll_event(handle, &raw mut event, &raw mut has_event) };
    assert_eq!(status, MediawayDeviceStatus::HandlePoisoned);

    // `close` never short-circuits on `poisoned` (`adr/0001-capture-c-abi.md` §8).
    let status = unsafe { mediaway_device_hotplug_close(handle) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
}

// ── callback mode / mode exclusivity ─────────────────────────────────────────────────

/// Records every event delivered to [`record_callback`], plus the raw
/// `device_id` string (copied out — the callback receives a *borrowed* pointer,
/// `adr/0002-callback-event-delivery.md` §2).
#[derive(Default)]
struct CallbackLog {
    events: Mutex<Vec<(MediawayDeviceEventKind, MediawayDeviceKind, Option<String>)>>,
}

unsafe extern "C" fn record_callback(user_data: *mut c_void, event: *const MediawayDeviceEvent) {
    // SAFETY: test-controlled — `user_data` points to a live `CallbackLog` for the
    // duration of the test, and `event` is valid for this call only, matching the real
    // callback contract (`adr/0002-callback-event-delivery.md` §2).
    let log = unsafe { &*user_data.cast::<CallbackLog>() };
    let event = unsafe { &*event };
    let id = device_id_string(event);
    log.events
        .lock()
        .expect("test mutex")
        .push((event.event_kind, event.device_kind, id));
}

#[test]
fn register_callback_delivers_events_and_mode_is_exclusive() {
    let handle_ptr = handle_idle(vec![DeviceKind::Microphone]);
    // SAFETY: `handle_ptr` was just built above and is not shared with any other
    // thread.
    let handle = unsafe { &mut *handle_ptr };

    let log = CallbackLog::default();
    let user_data: *mut c_void = std::ptr::from_ref(&log).cast_mut().cast();
    let construct = mock_constructor(
        VecDeque::from([Ok(Some(added("cb-a"))), Ok(Some(added("cb-b")))]),
        Arc::new(AtomicBool::new(false)),
    );

    let status = register_callback_impl(
        handle,
        CallbackTarget {
            callback: record_callback,
            user_data,
        },
        construct,
    );
    assert_eq!(status, MediawayDeviceStatus::Ok);
    assert!(matches!(handle.backend, HotplugBackend::Pushing(_)));

    // Registering twice is rejected (§4) — does not replace the existing callback, and
    // must not even call `construct`.
    let status = register_callback_impl(
        handle,
        CallbackTarget {
            callback: record_callback,
            user_data,
        },
        construct_must_not_be_called(),
    );
    assert_eq!(status, MediawayDeviceStatus::CallbackAlreadyRegistered);

    // `poll_event` while a callback is registered drains nothing (§4) and must not
    // even call `construct`.
    let result = poll_event_impl(handle, construct_must_not_be_called());
    assert_eq!(result, Err(MediawayDeviceStatus::CallbackModeActive));

    let delivered = wait_until(
        || log.events.lock().expect("test mutex").len() >= 2,
        Duration::from_secs(2),
    );
    assert!(delivered, "callback did not receive both scripted events");

    let status = join_callback(handle);
    assert_eq!(status, MediawayDeviceStatus::Ok);
    assert!(matches!(handle.backend, HotplugBackend::Idle { .. }));

    // Idempotent.
    let status = join_callback(handle);
    assert_eq!(status, MediawayDeviceStatus::Ok);

    let events = log.events.lock().expect("test mutex").clone();
    assert_eq!(
        events,
        vec![
            (
                MediawayDeviceEventKind::Added,
                MediawayDeviceKind::Microphone,
                Some("wasapi:cb-a".to_owned())
            ),
            (
                MediawayDeviceEventKind::Added,
                MediawayDeviceKind::Microphone,
                Some("wasapi:cb-b".to_owned())
            ),
        ]
    );

    // Poll mode works again once the callback is unregistered — constructs a fresh
    // mock (the previous one was closed inside the bridging thread).
    let result = poll_event_impl(
        handle,
        mock_constructor(VecDeque::new(), Arc::new(AtomicBool::new(false))),
    );
    assert_eq!(result, Ok(None));

    unsafe { mediaway_device_hotplug_close(handle_ptr) };
}

#[test]
fn register_callback_rejects_null_callback() {
    let handle = handle_with(MockHotplug {
        script: VecDeque::new(),
        closed: Arc::new(AtomicBool::new(false)),
    });
    let status =
        unsafe { mediaway_device_hotplug_register_callback(handle, None, std::ptr::null_mut()) };
    assert_eq!(status, MediawayDeviceStatus::InvalidArgument);
    unsafe { mediaway_device_hotplug_close(handle) };
}

#[test]
fn bridging_thread_backend_error_poisons_handle() {
    let handle_ptr = handle_idle(vec![DeviceKind::Microphone]);
    // SAFETY: `handle_ptr` was just built above and is not shared with any other
    // thread.
    let handle = unsafe { &mut *handle_ptr };
    let log = CallbackLog::default();
    let user_data: *mut c_void = std::ptr::from_ref(&log).cast_mut().cast();
    let construct = mock_constructor(
        VecDeque::from([Err(CaptureError::Backend)]),
        Arc::new(AtomicBool::new(false)),
    );

    let status = register_callback_impl(
        handle,
        CallbackTarget {
            callback: record_callback,
            user_data,
        },
        construct,
    );
    assert_eq!(status, MediawayDeviceStatus::Ok);

    let became_poisoned = wait_until(
        || handle.poisoned.load(Ordering::Relaxed),
        Duration::from_secs(2),
    );
    assert!(became_poisoned, "bridging thread did not poison on Err");

    // `unregister_callback` does not short-circuit on `poisoned` — it must still be
    // able to join the (already-exited) bridging thread and reclaim the slot.
    let status = join_callback(handle);
    assert_eq!(status, MediawayDeviceStatus::Ok);

    let status = unsafe { mediaway_device_hotplug_close(handle_ptr) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
}

#[test]
fn close_with_active_callback_performs_implicit_unregister() {
    let handle_ptr = handle_idle(vec![DeviceKind::Microphone]);
    // SAFETY: `handle_ptr` was just built above and is not shared with any other
    // thread.
    let handle = unsafe { &mut *handle_ptr };
    let closed = Arc::new(AtomicBool::new(false));
    let log = CallbackLog::default();
    let user_data: *mut c_void = std::ptr::from_ref(&log).cast_mut().cast();
    let construct = mock_constructor(VecDeque::new(), Arc::clone(&closed));

    let status = register_callback_impl(
        handle,
        CallbackTarget {
            callback: record_callback,
            user_data,
        },
        construct,
    );
    assert_eq!(status, MediawayDeviceStatus::Ok);

    let status = unsafe { mediaway_device_hotplug_close(handle_ptr) };
    assert_eq!(status, MediawayDeviceStatus::Ok);
    assert!(closed.load(Ordering::Relaxed));
}

// ── full mode-switch regression (ADR-0002 revision's exact scenario) ────────────────

/// Reproduces the ADR revision's own worked example end to end: `Idle` ->
/// `register_callback` (constructs on the bridging thread, `Idle` -> `Pushing`) ->
/// `unregister_callback` (closes, `Pushing` -> `Idle`) -> `poll_event` (constructs
/// again on the calling thread, `Idle` -> `Pulling`) -> `register_callback` again
/// (closes the `Pulling` mock, `Pulling` -> `Idle` -> `Pushing`). Every transition is
/// asserted, plus that each backend is a *fresh* construction (never a reused mock)
/// and that every superseded mock was actually closed.
#[test]
fn mode_switch_idle_pushing_idle_pulling_pushing() {
    let construction_count = Arc::new(AtomicUsize::new(0));
    let closed_flags: Arc<Mutex<Vec<Arc<AtomicBool>>>> = Arc::new(Mutex::new(Vec::new()));

    // Returns a fresh, one-shot `construct` closure each call, sharing the counters
    // above so the test can observe how many times — and which instances — were built.
    let make_construct = {
        let construction_count = Arc::clone(&construction_count);
        let closed_flags = Arc::clone(&closed_flags);
        move || {
            // clone: each returned closure needs its own strong refs to the shared
            // counters, independent of the factory's own and of every other closure
            // this factory has already produced.
            let construction_count = Arc::clone(&construction_count);
            let closed_flags = Arc::clone(&closed_flags);
            move |_: &[DeviceKind]| {
                construction_count.fetch_add(1, Ordering::Relaxed);
                let closed = Arc::new(AtomicBool::new(false));
                closed_flags
                    .lock()
                    .expect("test mutex")
                    .push(Arc::clone(&closed));
                Ok(Box::new(MockHotplug {
                    script: VecDeque::new(),
                    closed,
                }) as Box<dyn DeviceHotplug>)
            }
        }
    };

    let handle_ptr = handle_idle(vec![DeviceKind::Microphone]);
    // SAFETY: `handle_ptr` was just built above and is not shared with any other
    // thread.
    let handle = unsafe { &mut *handle_ptr };
    assert_eq!(construction_count.load(Ordering::Relaxed), 0);

    let log = CallbackLog::default();
    let user_data: *mut c_void = std::ptr::from_ref(&log).cast_mut().cast();

    // Idle -> Pushing: register_callback constructs #1 on the bridging thread.
    let status = register_callback_impl(
        handle,
        CallbackTarget {
            callback: record_callback,
            user_data,
        },
        make_construct(),
    );
    assert_eq!(status, MediawayDeviceStatus::Ok);
    assert!(wait_until(
        || construction_count.load(Ordering::Relaxed) >= 1,
        Duration::from_secs(2)
    ));
    assert!(matches!(handle.backend, HotplugBackend::Pushing(_)));

    // Pushing -> Idle: unregister_callback joins the bridging thread, which closes #1
    // itself as its last act.
    let status = join_callback(handle);
    assert_eq!(status, MediawayDeviceStatus::Ok);
    assert!(matches!(handle.backend, HotplugBackend::Idle { .. }));
    assert!(
        closed_flags.lock().expect("test mutex")[0].load(Ordering::Relaxed),
        "bridging thread must close its own object before exiting"
    );

    // Idle -> Pulling: poll_event constructs #2 on this (the calling) thread.
    let result = poll_event_impl(handle, make_construct());
    assert_eq!(result, Ok(None));
    assert_eq!(construction_count.load(Ordering::Relaxed), 2);
    assert!(matches!(handle.backend, HotplugBackend::Pulling { .. }));
    assert!(!closed_flags.lock().expect("test mutex")[1].load(Ordering::Relaxed));

    // Pulling -> Idle -> Pushing: register_callback closes #2 (the `Pulling` mock)
    // first, then constructs #3 on a fresh bridging thread.
    let status = register_callback_impl(
        handle,
        CallbackTarget {
            callback: record_callback,
            user_data,
        },
        make_construct(),
    );
    assert_eq!(status, MediawayDeviceStatus::Ok);
    assert!(
        closed_flags.lock().expect("test mutex")[1].load(Ordering::Relaxed),
        "switching Pulling -> Pushing must close the Pulling mock first"
    );
    assert!(wait_until(
        || construction_count.load(Ordering::Relaxed) >= 3,
        Duration::from_secs(2)
    ));
    assert!(matches!(handle.backend, HotplugBackend::Pushing(_)));

    // Clean up: join the final bridging thread and free the handle.
    let status = join_callback(handle);
    assert_eq!(status, MediawayDeviceStatus::Ok);
    unsafe { mediaway_device_hotplug_close(handle_ptr) };
}

// ── null-pointer handling ────────────────────────────────────────────────────────────

#[test]
fn close_and_event_free_are_no_ops_on_null() {
    assert_eq!(
        unsafe { mediaway_device_hotplug_close(std::ptr::null_mut()) },
        MediawayDeviceStatus::Ok
    );
    unsafe { mediaway_device_hotplug_event_free(std::ptr::null_mut()) };
}

#[test]
fn register_and_unregister_reject_null_handle() {
    assert_eq!(
        unsafe {
            mediaway_device_hotplug_register_callback(
                std::ptr::null_mut(),
                Some(record_callback),
                std::ptr::null_mut(),
            )
        },
        MediawayDeviceStatus::InvalidArgument
    );
    assert_eq!(
        unsafe { mediaway_device_hotplug_unregister_callback(std::ptr::null_mut()) },
        MediawayDeviceStatus::InvalidArgument
    );
}
