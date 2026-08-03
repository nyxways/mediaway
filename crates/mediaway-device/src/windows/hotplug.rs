//! Windows `WASAPI` audio hotplug backend (`WindowsDeviceHotplug`,
//! `IMMNotificationClient`). See
//! [`mediaway-device` ADR-0005](../../mediaway-device/adr/0005-device-selection.md)
//! § Hotplug — v1 scope is [`DeviceKind::Microphone`]/[`DeviceKind::Loopback`]
//! only.

#![allow(unsafe_code)]
#![allow(
    clippy::inline_always,
    clippy::ref_as_ptr,
    clippy::redundant_pub_crate,
    reason = "windows `#[implement]` expansion + private module visibility (same as wasapi_process.rs)"
)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::{CaptureError, DeviceEvent, DeviceHotplug, DeviceId, DeviceKind};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator, IMMEndpoint, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, eCapture, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows_core::{Interface, PCWSTR, implement};

use crate::windows_audio::ComGuard;

/// Bounded queue capacity in [`DeviceEvent`]s, mirroring `wasapi.rs`'s
/// `PCM_QUEUE_CAP` drop-oldest convention. Hotplug events are rare,
/// user-driven occurrences (plug/unplug, default-device switch) rather than
/// a per-period stream, so this cap is generous headroom, not a real
/// backpressure concern.
const HOTPLUG_QUEUE_CAP: usize = 64;

struct HotplugQueue {
    events: Mutex<VecDeque<DeviceEvent>>,
}

/// Push `event` onto `queue`, dropping the oldest pending event when full —
/// mirrors `wasapi.rs::pump_capture_loop`'s drop-oldest bounded-queue policy
/// (a poller that falls behind should not grow this queue unboundedly).
/// Pure aside from the lock — exercised directly in `hotplug_tests.rs`.
fn push_bounded(queue: &HotplugQueue, event: DeviceEvent) {
    let Ok(mut events) = queue.events.lock() else {
        return;
    };
    if events.len() >= HOTPLUG_QUEUE_CAP {
        let _ = events.pop_front();
    }
    events.push_back(event);
}

/// `EDataFlow` → the two v1-scope [`DeviceKind`]s ADR-0005 § Hotplug names:
/// `eCapture` → [`DeviceKind::Microphone`], `eRender` → [`DeviceKind::Loopback`].
/// Any other value (`eAll`, or a future flow) has no hotplug-watchable kind
/// and maps to `None`. Pure — no COM/`unsafe` — exercised directly in
/// `hotplug_tests.rs` without live hardware.
fn map_dataflow_to_kind(flow: EDataFlow) -> Option<DeviceKind> {
    if flow == eCapture {
        Some(DeviceKind::Microphone)
    } else if flow == eRender {
        Some(DeviceKind::Loopback)
    } else {
        None
    }
}

/// [`DeviceEvent::DefaultChanged`]'s `kind` for an `OnDefaultDeviceChanged`
/// callback. `OnDefaultDeviceChanged` fires once per `(flow, role)` pair
/// whenever any of the three roles' default changes; only `eConsole` is
/// treated as *the* default here, matching this crate's existing "`eConsole`
/// is the default role" precedent (`enumeration.rs`'s `is_default` check and
/// `wasapi.rs::resolve_endpoint`'s `Select::Default` both resolve against
/// `GetDefaultAudioEndpoint(.., eConsole)`). Without this filter, a single
/// real default-device switch would fire three `DeviceEvent::DefaultChanged`
/// events (`eConsole`/`eMultimedia`/`eCommunications`) for what `enumerate`'s
/// `is_default` treats as one change. Pure — exercised directly in
/// `hotplug_tests.rs`.
fn map_default_changed_kind(flow: EDataFlow, role: ERole) -> Option<DeviceKind> {
    if role == eConsole {
        map_dataflow_to_kind(flow)
    } else {
        None
    }
}

