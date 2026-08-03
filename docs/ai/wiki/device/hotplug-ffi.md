# `mediaway-ffi` hotplug — callback event delivery over the C ABI

**Status: implemented, real Windows backend wired in, `close()` crash root-caused and
fixed.** [`adr/0002-callback-event-delivery.md`](../../../../crates/mediaway-ffi/adr/0002-callback-event-delivery.md)
(Accepted, plus a "lazy, thread-owned construction" revision and three implementation
addenda). `HotplugHandle` and the six `mediaway_device_hotplug_*` symbols exist
(`src/hotplug.rs`), default-on `hotplug` feature, header addition. `open` ->
`poll_event`/`register_callback`/`close` all genuinely work against
`WindowsDeviceHotplug` on Windows, confirmed on real hardware — see § Real backend below.

## Decision in one line

Ship **both** poll and callback registration on one `mediaway_device_hotplug_t`
handle, **mutually exclusive per handle** (switching modes means unregister
then re-register). `DeviceHotplug` at the Rust level stays sync-poll — the
push mechanism lives entirely inside `mediaway-ffi`.

## The callback is not the real OS thread

The C callback is invoked from a **separate, `mediaway-ffi`-owned
bridging thread** (`hotplug.rs::bridging_loop`) that polls `poll_event()`
every 50ms (`HOTPLUG_CALLBACK_POLL_INTERVAL`) and invokes the caller's
function pointer per drained event — push from the caller's point of view,
with a bounded ~50ms added latency, not zero-latency. Every iteration is
wrapped in one `catch_unwind`; a real backend `Err` or a caught panic
poisons the handle and stops the thread.

## Why exclusivity, not just a `Mutex`

Poll and callback both drain the *same* single-consumer queue — locking
would stop a data race but not the deeper bug: one logical event stream
nondeterministically split between two consumers. `poll_event` while a
callback is registered returns `CALLBACK_MODE_ACTIVE`; registering twice
returns `CALLBACK_ALREADY_REGISTERED`. `unregister_callback`/`close` join the
bridging thread and are the only two functions here that do **not**
short-circuit on a poisoned handle — both must reclaim a thread that
poisoned itself.

## Handle shape — lazy, thread-owned construction (supersedes the original design)

`HotplugHandle { poisoned: Arc<AtomicBool>, backend: HotplugBackend }`, where
`HotplugBackend` is `Idle { kinds }` / `Pulling { kinds, inner: Box<dyn
DeviceHotplug> }` / `Pushing(CallbackBridge)`. `open()` only validates
`kinds`; the real backend is constructed **lazily, on whichever thread first
needs it** — the caller's thread on first `poll_event` (`Idle` -> `Pulling`),
the bridging thread's own body on first `register_callback` (`Idle` ->
`Pushing`) — and never crosses a thread boundary after, so **no
`Arc<Mutex<..>>`, no `unsafe impl Send` on the backend, no channel**.
`register_callback` on `Pulling` closes the existing object, then
reconstructs for push mode. `unregister_callback` joins the bridging thread
and returns to `Idle { kinds }`. `CallbackTarget` (function pointer +
`user_data`, `unsafe impl Send`, never dereferenced by this crate) is
unchanged from the original design.

## `mediaway_device_event_t`

Flat struct + discriminant, not a C union. `device_id` is an owned,
NUL-terminated UTF-8 C string in poll mode; **borrowed**, valid only for the
callback call's duration, in callback mode. `mediaway_device_kind_t` is this
crate's first full `DeviceKind` mirror.

## Real backend: wired in, `open`/`poll_event`/`close` all work

`WindowsDeviceHotplug: Send` was confirmed **unsound** (a live
`QueryInterface(IAgileObject)` against `MMDeviceEnumerator` fails with
`E_NOINTERFACE`), so the handle-shape revision above sidesteps the
requirement instead of fixing it. `open_hotplug` now dispatches to the real
`WindowsDeviceHotplug::open` on Windows (Linux has no backend yet). 19
sibling unit tests pass against a mock `DeviceHotplug`; a real-hardware check
confirms `open()` -> `poll_event()` -> `close()` genuinely reach and succeed.

**`close()` crash, root-caused and fixed**: `close()` on a real,
successfully-constructed handle used to crash the process
(`STATUS_ACCESS_VIOLATION`) — root-caused via a Win32 SEH exception filter to
`WindowsDeviceHotplug::open()` calling `CoUninitialize()` before returning
while still storing the `IMMDeviceEnumerator` obtained under that torn-down
apartment; `close()` later called `UnregisterEndpointNotificationCallback`
through the now-stale pointer. Fixed in `mediaway-device-windows/src/hotplug.rs`
by having `HotplugSession` own its `ComGuard` for the object's whole
lifetime instead of two independent per-call scopes. Full write-up: ADR-0002's
2026-07-31 addenda; the fix's own thread-affinity requirement (`open`/
`poll_event`/`close` must share one thread) is documented in that file's
type-level doc and already satisfied by this crate's lazy-construction design.

## Python / GIL finding (sourced, not assumed)

The blanket "GIL makes foreign-thread callbacks unsafe" claim is **overstated**:
`ctypes.CFUNCTYPE`/`cffi`'s API-mode `extern "Python"` both correctly acquire
the GIL for a call from a thread Python didn't create. Narrower real reasons
to still default Python bindings to poll: `ctypes`' foreign-thread path
resets `threading.local` every call, and `cffi`'s no-build-step
`ffi.callback()` has documented crash risk on hardened OSes. See ADR-0002 §5.

## General principle for future event-shaped `-ffi` additions

ADR-0002's final section is written to be cited directly (poll stays at the
Rust core; both poll+callback at the C ABI; mode exclusivity; thread/latency/
reentrancy/unwind documentation) — read it before designing a second
event-shaped capability rather than re-deriving the same decisions.