/// Convert a borrowed, callback-owned `PCWSTR` (an `IMMNotificationClient`
/// method parameter) to an owned `String`.
///
/// Deliberately **not** a call to `wasapi.rs::endpoint_id`: that helper
/// pairs `IMMDevice::GetId()` with a matching `CoTaskMemFree`, because
/// `GetId()` hands the caller a freshly allocated string it now owns. The
/// wide string an `IMMNotificationClient` callback parameter points to is
/// different — it is owned by the audio engine's notification dispatcher for
/// the duration of the call only. Calling `CoTaskMemFree` on it here would
/// free memory this module never allocated.
fn pcwstr_to_owned_string(raw: PCWSTR) -> Option<String> {
    if raw.is_null() {
        return None;
    }
    // SAFETY: `raw` is a valid null-terminated wide string for the duration
    // of this callback invocation, per `IMMNotificationClient`'s documented
    // contract — not freed here (see doc comment above).
    unsafe { raw.to_string() }.ok()
}

/// Resolve `id`'s `EDataFlow` via a fresh, call-scoped `IMMDeviceEnumerator`
/// lookup (own `CoInitializeEx`/[`ComGuard`] pair, matching `wasapi.rs`'s
/// per-thread COM setup) and map it to a [`DeviceKind`].
///
/// Called from inside an `IMMNotificationClient` callback, which fires on an
/// OS-owned worker thread — never necessarily the thread that called
/// [`WindowsDeviceHotplug::open`] — so this cannot reuse an enumerator
/// captured at `open()` time even if one were kept around for that purpose.
/// A fresh in-proc activation here costs the same as `enumerate` already
/// pays per call ([`crate::windows::enumeration::enumerate`]).
///
/// A device that has just been physically removed still resolves here:
/// Windows keeps a `DEVICE_STATE_NOTPRESENT`/`DEVICE_STATE_UNPLUGGED` record
/// for a previously seen endpoint rather than deleting it outright, so
/// `GetDevice` on an `OnDeviceRemoved` id is not a race against deletion.
///
/// Best-effort: any COM failure along this path returns `None` (event
/// dropped, not surfaced as an error) — `IMMNotificationClient` methods must
/// stay nonblocking and there is no caller to report a callback-internal
/// failure to (MSDN: "the methods of the interface must be nonblocking").
fn lookup_endpoint_kind(id: &str) -> Option<DeviceKind> {
    // SAFETY: COM init for this callback invocation; `_com` runs
    // `CoUninitialize` on drop before this function returns, matching every
    // other per-call COM scope in this crate (`capabilities.rs`,
    // `enumeration.rs`).
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return None;
    }
    let _com = ComGuard;

    // SAFETY: standard in-proc COM activation.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }.ok()?;

    let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
    let id_pcwstr = PCWSTR::from_raw(wide.as_ptr());
    // SAFETY: `GetDevice` only reads `id_pcwstr` for the duration of this
    // call; `wide` (which it points into) outlives that call on this stack.
    let device = unsafe { enumerator.GetDevice(id_pcwstr) }.ok()?;
    let endpoint: IMMEndpoint = device.cast().ok()?;
    // SAFETY: GetDataFlow is a plain out-param read on a live endpoint.
    let flow = unsafe { endpoint.GetDataFlow() }.ok()?;
    map_dataflow_to_kind(flow)
}

/// The real `IMMNotificationClient` COM server object.
///
/// Every field must stay trivially `Send + Sync`: `IMMNotificationClient`
/// callbacks fire on an arbitrary MTA worker thread owned by the audio
/// engine, never necessarily the thread that called
/// [`WindowsDeviceHotplug::open`]. `windows-rs`'s `#[implement]` macro does
/// not itself enforce a `Send`/`Sync` bound on the wrapped type (unlike,
/// say, `thread::spawn`'s `'static + Send` bound on a closure) — keeping
/// this invariant is a manual responsibility of this module. `Arc<HotplugQueue>`
/// and `Vec<DeviceKind>` (a `Copy` enum) both satisfy it trivially.
#[implement(IMMNotificationClient)]
struct NotificationSink {
    queue: Arc<HotplugQueue>,
    /// The [`DeviceKind`]s this session was opened to watch — already
    /// validated to be a subset of `{Microphone, Loopback}` by
    /// [`WindowsDeviceHotplug::open`].
    kinds: Vec<DeviceKind>,
}

impl NotificationSink {
    /// Resolve `raw_id`'s [`DeviceKind`] and, only if it is one this session
    /// was opened to watch, push `build(id, kind)` onto the shared queue.
    /// Shared by [`IMMNotificationClient_Impl::OnDeviceAdded`],
    /// `OnDeviceRemoved`, and `OnDeviceStateChanged` — the three callbacks
    /// that carry only an endpoint id, not a flow.
    fn push_endpoint_event(
        &self,
        raw_id: PCWSTR,
        build: impl FnOnce(DeviceId, DeviceKind) -> DeviceEvent,
    ) {
        let Some(id_string) = pcwstr_to_owned_string(raw_id) else {
            return;
        };
        let Some(kind) = lookup_endpoint_kind(&id_string).filter(|k| self.kinds.contains(k)) else {
            return;
        };
        push_bounded(
            &self.queue,
            build(DeviceId::from_wasapi_endpoint_id(id_string), kind),
        );
    }
}

impl IMMNotificationClient_Impl for NotificationSink_Impl {
    fn OnDeviceStateChanged(
        &self,
        pwstrdeviceid: &PCWSTR,
        _dwnewstate: DEVICE_STATE,
    ) -> windows_core::Result<()> {
        self.push_endpoint_event(*pwstrdeviceid, |id, kind| DeviceEvent::StateChanged {
            id,
            kind,
        });
        Ok(())
    }

    fn OnDeviceAdded(&self, pwstrdeviceid: &PCWSTR) -> windows_core::Result<()> {
        self.push_endpoint_event(*pwstrdeviceid, |id, kind| DeviceEvent::Added { id, kind });
        Ok(())
    }

    fn OnDeviceRemoved(&self, pwstrdeviceid: &PCWSTR) -> windows_core::Result<()> {
        self.push_endpoint_event(*pwstrdeviceid, |id, kind| DeviceEvent::Removed { id, kind });
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        pwstrdefaultdeviceid: &PCWSTR,
    ) -> windows_core::Result<()> {
        let Some(kind) = map_default_changed_kind(flow, role) else {
            return Ok(());
        };
        if !self.kinds.contains(&kind) {
            return Ok(());
        }
        let id =
            pcwstr_to_owned_string(*pwstrdefaultdeviceid).map(DeviceId::from_wasapi_endpoint_id);
        push_bounded(&self.queue, DeviceEvent::DefaultChanged { kind, id });
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _pwstrdeviceid: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows_core::Result<()> {
        // Not part of ADR-0005's v1 `DeviceEvent` vocabulary — no
        // `DeviceEvent::PropertyChanged` variant exists to translate this
        // into; a required no-op, not a missing feature.
        Ok(())
    }
}

/// Windows `WASAPI` audio hotplug watcher (Microphone / Loopback only — see
/// ADR-0005 § Hotplug).
///
/// Registers a real `IMMNotificationClient` COM callback via
/// `IMMDeviceEnumerator::RegisterEndpointNotificationCallback` and drains the
/// events that callback pushes into a bounded queue.
///
/// # COM object lifetime & threading
///
/// `IMMNotificationClient` callbacks fire on an arbitrary MTA worker thread
/// owned by the audio engine — never necessarily the thread that called
/// [`open`](Self::open), [`poll_event`](DeviceHotplug::poll_event), or
/// [`close`](DeviceHotplug::close). Unlike [`crate::windows::WindowsWasapiCapture`]/
/// [`crate::windows::WindowsWasapiPlayback`], this type does not spawn a worker
/// thread of its own — there is no polling loop to run; the OS calls
/// directly into the registered COM object. `poll_event` only drains the
/// shared queue (no COM calls, no blocking).
///
/// **`open()` holds its `CoInitializeEx`/[`ComGuard`] open for this object's
/// entire lifetime — it is not a short-lived, per-call scope.** An earlier
/// version of this type gave `open()` and `close()` each their own
/// independent `CoInitializeEx`/`ComGuard` pair, on the theory that
/// registration "lives at the audio engine's level" once
/// `RegisterEndpointNotificationCallback` returns and does not depend on the
/// registering apartment staying initialized. **That theory was wrong, and
/// caused a real, reproduced `STATUS_ACCESS_VIOLATION`**: `open()`'s own
/// `ComGuard` called `CoUninitialize()` on its calling thread before
/// `open()` even returned, and `close()` calling
/// `enumerator.UnregisterEndpointNotificationCallback(&client)` through an
/// `IMMDeviceEnumerator` obtained in an apartment that has since been torn
/// down on that thread is exactly the undefined behavior COM's own
/// documentation warns `CoUninitialize` causes for outstanding interface
/// pointers — confirmed via a real SEH exception filter pinpointing the
/// fault inside `IMMDeviceEnumerator::UnregisterEndpointNotificationCallback`
/// itself (`mediaway-device-ffi/adr/0002-callback-event-delivery.md`'s
/// implementation addendum). It only failed to reproduce in this crate's own
/// hardware test because that test's calling thread happened to already have
/// an outstanding, independent `CoInitializeEx` refcount from elsewhere,
/// keeping the apartment alive by accident — not because the design was
/// actually sound.
///
/// The fix: [`HotplugSession`] itself now owns the [`ComGuard`] (`open()`
/// moves it in rather than letting it drop at the end of the function), so
/// the same apartment initialization spans from a successful `open()` all
/// the way through `close()`'s `UnregisterEndpointNotificationCallback` call
/// — `CoUninitialize()` only runs when `HotplugSession` itself is dropped,
/// strictly *after* that call, not before it.
///
/// **This makes explicit a real, narrower thread-affinity requirement than
/// this crate's general "handles may move between threads, just not be used
/// concurrently" convention (`adr/0001-capture-c-abi.md` §9,
/// `mediaway-device-ffi`'s own convention): `open()`, every `poll_event()`,
/// and `close()`/`Drop` must all run on the *same* thread for a given
/// instance.** `ComGuard::drop` calls `CoUninitialize()`, which is itself
/// only valid on the thread that called the matching `CoInitializeEx` — a
/// `HotplugSession` moved to a different thread before `close()` would
/// `CoUninitialize()` an apartment that thread never initialized, which is
/// its own distinct bug. This is not a new constraint invented for this fix:
/// the locally-implemented `client` (`NotificationSink`, a
/// `#[implement]`-generated object) is confirmed agile
/// (`IAgileObject`/`IMarshal` by default per `windows-implement`'s own macro
/// behavior), but the real, OS-provided `enumerator` (`MMDeviceEnumerator`)
/// is confirmed **not** agile — a real `QueryInterface(IAgileObject)`
/// against a live instance
/// (`lib_tests.rs::mmdevice_enumerator_does_not_implement_iagileobject_or_skip`)
/// fails with `E_NOINTERFACE` on real hardware, despite an earlier version
/// of this doc claiming otherwise without a citation. A non-agile interface
/// pointer was never soundly usable from a thread other than the one that
/// created it in the first place; this fix's same-thread contract merely
/// states that pre-existing limit plainly instead of leaving it implicit.
/// **Consequence: `WindowsDeviceHotplug`/`HotplugSession` must never be given
/// `unsafe impl Send`** — doing so would be unsound regardless of this fix.
/// This is exactly the `WindowsDeviceHotplug: Send` question
/// `mediaway-device-ffi/adr/0002-callback-event-delivery.md` §8 left open;
/// it is answered, empirically, as "no" — and `mediaway-device-ffi`'s own
/// lazy-construction design (that ADR's revision) already keeps a
/// `WindowsDeviceHotplug` confined to a single thread for its whole life for
/// an unrelated reason, so it also happens to satisfy this constraint.
pub struct WindowsDeviceHotplug {
    inner: Option<HotplugSession>,
}

struct HotplugSession {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
    queue: Arc<HotplugQueue>,
    /// Keeps this session's COM apartment initialized from a successful
    /// [`WindowsDeviceHotplug::open`] until [`HotplugSession`] itself is
    /// dropped (in [`DeviceHotplug::close`], strictly after the
    /// `UnregisterEndpointNotificationCallback` call) — see the type-level
    /// doc for why a per-call scope crashed instead.
    _com: ComGuard,
}

impl WindowsDeviceHotplug {
    /// Open a hotplug watcher for `kinds`.
    ///
    /// # Errors
    ///
    /// [`CaptureError::InvalidInput`] when `kinds` is empty — nothing to
    /// watch. [`CaptureError::Unsupported`] when `kinds` contains anything
    /// other than [`DeviceKind::Microphone`]/[`DeviceKind::Loopback`] — v1
    /// scope is audio-only (ADR-0005 § Hotplug); this backend rejects an
    /// out-of-scope kind outright rather than silently watching only the
    /// subset it supports, matching every other Windows backend in this
    /// crate hard-rejecting an unsupported config instead of partially
    /// honoring it. [`CaptureError::Backend`] on COM/API failures.
    pub fn open(kinds: &[DeviceKind]) -> Result<Self, CaptureError> {
        if kinds.is_empty() {
            return Err(CaptureError::InvalidInput);
        }
        let mut watched = Vec::with_capacity(kinds.len());
        for &kind in kinds {
            match kind {
                DeviceKind::Microphone | DeviceKind::Loopback => watched.push(kind),
                // `DeviceKind` is `#[non_exhaustive]`; `ProcessLoopback` (no
                // device identity to watch) and any other kind falls here
                // too, per ADR-0005 § Hotplug's "audio only in v1" scope.
                _ => return Err(CaptureError::Unsupported),
            }
        }

        // SAFETY: COM init for this session's entire lifetime — `_com` moves into
        // `HotplugSession` below rather than dropping here (see the type-level doc:
        // a per-call scope previously called `CoUninitialize` before `open` even
        // returned, corrupting the stored `enumerator`/`client` for `close`).
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_err() {
            return Err(CaptureError::Backend);
        }
        let com = ComGuard;

        // SAFETY: standard in-proc COM activation.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }
                .map_err(|_| CaptureError::Backend)?;

        let queue = Arc::new(HotplugQueue {
            events: Mutex::new(VecDeque::new()),
        });
        // clone: Arc share — the `IMMNotificationClient` COM object outlives
        // this function (the audio engine keeps calling it after `open`
        // returns), so it needs its own strong ref to the queue independent
        // of the one `HotplugSession` keeps for `poll_event`.
        let sink_queue = Arc::clone(&queue);
        let client: IMMNotificationClient = NotificationSink {
            queue: sink_queue,
            kinds: watched,
        }
        .into();

        // SAFETY: registers `client` for endpoint notifications. Its
        // refcount is held by the owned `client` binding stored in
        // `HotplugSession` below (not a temporary), matching
        // `RegisterEndpointNotificationCallback`'s documented contract that
        // the caller keeps the object alive until
        // `UnregisterEndpointNotificationCallback`.
        unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }
            .map_err(|_| CaptureError::Backend)?;

        Ok(Self {
            inner: Some(HotplugSession {
                enumerator,
                client,
                queue,
                _com: com,
            }),
        })
    }
}

impl DeviceHotplug for WindowsDeviceHotplug {
    fn poll_event(&mut self) -> Result<Option<DeviceEvent>, CaptureError> {
        let Some(session) = self.inner.as_ref() else {
            return Err(CaptureError::Closed);
        };
        let mut events = session
            .queue
            .events
            .lock()
            .map_err(|_| CaptureError::Backend)?;
        Ok(events.pop_front())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        let Some(session) = self.inner.take() else {
            return Ok(());
        };
        // No fresh `CoInitializeEx` here — `session._com` has kept this thread's
        // apartment (the one `open()` initialized) alive the whole time; using a
        // second, independent `CoInitializeEx`/`ComGuard` pair here is exactly what
        // used to crash (see the type-level doc). `session._com` drops at the end of
        // this function, strictly after the call below, calling `CoUninitialize` last.
        //
        // SAFETY: unregisters the same `client` passed to
        // `RegisterEndpointNotificationCallback` in `open`, through the same
        // still-live apartment that registered it. Not called from within an
        // `IMMNotificationClient` callback (MSDN: doing so would deadlock) — `close`
        // always runs on the caller's thread. Must run on the same thread that called
        // `open` (see the type-level doc's thread-affinity requirement) — `session`'s
        // `_com` guard cannot validly `CoUninitialize` any other thread's apartment.
        unsafe {
            session
                .enumerator
                .UnregisterEndpointNotificationCallback(&session.client)
        }
        .map_err(|_| CaptureError::Backend)?;
        Ok(())
    }
}

impl Drop for WindowsDeviceHotplug {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
#[path = "hotplug_tests.rs"]
mod tests;
