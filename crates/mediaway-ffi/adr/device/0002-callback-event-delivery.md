# ADR-0002: Callback-based event delivery over the C ABI — `DeviceHotplug` as the first case

- **Status**: Accepted
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-ffi`

## Context

Every C ABI function this crate has shipped so far (`adr/0001-capture-c-abi.md`) is
either a one-shot call (`open`/`close`) or a **non-blocking poll**
(`mediaway_video_capture_poll_frame`/`mediaway_audio_capture_poll_frame`,
`Ok(None)` = idle). That convention matches the underlying Rust traits, which are
all sync-poll by design (`docs/spec/async-and-streaming.md`). `DeviceHotplug`
(`mediaway-device/src/hotplug.rs`) is the same shape:

```rust
pub trait DeviceHotplug {
    fn poll_event(&mut self) -> Result<Option<DeviceEvent>, CaptureError>;
    fn close(&mut self) -> Result<(), CaptureError>;
}
```

**This trait is correct and is not touched by this ADR.** Poll is the more
composable sans-io-adjacent primitive — a Rust caller who wants push semantics
can trivially wrap a poll loop in their own thread; the reverse (deriving a poll
API from a push-only one) is strictly harder. Nothing here proposes changing
`mediaway-device`'s public surface.

`mediaway-device-windows/src/hotplug.rs` ships the first real backend,
`WindowsDeviceHotplug` (`IMMNotificationClient`-backed, Microphone/Loopback only
per [`mediaway-device/adr/0005-device-selection.md`](../../mediaway-device/adr/0005-device-selection.md)
§ Hotplug, hardware-verified, wired into `mediaway-device-windows`'s public API:
`pub use hotplug::WindowsDeviceHotplug;` in that crate's `lib.rs`). Its own module
doc is explicit about where events actually originate: `IMMNotificationClient`
callbacks fire on **an arbitrary MTA worker thread owned by the audio engine**,
never necessarily the thread that called `open`/`poll_event`/`close`. That
callback pushes into a bounded, `Mutex`-guarded queue (`HOTPLUG_QUEUE_CAP = 64`);
`poll_event` only pops from that queue — cheap, no COM calls, no blocking.

**`mediaway-device-ffi` exposes none of this today.** There is no
`mediaway_device_hotplug_*` symbol anywhere in this crate — this ADR designs a
genuinely new addition, not a correction of an existing surface (unlike most of
`adr/0001`'s findings).

### The open question

`DeviceHotplug` is the first of what will likely be several **event-shaped**
capabilities this crate family exposes over time (future candidates named in the
task brief: encoder/decoder backend-lost, permission-state changes). At the raw C
ABI boundary — not the Rust trait — should an event-shaped capability:

(a) expose only a poll-shaped function, matching every other `poll_*`-shaped
    function this workspace's `-ffi` crates already ship, or
(b) let a caller register a function-pointer callback the library invokes
    directly when an event occurs, or
(c) both.

### Real precedent for (b)

Mature C libraries with an event model already do this, with signatures worth
comparing against directly:

- **libusb**: `typedef int (*libusb_hotplug_callback_fn)(libusb_context *ctx, libusb_device *device, libusb_hotplug_event event, void *user_data);` registered via `libusb_hotplug_register_callback(ctx, events, flags, vendor_id, product_id, dev_class, cb_fn, user_data, &callback_handle)`. `device` is **borrowed**, not owned by the callback. ([libusb hotplug docs](https://libusb.sourceforge.io/api-1.0/group__libusb__hotplug.html))
- **PortAudio**: a per-stream `PaStreamCallback` function pointer supplied at `Pa_OpenStream` time, invoked by a PortAudio-owned thread — the same "library owns the thread, caller supplies a function pointer + opaque user data" shape.
- **CoreAudio**: `AudioObjectPropertyListenerProc`, registered per property via `AudioObjectAddPropertyListener`, invoked on a CoreAudio-internal thread — the Apple-platform analog of `IMMNotificationClient`'s role here.

All three share the same skeleton this ADR adopts: `fn(user_data, event_data) -> void|int`, opaque `user_data` threaded through unmodified, event payload borrowed (not owned) by the callback invocation.

### `docs/spec/c-ffi.md` does not currently decide this

Design rule 2 there is "opaque handles + error codes; no panic across FFI" — it
does not say push vs. pull for event-shaped surfaces, and nothing else in that
spec or `docs/adr/0004-c-ffi.md` addresses it either. This is a genuine gap this
ADR fills, at crate scope per `docs/conventions/docs-layout.md` ("crate
decisions → that crate's `adr/`"), written so a later `mediaway-*-ffi` crate can
cite it directly rather than re-deriving the same reasoning (see § General
principle) — the same "decide once locally, let siblings cite it" precedent
`mediaway-pipeline-ffi/adr/0001` and `mediaway-device-ffi/adr/0001` already set
for handle/status-enum shape.

## Decision

> Ship **both** (c): keep `mediaway_device_hotplug_poll_event` as the baseline,
> always-available, poll-shaped function, and add
> `mediaway_device_hotplug_register_callback` /
> `mediaway_device_hotplug_unregister_callback` as an opt-in push mode on the
> same handle. **Exactly one mode is active per handle at a time** — not a
> free-standing recommendation, an enforced exclusivity (§4). The callback is
> implemented via a `mediaway-device-ffi`-owned bridging thread that itself
> calls the real Rust `poll_event()` at a documented, bounded interval and
> invokes the caller's function pointer per drained event — **not** a literal
> re-wiring of the raw `IMMNotificationClient` COM thread into the C callback
> (§1 corrects the task brief's framing on this point). Binding-author guidance:
> prefer callback registration for languages with mature native-callback
> marshaling (C# `[UnmanagedCallersOnly]`/delegates, Go `cgo` exports,
> Kotlin/JNI, Swift C bridging); prefer poll — or accept callback registration
> only via `cffi`'s API-mode `extern "Python"`, not the simpler `ctypes`/`cffi`
> ABI-mode path — for Python bindings (§5, with sourced findings, not asserted
> as folklore).

### 1. Correction to the task brief's framing — the callback is not literally the COM thread

The task brief describes the callback as invoked "from the real Windows
implementation's `IMMNotificationClient` callback... an arbitrary MTA worker
thread." That is true of where the **event originates** — `WindowsDeviceHotplug`
really does receive events on that thread. It is **not**, in this design, where
the **C callback is invoked**, and the distinction is load-bearing, not
pedantic:

`DeviceHotplug` is accessed through this crate's established `Box<dyn
DeviceHotplug>` trait-object pattern (matching `VideoCaptureHandle`/
`AudioCaptureHandle`, `adr/0001-capture-c-abi.md` §2) — generic over any current
or future backend, not just Windows. The trait exposes only `poll_event`/
`close`; there is no subscribe/callback method to reach into, and this ADR does
not add one (§ Context — the trait stays unchanged). Reaching the raw COM thread
directly would require downcasting past the trait object to a concrete
`WindowsDeviceHotplug` and threading a closure into its private
`NotificationSink` — breaking the exact backend-genericity the trait-object
design already committed to, for a Windows-only shortcut.

So the real mechanism is: `mediaway_device_hotplug_register_callback` spawns
**one dedicated bridging thread per handle**, owned by `mediaway-device-ffi`
(not the caller, not the OS audio engine), that loops
`inner.poll_event()` → build `mediaway_device_event_t` → invoke the caller's
function pointer → free, on a fixed poll interval (§3). From the C caller's
perspective this is still genuine push — no polling loop in *their* code — but
it is **callback-shaped near-real-time delivery with a bounded added latency**,
not zero-latency OS-thread-direct delivery. Per
[`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md),
this is stated plainly, not left implicit: a consumer reading only the function
name would reasonably assume "instant," and that assumption would be wrong by up
to one poll interval.

The thread-safety contract (§4) — arbitrary thread, no reentrancy, no blocking,
no unwind — still applies unchanged; it just applies to the bridging thread
rather than literally the COM thread. Practically indistinguishable to a
correctly-written caller (which must treat "some unspecified thread" identically
either way), but the ADR states the real mechanism rather than the shortcut the
brief assumed.

### 2. Function list and types

```c
/* — the callback function pointer type — */
typedef void (*mediaway_device_hotplug_callback_fn)(
    void *user_data,
    const mediaway_device_event_t *event);

/* — open / close, mirroring mediaway_video_capture_open/close's shape — */
mediaway_device_status_t mediaway_device_hotplug_open(
    const mediaway_device_kind_t *kinds, size_t kinds_len,
    mediaway_device_hotplug_t **out_hotplug);
mediaway_device_status_t mediaway_device_hotplug_close(
    mediaway_device_hotplug_t *hotplug);

/* — push mode — */
mediaway_device_status_t mediaway_device_hotplug_register_callback(
    mediaway_device_hotplug_t *hotplug,
    mediaway_device_hotplug_callback_fn callback,
    void *user_data);
mediaway_device_status_t mediaway_device_hotplug_unregister_callback(
    mediaway_device_hotplug_t *hotplug);

/* — pull mode (default; always available unless a callback is registered) — */
mediaway_device_status_t mediaway_device_hotplug_poll_event(
    mediaway_device_hotplug_t *hotplug,
    mediaway_device_event_t *out_event, bool *out_has_event);

/* — owned output free — */
void mediaway_device_hotplug_event_free(mediaway_device_event_t *event);
```

`mediaway_device_hotplug_t` is a forward-declared opaque struct, same convention
as `mediaway_video_capture_t`/`mediaway_audio_capture_t`.

`kinds`/`kinds_len` mirror `WindowsDeviceHotplug::open(kinds: &[DeviceKind])`
directly: borrowed, no free function, empty → `InvalidArgument`
(`CaptureError::InvalidInput`), an out-of-v1-scope kind (anything but
Microphone/Loopback) → `Unsupported`, matching that backend's own validation
1:1 (no new validation invented at the FFI layer).

**The callback receives a *borrowed* `const mediaway_device_event_t *`, valid
only for the duration of the call** — the bridging thread owns it, invokes the
callback, and frees it immediately after the call returns. This deliberately
differs from `poll_event`'s owned-output-the-caller-must-free convention: it
matches libusb's `libusb_device *device` (also borrowed in the hotplug callback,
§ Context) and removes a whole class of binding-author mistakes (forgetting to
free, freeing on the wrong thread, freeing after the callback returns) for the
push path specifically. A callback implementation that needs `device_id`
afterward must copy it itself before returning.

### 3. Bridging thread — mechanism, poll interval, and honesty about latency

```rust
/// Poll interval for the mediaway-device-ffi-owned bridging thread while a
/// callback is registered. Hotplug events are rare and user-driven (plug/unplug,
/// default-device switch), not a per-frame stream — mirrors
/// `hotplug.rs::HOTPLUG_QUEUE_CAP`'s "generous headroom, not a real backpressure
/// concern" reasoning. Fixed for v1, not caller-configurable (§ Deferred).
const HOTPLUG_CALLBACK_POLL_INTERVAL: Duration = Duration::from_millis(50);
```

`register_callback` spawns a thread running, conceptually:

```rust
while !stop.load(Relaxed) {
    let outcome = catch_unwind(AssertUnwindSafe(|| inner.lock().unwrap().poll_event()));
    match outcome {
        Ok(Ok(Some(event))) => invoke_callback(&target, event), // builds + frees the FFI struct
        Ok(Ok(None)) => thread::sleep(HOTPLUG_CALLBACK_POLL_INTERVAL),
        Ok(Err(_)) | Err(_) => { poisoned.store(true, Relaxed); break } // real backend error or a caught panic — both fatal to this session
    }
}
```

Every iteration is individually `catch_unwind`-wrapped — a panic inside
`poll_event()`, inside building `MediawayDeviceEvent` (e.g. a defensive
`CString::new` failure, §6), or inside the caller's own callback (only if that
callback itself is Rust code reached through a Rust-side test harness; a
genuine foreign-language panic/exception is the caller's own UB to prevent, §4)
poisons the handle and stops the thread rather than looping in a broken state
silently. **A normal (non-panic) `Err` from `poll_event()` is also treated as
fatal** to the bridging session — there is no error-reporting channel through
the callback function pointer (which only carries events, by design; adding one
is deferred, see § Deferred) — the handle is poisoned and the next explicit call
the caller makes (`poll_event`, `unregister_callback`, `close`) observes
`HandlePoisoned`/the real status then. This is an accepted v1 limitation, not
hidden: a callback-only consumer that never calls another function on the handle
until shutdown will not learn about a mid-session backend failure until `close`.

### 4. Mode exclusivity — poll and callback are mutually exclusive per handle, not just a documentation note

`poll_event` and the bridging thread both ultimately call the **same**
`inner.poll_event()` on the **same** underlying single-consumer queue. Mixing
them would not just be a data race to guard against with a lock — it would
**nondeterministically split delivery of one logical event stream** between two
independent consumers, which is a semantic bug (a caller polling directly could
silently "steal" an event the registered callback was supposed to see, or vice
versa), not merely a thread-safety hazard a `Mutex` alone fixes. So:

- `mediaway_device_hotplug_register_callback` on a handle that already has an
  active callback returns a new status, `CallbackAlreadyRegistered` (§7) — it
  does **not** replace the previous callback. Callers that want to change the
  callback call `unregister_callback` first, then `register_callback` again;
  no hidden extra thread-join cost versus an implicit replace, since replacing
  would still have to join the old thread first internally.
- `mediaway_device_hotplug_poll_event`, called while a callback is registered,
  returns a new status, `CallbackModeActive` (§7), and drains nothing — it does
  **not** silently degrade to sharing the queue with the bridging thread.
- `mediaway_device_hotplug_unregister_callback` joins the bridging thread
  (blocking — see honesty note below) and returns the handle to poll mode.
  **Idempotent**: calling it when no callback is registered is a documented
  no-op returning `Ok`, matching this crate's existing "safe to call when
  already in the target state" convention (`mediaway_video_capture_close` on
  `NULL`).
- `mediaway_device_hotplug_close`, called while a callback is registered,
  performs the same join first (an implicit unregister), then closes the
  underlying `DeviceHotplug` (unregisters the real `IMMNotificationClient`) and
  frees the handle — one call does the full teardown, matching how
  `mediaway_video_capture_close` already documents itself as "blocks for up to
  one frame interval, joins the backend's worker thread."

**Blocking cost, stated explicitly (`docs/spec/caveats-and-clarity.md`)**:
`unregister_callback` and a callback-mode `close` block for up to
`HOTPLUG_CALLBACK_POLL_INTERVAL` **plus** the time any currently in-flight
callback invocation takes to return — the bridging thread cannot be safely
killed mid-callback (the caller's function pointer would be reentered into or
its `user_data` invalidated out from under it). A callback that blocks
indefinitely (§5's "must not block" rule) makes `unregister_callback`/`close`
block indefinitely too; this is the caller's own contract violation surfacing
as a hang, not a bug in this design.

### 5. Thread-safety contract

Stated once here as the template every future event-shaped `-ffi` addition
should copy (§ General principle):

1. **The callback may be invoked from an unspecified, Mediaway-owned thread**
   — never the thread that called `register_callback`, and (§1) not
   necessarily the platform backend's own raw OS-callback thread either. A
   binding must not assume any particular thread identity, priority, or COM
   apartment state.
2. **The callback must not block.** It runs on the one bridging thread this
   handle owns; blocking it delays every subsequent event and blocks
   `unregister_callback`/`close` (§4).
3. **The callback must not call back into any `mediaway_device_*` function on
   the *same* handle synchronously.** `WindowsDeviceHotplug`'s own doc already
   states the underlying constraint this propagates: MSDN documents that
   `IMMNotificationClient` methods "must be nonblocking," and
   `WindowsDeviceHotplug::close`'s own comment notes calling
   `UnregisterEndpointNotificationCallback` from inside a live notification
   callback would deadlock. Calling `mediaway_device_hotplug_close`/
   `unregister_callback` on the same handle from inside the callback is the
   direct C-ABI analog of that exact deadlock and is forbidden for the same
   reason, transitively.
4. **The callback must not unwind/panic across the FFI boundary.** This
   crate's own `catch_unwind` wrapping (§3) covers panics inside *this crate's*
   Rust code up to and including the call into the callback function pointer —
   it does **not**, and cannot, cover a foreign exception (a C++ exception, a
   Go panic escaping a `cgo` export, a Swift/JNI exception) unwinding *back*
   into this crate's Rust frames from inside the caller's own callback body.
   Unwinding across an `extern "C"` boundary with a foreign personality is
   undefined behavior per Rust's own `extern "C"` contract (`rustc` diagnoses
   this at runtime as an abort when it can detect it, but detection is not
   guaranteed for every source language). **This is the caller's
   responsibility to prevent** (e.g. catch and convert to an error code
   *before* returning from their callback), not something this ADR — or
   `docs/spec/c-ffi.md`, checked and confirmed silent on this exact point —
   currently states anywhere else in this workspace. Flagged as a real,
   general C-FFI-spec gap in § Deferred, not invented ad hoc just for hotplug.

### 6. `mediaway_device_event_t` — flat struct + discriminant, not a C union

Mirrors [`DeviceEvent`](../../mediaway-device/src/hotplug.rs) 1:1:

```c
typedef enum mediaway_device_kind {
    MEDIAWAY_DEVICE_KIND_SCREEN           = 0,
    MEDIAWAY_DEVICE_KIND_WINDOW           = 1,
    MEDIAWAY_DEVICE_KIND_CAMERA           = 2,
    MEDIAWAY_DEVICE_KIND_MICROPHONE       = 3,
    MEDIAWAY_DEVICE_KIND_LOOPBACK         = 4,
    MEDIAWAY_DEVICE_KIND_PROCESS_LOOPBACK = 5,
    /* DeviceKind is #[non_exhaustive] (ADR-0005 leaves room for Linux/Web
     * variants); catch-all for a future Rust-side addition, same reasoning as
     * MediawayDeviceStatus::UnknownError (adr/0001-capture-c-abi.md §3) — the
     * first time this crate applies that reasoning to a *data* enum rather
     * than an error enum. Not reachable from any backend today: v1 hotplug
     * scope is Microphone/Loopback only (WindowsDeviceHotplug::open rejects
     * every other kind at open() time). */
    MEDIAWAY_DEVICE_KIND_UNKNOWN          = 255,
} mediaway_device_kind_t;

typedef enum mediaway_device_event_kind {
    MEDIAWAY_DEVICE_EVENT_ADDED           = 0,
    MEDIAWAY_DEVICE_EVENT_REMOVED         = 1,
    MEDIAWAY_DEVICE_EVENT_DEFAULT_CHANGED = 2,
    MEDIAWAY_DEVICE_EVENT_STATE_CHANGED   = 3,
} mediaway_device_event_kind_t;

/* Owned; release with mediaway_device_hotplug_event_free (poll mode only —
 * the callback-mode form is borrowed, see §2). Flat struct + discriminant, not
 * a C union — follows this crate's existing mediaway_video_capture_config_t /
 * mediaway_audio_capture_config_t convention (adr/0001-capture-c-abi.md §5:
 * "kind field + flat struct with fields ignored per variant") rather than
 * introducing this crate's first tagged C union. */
typedef struct mediaway_device_event {
    mediaway_device_event_kind_t event_kind;
    mediaway_device_kind_t device_kind;
    /* Owned, NUL-terminated UTF-8 — DeviceId's Display form (ADR-0005), e.g.
     * "wasapi:<endpoint-id>". Chosen over a data+len byte-buffer pair (this
     * crate's convention for binary frame payloads) because it is plain,
     * NUL-free-by-construction text and every Tier B language's native string
     * marshaling (P/Invoke, ctypes/cffi, cgo, JNI, Swift bridging) handles a
     * NUL-terminated C string more directly than a manual pointer+length pair.
     * NULL only for DEFAULT_CHANGED when DeviceEvent::DefaultChanged.id is
     * None (the kind now has no default). In the practically-unreachable case
     * a device identity ever did contain an embedded NUL, CString::new fails
     * defensively to NULL here too, rather than dropping the whole event —
     * event_kind/device_kind still carry real information even without an id. */
    char *device_id;
} mediaway_device_event_t;
```

`mediaway_device_kind_t` is this crate's **first** C mirror of
[`DeviceKind`](../../mediaway-device/src/capability.rs) — no prior
`mediaway_device_*_t` symbol wraps it (confirmed: `video.rs`/`audio.rs` only
`use mediaway_device::DeviceKind` internally, never expose it to C).
`mediaway_video_capture_config_t`/`mediaway_audio_capture_config_t` instead
expose their own narrower per-capability source-kind enums
(`mediaway_device_video_source_kind_t`/`mediaway_device_audio_source_kind_t`).
This ADR mints the first full mirror because `mediaway_device_hotplug_open`'s
`kinds` parameter and `DeviceEvent`'s `kind` field both need the **general**
`DeviceKind`, not a capability-narrowed subset.

### 7. Status enum — two new, crate-local variants

```c
typedef enum mediaway_device_status {
    /* ...existing 0-10 unchanged, adr/0001-capture-c-abi.md §3... */
    MEDIAWAY_DEVICE_STATUS_CALLBACK_ALREADY_REGISTERED = 11, /* register_callback called twice without an intervening unregister_callback (§4) */
    MEDIAWAY_DEVICE_STATUS_CALLBACK_MODE_ACTIVE        = 12, /* poll_event called while a callback is registered (§4) */
} mediaway_device_status_t;
```

Appended, not inserted — this crate's status enum is already independently
numbered from every sibling `-ffi` crate's (`adr/0001-capture-c-abi.md` §3), so
there is no cross-crate numeric alignment to preserve, and `publish = false`
(no header/ABI has shipped, per this crate's own `Cargo.toml` comment) means
there is no external consumer to break either way — appending is simply the
smaller, more mechanical diff against the existing header.

### 8. Handle shape — a genuine structural deviation from ADR-0001 §2

`VideoCaptureHandle`/`AudioCaptureHandle` are `{ poisoned: bool, inner: Box<dyn
Trait> }`, thread-**confined** by convention (may move between threads, never
touched by two threads at once). `HotplugHandle` cannot use that shape as-is:
once a callback is registered, the handle is, for the first time in this crate,
genuinely touched by **two** threads by design — the caller's thread (for
`register_callback`/`unregister_callback`/`poll_event`/`close`) and this
crate's own bridging thread (§3). This ADR's handle shape:

```rust
struct HotplugHandle {
    /// Shared, not a plain `bool`: the bridging thread must be able to poison
    /// the handle itself (§3's catch_unwind/fatal-Err path) while the caller's
    /// thread independently reads/sets it on every other call — the same
    /// "poisoned" meaning as ADR-0001 §2/§8, extended to be safely observable
    /// from two threads for the first time in this crate.
    poisoned: Arc<AtomicBool>,
    /// `Mutex`-guarded even though mode exclusivity (§4) already prevents two
    /// threads calling `poll_event()` concurrently by construction: hotplug
    /// events are rare/user-driven, not a hot path (`hotplug.rs`'s own
    /// "generous headroom, not a real backpressure concern" framing), so the
    /// negligible lock cost buys real defense-in-depth against a future
    /// mode-exclusivity bug reaching two-threads-one-poll_event undefined
    /// behavior instead of a clean deadlock/panic — deliberate per
    /// `docs/conventions/code-style.md` § Allocation/clone/copy discipline's
    /// "deliberate on hot paths" carve-out: this is explicitly not one.
    inner: Arc<Mutex<Box<dyn DeviceHotplug + Send>>>,
    /// `Some` only while a callback is registered; the join handle + stop flag
    /// for the bridging thread (§3).
    callback: Option<CallbackBridge>,
}

struct CallbackBridge {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

/// Wraps the caller's function pointer + opaque `user_data` so both can move
/// into the bridging thread (`thread::spawn` requires `Send + 'static`).
/// Neither an `extern "C" fn(...)` pointer nor a raw `*mut c_void` is `Send` by
/// default in Rust's type system, even though both are plain addresses at the
/// machine level.
///
/// # Safety
///
/// `unsafe impl Send` is sound only because this crate never dereferences
/// `user_data` itself — it is opaque, caller-owned data threaded straight
/// through to `callback` on every invocation, unmodified. The **caller** is
/// responsible for `user_data` being safe to access from whatever unspecified
/// thread `callback` actually runs on (§5) — the same responsibility every
/// callback-registration API surveyed in § Context (libusb, PortAudio,
/// CoreAudio) already places on its own caller; this is not a new obligation
/// invented for this crate.
struct CallbackTarget {
    callback: mediaway_device_hotplug_callback_fn,
    user_data: *mut c_void,
}
unsafe impl Send for CallbackTarget {}
```

**Open, flagged-not-proven precondition**: this shape requires
`WindowsDeviceHotplug: Send` (to move the trait object into the bridging
thread). `hotplug.rs`'s own module doc strongly implies this already holds —
`IMMDeviceEnumerator`/`IMMNotificationClient` are documented "free-threaded/
agile COM objects," and the type is already designed to have `poll_event`/
`close` called from a different thread than `open` — but this is a
design-only ADR (no implementation, per task scope); actually compiling `Arc<Mutex<Box<dyn
DeviceHotplug + Send>>>` against the real `WindowsDeviceHotplug` is a concrete
Stage 1 implementation checkpoint (`docs/roadmap.md`), not asserted as already
verified here.

### 9. Feature flag

`mediaway-device-ffi/Cargo.toml` gains a third feature, symmetric with the
existing `video`/`audio` split (`adr/0001-capture-c-abi.md` §11):

```toml
[features]
default = ["video", "audio", "hotplug"]
video = []
audio = []
hotplug = []
```

Default-on for consistency with the existing precedent (both shipped
capabilities are already default-on; a consumer who wants a minimal build
already has to opt *out* via `default-features = false` regardless of how many
default features exist). No new dependency — the bridging thread uses only
`std::thread`/`std::sync`, already implicitly available.

### 10. Header authoring

Hand-write the addition into the existing `include/mediaway/device.h`,
consistent with this crate's **current actual state**:
[ADR-0016](../../../docs/adr/0016-cbindgen-ffi-headers.md) decided to adopt
`cbindgen` for this crate specifically, but per this crate's own
`docs/roadmap.md`, that migration has **not** executed yet (a documented
sequencing gap — the hand-written header shipped before ADR-0016 concluded).
Re-litigating that migration is out of scope here; this addition follows
whatever the header's current form is and is expected to be swept into the
`cbindgen` migration whenever that lands, same as the rest of the file.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Poll-only (a) | Forces every binding author, including ones in languages with excellent native-callback support (C#, Go, Kotlin, Swift), to hand-roll a polling/blocking-wait loop for a capability real C libraries (libusb, PortAudio, CoreAudio) already solve with a callback; strictly less capable than (c) for zero benefit — poll stays available under (c) anyway. |
| Callback-only (b) | Breaks GIL-based script runtimes' pragmatic ctypes/cffi-ABI-mode path (§5) without a fallback, and removes the simplest possible "just tick my own loop" integration a game engine or a test harness might prefer even in a language with native callback support. |
| Reach the raw `IMMNotificationClient` COM thread directly (downcast past the trait object) | Would give true zero-latency push, but requires breaking the `Box<dyn DeviceHotplug>` genericity this crate already committed to (`adr/0001-capture-c-abi.md` §2) for a Windows-only shortcut, and reaches into `mediaway-device-windows`'s private `NotificationSink` — a much larger, backend-specific redesign for a latency win (50ms bound, §3) that is negligible for a human-scale hotplug/default-change event. |
| Let callback and poll share the queue concurrently (Mutex only, no mode exclusivity) | Fixes the data race but not the deeper semantic bug: one logical event stream would be nondeterministically split between two consumers (§4) — a caller could never reliably reason about "did my callback see this event, or did my poll call steal it." |
| A C union (`mediaway_device_event_kind_t` tag + `union { ... }` payload) instead of a flat struct | This crate already established "flat struct, fields unused per variant" for `mediaway_video_capture_config_t`/`mediaway_audio_capture_config_t` (`adr/0001-capture-c-abi.md` §5) — introducing this crate's first C union for one more tagged type would be inconsistent with zero benefit; `DeviceEvent`'s payload is small (one `DeviceKind` + one optional string) with nothing size-sensitive enough to justify overlapping storage. |
| A generic "device changed, call `poll_event` to see what" callback with no payload, mirroring some minimal notify-only APIs | Would force every callback consumer to immediately turn around and call `poll_event` anyway, defeating mode exclusivity (§4) and reintroducing the exact "who drains the queue" ambiguity this ADR's push mode exists to avoid; the borrowed-payload design (§2) costs nothing extra to deliver directly. |
| Add a second, error-reporting callback alongside the event callback | Real gap (§3), but a second function-pointer parameter is new surface beyond what was asked; deferred until a concrete caller need is shown, per "no features beyond the request." |

## Consequences

### Positive

- First concrete, reusable answer in this workspace to "how does an
  event-shaped Rust capability cross the C ABI" — future
  encoder/decoder-backend-lost or permission-state-changed FFI additions can
  cite § General principle directly instead of re-deriving push-vs-poll,
  mode exclusivity, and the thread-safety contract from scratch.
- Keeps `mediaway-device`'s sync-poll `DeviceHotplug` trait completely
  unchanged — the FFI-layer bridging thread is entirely additive, contained
  inside `mediaway-device-ffi`, no ripple into the sans-io-adjacent core.
- Refutes the strongest, most commonly repeated form of "GIL makes
  foreign-thread callbacks into Python unsafe" with primary-source citations
  (§5) rather than accepting or rejecting the claim on priors — while still
  landing on a concrete, sourced reason (packaging cost of the safe API-mode
  path vs. real documented crash risk on the simpler ABI-mode path) to keep
  poll as the pragmatic Python default.
- `mediaway_device_kind_t` (§6) is now available as a general-purpose
  `DeviceKind` mirror any future FFI surface needing the *general* enum (not
  a capability-narrowed subset) can reuse, rather than each minting its own.

### Negative / Trade-offs

- `HotplugHandle` is structurally heavier than `VideoCaptureHandle`/
  `AudioCaptureHandle` (§8: `Arc<AtomicBool>` + `Arc<Mutex<..>>` +
  an optional owned background thread) — a real, not cosmetic, deviation from
  ADR-0001's established handle shape, justified by a real new requirement
  (two threads touching one handle by design) that did not exist in any prior
  handle in this crate.
- Callback-mode delivery has a real, bounded added latency
  (`HOTPLUG_CALLBACK_POLL_INTERVAL = 50ms`) versus the underlying OS
  notification — not true zero-latency push, and this ADR is explicit that
  the task brief's "invoked from the OS's own arbitrary thread" framing
  needed correcting (§1) rather than silently building to a different,
  undocumented mechanism.
- A backend error surfacing during callback mode is only observable on the
  *next* explicit call the caller makes (§3) — no dedicated error callback in
  v1. A caller that only ever registers a callback and waits for process
  shutdown without calling `close`/`poll_event`/`unregister_callback` again
  will not learn about a mid-session failure promptly.
- Two more crate-local status variants (§7) — this crate's status enum is
  already independently numbered from its siblings (`adr/0001` §3), so this
  adds no new cross-crate fragmentation, but it is one more variant set a
  consumer linking this crate must handle.
- `docs/spec/c-ffi.md` still does not state the general
  "foreign-code-unwinding-into-Rust-via-a-callback is caller UB" rule this
  ADR relies on (§5 point 4) — stated here at crate scope for now; a
  workspace-wide spec addition is flagged, not made, by this ADR (§ Deferred).

## Deferred to a later ADR / explicit open questions

- **`docs/spec/c-ffi.md` addendum**: a general "a foreign callback must not
  unwind back into Rust; this is caller-enforced, not library-enforced"
  clarification belongs in the workspace-wide C-FFI spec, not just this
  crate's ADR — checked, and that spec is currently silent on this exact
  point (§5 point 4). Not added here (workspace-wide spec edits are out of
  this ADR's crate-local scope), but the gap is now named precisely instead
  of left implicit.
- **Configurable poll interval.** `HOTPLUG_CALLBACK_POLL_INTERVAL` is a fixed
  internal constant in v1 (§3); a `register_callback`-with-interval variant
  (or a separate setter) is a plausible follow-up if a real caller needs
  tighter latency or looser CPU usage, not designed here absent that need.
- **A second, error-reporting callback** for backend failures during callback
  mode (§3, § Alternatives) — real gap, not designed here; would need its own
  signature/registration story.
- **`WindowsDeviceHotplug: Send` verification** (§8) — implied by the type's
  own doc comments, not confirmed by compiling anything in this design-only
  pass; a concrete Stage 1 implementation checkpoint.
- **Promoting this ADR's § General principle to `docs/adr/`** (workspace-wide)
  once a second event-shaped `-ffi` capability actually lands and needs it —
  mirroring `docs/adr/0016-cbindgen-ffi-headers.md`'s own "wait for ≥2/3 real
  instances before generalizing" discipline; not done preemptively here at
  n=1 (hotplug is still the only concrete case).
- **`cbindgen` migration** for this crate's header (§10) — unaffected by this
  ADR, already tracked in `docs/roadmap.md`.

## General principle (for reuse by future event-shaped `mediaway-*-ffi` additions)

Any future `mediaway-*-ffi` capability that is event-shaped rather than
request/response-shaped (named candidates: encoder/decoder backend-lost,
permission-state changes) should follow this shape rather than re-deriving it:

1. **Keep the underlying Rust-level trait/type sync-poll.** Do not add
   `extern "C"` or callback machinery to sans-io/facade Rust cores
   (`docs/spec/c-ffi.md` design rule 2; AGENTS.md § C-FFI item 4). The C ABI
   layer, not the Rust core, is where push semantics get added.
2. **Offer both a poll-shaped function and a callback-registration pair**
   (`*_register_callback`/`*_unregister_callback`) on the same handle,
   implemented via an FFI-crate-owned bridging thread that calls the Rust
   poll method at a documented, bounded interval and invokes the caller's
   function pointer once per drained event, borrowed for the duration of the
   call only.
3. **Enforce exactly one active consumption mode per handle** — poll XOR
   callback, not both concurrently — because the underlying event source is a
   single-consumer stream; mixing modes silently splits delivery rather than
   merely racing (§4).
4. **Document, specifically and honestly, not generically**: (a) which
   thread the callback may run on (the FFI's own bridging thread — not
   necessarily the caller's thread, and not necessarily the platform
   backend's own raw OS-callback thread even when one exists, §1); (b) the
   added polling-interval latency versus true OS-level delivery, if any;
   (c) that the callback must not block or re-enter any function on the same
   handle; (d) that a foreign exception/panic unwinding back across the
   callback into Rust is the caller's responsibility to prevent, not this
   library's (§5).
5. **Extend, don't replace, this crate's `catch_unwind` + poisoned-handle
   convention** — the poisoned flag becomes shared (`Arc`-based) only when a
   background bridging thread is introduced (§8); handles with no such thread
   keep the simpler plain-`bool` shape from `adr/0001-capture-c-abi.md` §2/§8
   unchanged.
6. **For script/GIL-based runtimes (Python named specifically), recommend
   poll as the default**, not because foreign-thread callbacks are
   fundamentally GIL-unsafe (they are not, when using the runtime's own
   documented safe mechanism — §5 of this ADR's findings), but because the
   *safe, recommended* mechanism (`cffi`'s API-mode `extern "Python"`)
   requires a compiled extension-module build step most quick scripts avoid,
   while the simpler no-build-step path (`cffi`'s ABI-mode `ffi.callback()`)
   carries real, currently-documented platform-hardening crash risk on
   locked-down OSes. Verify this against the current documentation for the
   specific runtime in question rather than assuming it holds unchanged.

## References

- [`crates/mediaway-device/src/hotplug.rs`](../../mediaway-device/src/hotplug.rs) — `DeviceEvent`/`DeviceHotplug`, unchanged by this ADR
- [`crates/mediaway-device-windows/src/hotplug.rs`](../../mediaway-device-windows/src/hotplug.rs) — `WindowsDeviceHotplug`, real `IMMNotificationClient` backend, arbitrary-MTA-thread event origin (§1)
- [`crates/mediaway-device/adr/0005-device-selection.md`](../../mediaway-device/adr/0005-device-selection.md) § Hotplug — `DeviceEvent`/`DeviceHotplug` design, v1 audio-only scope
- [`crates/mediaway-device-ffi/adr/0001-capture-c-abi.md`](0001-capture-c-abi.md) — handle/status-enum/`catch_unwind`/header conventions this ADR extends (§§2,3,4,5,7,8,10 all cite it directly)
- [`crates/mediaway-common-ffi/adr/`](../../mediaway-common-ffi/adr) — `#[repr(C)]` value-type-mirror precedent (ADR-0015); not directly reused here (`DeviceKind`/`DeviceEvent` have no `mediaway-common` analog to unify against) but the same "mirror the real Rust type 1:1, distinct C name per crate" discipline is followed
- [`docs/spec/c-ffi.md`](../../../docs/spec/c-ffi.md), [`docs/adr/0004-c-ffi.md`](../../../docs/adr/0004-c-ffi.md) — workspace C-FFI design rules this ADR concretizes for the event-shaped case; confirmed silent on foreign-unwind-into-Rust (§5 point 4, § Deferred)
- [`docs/adr/0016-cbindgen-ffi-headers.md`](../../../docs/adr/0016-cbindgen-ffi-headers.md) — cbindgen adoption decided but not yet executed for this crate's header (§10)
- [`docs/conventions/error-handling.md`](../../../docs/conventions/error-handling.md) — status-enum/error-mapping conventions this ADR's new variants (§7) follow
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) — honest-latency/honest-blocking-cost documentation requirement (§§1,3,4)
- libusb hotplug API: <https://libusb.sourceforge.io/api-1.0/group__libusb__hotplug.html> — `libusb_hotplug_callback_fn`/`libusb_hotplug_register_callback` precedent (§ Context)
- Python `ctypes` callback functions: <https://docs.python.org/3/library/ctypes.html#callback-functions> — `CFUNCTYPE` releases the GIL during the call and creates a dummy Python thread state per invocation when called from a foreign thread (§5)
- CFFI `using.html`: <https://cffi.readthedocs.io/en/stable/using.html> — recommends `extern "Python"`/API mode over `ffi.callback()`/ABI mode; documents `ffi.callback()`'s platform-hardening crash risks (macOS hardened runtime, SELinux/PAX, systemd `MemoryDenyWriteExecute`) (§5)

**2026-07-31 addendum**: Accepted for implementation. No new external dependency
(the bridging thread uses only `std::thread`/`std::sync`, already implicitly
available) — the `cargo deny check` gate that blocked `mediaway-sw-opus`/
`mediaway-audio-apm`/`rtmp`'s promotion does not apply here. The open,
flagged-not-proven precondition in §8 (`WindowsDeviceHotplug: Send`) is a
concrete Stage 1 implementation checkpoint, not a design gap — implementation
must confirm it compiles before relying on it.

**2026-07-31 implementation addendum**: Implemented per this ADR — `HotplugHandle`
(§8), `mediaway_device_hotplug_open`/`close`/`register_callback`/
`unregister_callback`/`poll_event`/`event_free` (§2), the bridging thread + 50ms
poll interval (§3), mode exclusivity (§4), `mediaway_device_kind_t`/
`mediaway_device_event_t` (§6), the two new status variants (§7), and the hand-written
header addition (§10) — one real, confirmed deviation from what §8 flagged as open:

- **`WindowsDeviceHotplug: Send` does not compile**, confirmed by a real
  `cargo check -p mediaway-device-ffi --all-features` attempt (not merely left
  unverified as §8 anticipated might happen). The exact compiler error:
  `NonNull<c_void> cannot be sent between threads safely`, because
  `windows_core::unknown::IUnknown` (wrapped by both `IMMDeviceEnumerator` and
  `IMMNotificationClient`, both held inside `WindowsDeviceHotplug`'s private
  `HotplugSession`) holds a `NonNull<c_void>`, and `windows-core` (v0.62.2, this
  workspace's pinned version) does not implement `Send`/`Sync` for its interface
  wrapper types generically — regardless of whether the specific COM objects
  involved are actually agile (`hotplug.rs`'s own module doc claims
  `IMMDeviceEnumerator`/`#[implement]`-generated objects are free-threaded/agile;
  that claim is about the underlying COM contract, not reflected anywhere in
  `windows-core`'s Rust-level `Send`/`Sync` impls).
- Per this task's explicit instruction, **no workaround was attempted**: no
  `unsafe impl Send for WindowsDeviceHotplug` was added (in `mediaway-device-windows`
  or via a wrapper newtype inside this crate), and `mediaway-device-windows` was not
  touched at all (out of this crate's scope regardless). Making
  `WindowsDeviceHotplug: Send` hold — with a real, reviewed `# Safety` justification
  grounded in the specific COM agility guarantees `hotplug.rs` already documents — is
  `mediaway-device-windows`'s own call to make, as a follow-up in that crate.
- **Consequence**: `mediaway-device-ffi/src/hotplug.rs::open_hotplug` does not
  reference `WindowsDeviceHotplug` at all and returns `CaptureError::NoBackend`
  unconditionally on every platform, including Windows, documented inline at the
  function definition. Every other function in this ADR's surface (`register_callback`/
  `unregister_callback`/`poll_event`/`close`/`event_free`, mode exclusivity, the
  bridging-thread mechanism, the status/type mirrors, the header) is implemented and
  compiles/passes its own tests against a **test-only mock** `DeviceHotplug`
  (`hotplug_tests.rs`) exactly as designed — only the real Windows backend wiring is
  blocked. `mediaway_device_hotplug_open` on any real `kinds` value therefore returns
  `MEDIAWAY_DEVICE_STATUS_NO_BACKEND` on this pass, the same honest "real capability,
  not yet reachable from C" shape `adr/0001-capture-c-abi.md` already established for
  Screen/Window capture (§ Finding 2) — not silently "successful but broken."
- Tracked as a follow-up in `docs/roadmap.md`, not resolved here.

**2026-07-31 Send-question resolution**: the `WindowsDeviceHotplug: Send`
question this ADR's §8 and the implementation addendum above left open is
now answered empirically, not just left unverified: **it is unsound.**
`mediaway-device-windows/src/hotplug.rs`'s own module doc previously claimed
"`MMDeviceEnumerator` instances are documented free-threaded/agile COM
objects" without a cited source. A real `QueryInterface(IAgileObject)`
against a live `MMDeviceEnumerator` instance
(`mediaway-device-windows::lib_tests::mmdevice_enumerator_does_not_implement_iagileobject_or_skip`,
now a permanent regression test) fails with `E_NOINTERFACE` on real
hardware. The registry's `ThreadingModel=Both` for that CLSID is real but
does not imply `IAgileObject` support — it only governs which apartment the
class factory may activate the object in, not whether an already-created
instance may cross threads without marshaling. Only the locally-implemented
`NotificationSink` (`client` field) is confirmed agile, via
`windows-implement`'s own macro default (verified against that crate's
source) — the `enumerator` field is not, and `HotplugSession` holds both, so
the struct as a whole cannot be `Send`.

**Consequence for §8's handle shape**: `Arc<Mutex<Box<dyn DeviceHotplug +
Send>>>` cannot ever be satisfied by `WindowsDeviceHotplug` as designed. The
bridging-thread mechanism (§3) as specified — open the handle on the
caller's thread, later move the already-open trait object into a new thread
for callback mode — is not just unimplemented for Windows, it is
**unimplementable** for this backend without either (a) a `#[repr(transparent)]`
newtype wrapper the platform crate itself reviews and stamps
`unsafe impl Send` on with a real, narrower SAFETY argument than "it's
agile" (none identified so far — COM apartment rules do not offer one), or
(b) restructuring so the bridging thread constructs and exclusively owns the
`WindowsDeviceHotplug`/COM objects itself from the moment a callback is
first requested, communicating only plain-data `DeviceEvent`s back across a
channel — never moving the COM-holding value across a thread boundary at
all. Option (b) is a real design change to §3/§8, not authorized by this
addendum; flagged here as the concrete next step, to be decided before any
further Windows-backend wiring work on this ADR.

## Revision (2026-07-31): lazy, thread-owned construction — supersedes §3/§8's handle shape

Chose option (b) above, in a form with **no performance regression** versus
either the original design or today's other handles in this crate, and no
channel at all (simpler than option (b)'s own framing suggested). This
section is authoritative over §3's pseudocode and §8's `HotplugHandle`
shape; those sections stay for their design-rationale value (why poll/push
share one queue, why exclusivity matters, the thread-safety contract) but
their concrete Rust shapes are replaced by what follows.

**The core idea**: never construct `WindowsDeviceHotplug` until the moment
something concrete needs it, and let *that* call site's own thread — the
caller's thread for pull mode, the bridging thread's own body for push mode
— be the one that constructs it. Because the object then never moves
between threads after construction, `Send` is never required for it at all.
`Arc<Mutex<Box<dyn DeviceHotplug + Send>>>` is dropped entirely; no wrapper
type, no `unsafe impl Send`, no channel.

```rust
enum HotplugBackend {
    /// `open()` only validated `kinds` — nothing COM-side has happened yet.
    Idle { kinds: Vec<DeviceKind> },
    /// Constructed directly on whichever thread made the first `poll_event()`
    /// call. Thread-confined by convention from that point on — the same
    /// contract `adr/0001-capture-c-abi.md` §9 already documents for every
    /// other handle in this crate (moving between threads is fine; two
    /// threads touching it at once without external sync is a data race the
    /// caller must avoid, not something this crate defends against here).
    Pulling(Box<dyn DeviceHotplug>),
    /// Owned exclusively by the bridging thread's own stack for its entire
    /// lifetime — constructed there, used there, closed there. The handle's
    /// own struct never holds it.
    Pushing(CallbackBridge),
}

struct HotplugHandle {
    /// Still `Arc<AtomicBool>` (§8's original reasoning unchanged): the
    /// bridging thread must be able to poison the handle itself while the
    /// caller's thread independently reads it.
    poisoned: Arc<AtomicBool>,
    backend: HotplugBackend,
}

struct CallbackBridge {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}
```

`poll_event` on an `Idle` handle constructs `WindowsDeviceHotplug` right
there (via the same `open_hotplug` dispatch this crate already has),
transitions to `Pulling`, then serves the call — one-time construction cost
on first use, not per call, and it is the *same* construction work `open()`
used to do eagerly; total work is identical, just shifted later. Every
subsequent `poll_event` on a `Pulling` handle is the direct, in-process call
this crate already had — **zero regression**, since only one thread ever
touches a `Pulling` handle by convention, matching every other handle here.

`register_callback` on an `Idle` handle spawns the bridging thread and moves
`kinds: Vec<DeviceKind>` into its closure (`Vec` of a `Copy` enum — trivially
`Send`, no COM object crosses the boundary). That thread constructs
`WindowsDeviceHotplug` itself, on itself, then loops
`poll_event → invoke callback → sleep(HOTPLUG_CALLBACK_POLL_INTERVAL)`
exactly as §3 already described, `catch_unwind`-wrapped per iteration,
poisoning `poisoned` (the same `Arc<AtomicBool>` — cloned in, `// clone: Arc
share`) on a fatal error, and closing the object itself before the thread
exits. **No new thread beyond what §3 already specified** — the bridging
thread was always going to exist for callback delivery; this only moves
*where* construction happens, into a thread that was being spawned anyway.
No channel is introduced: the bridging thread is fully self-contained
(construct, loop, invoke, close), coordinating with the outside only through
`stop`/`poisoned`, which §8 already had.

`register_callback` on an already-`Pulling` handle (a caller polled first,
then decided to switch to push mode) drops/closes the `Pulling` object
first — safe, since by the thread-confinement convention above, whatever
thread is calling `register_callback` now is the same one that owns the
`Pulling` object — then proceeds as the `Idle` case. `poll_event` on an
already-`Pushing` handle still returns `CallbackModeActive` unchanged (§4);
`unregister_callback` still joins the bridging thread (which closes its own
object as its last act before exiting) and returns the handle to `Idle {
kinds }`, ready to be lazily reconstructed by whichever mode touches it
next.

**One real, accepted behavioral difference from eager construction, stated
plainly (`docs/spec/caveats-and-clarity.md`)**: `open()` no longer registers
the real `IMMNotificationClient` callback immediately — that now happens on
first `poll_event`/`register_callback`. A hotplug event occurring in the
(typically sub-millisecond) gap between `open()` returning and that first
call could be missed, where eager construction would have caught it. Hotplug
events are already framed throughout this ADR as rare and user-driven (plug/
unplug, default-device switch), not a stream a caller needs to catch from
the very first instant `open()` returns, so this is accepted, not treated as
a regression needing a workaround.

**Deferred**: implementing this shape and re-attempting `open_hotplug`'s
Windows dispatch is not done by this ADR revision — a concrete follow-up
implementation task, same as the original 2026-07-31 implementation
addendum's scope, now unblocked by this design.

## Implementation addendum (2026-07-31): lazy-construction shape built; Windows dispatch wired in; real, unresolved close() crash found

Implemented this section's design in `hotplug.rs`, with two deliberate, documented
deviations from its literal sketch and one important new empirical finding.

**Shape actually built** — `HotplugBackend`/`HotplugHandle` match this section's
sketch for the two types this task was told to keep verbatim
(`HotplugHandle { poisoned: Arc<AtomicBool>, backend: HotplugBackend }`,
`HotplugBackend::{Idle, Pulling, Pushing}`, same variant names/arity for `Idle`/
`Pushing`). Two additions were necessary to make the state machine actually work:

- **`HotplugBackend::Pulling` and `CallbackBridge` both also carry `kinds: Vec<DeviceKind>`**,
  not just `Idle`. The sketch above has `Pulling(Box<dyn DeviceHotplug>)` and
  `CallbackBridge { stop, thread }` with no `kinds` anywhere outside `Idle`. That
  leaves a real gap: `register_callback` switching mode while `Pulling` (§ "proceeds
  as the `Idle` case") and `unregister_callback` returning to `Idle { kinds }` both
  need to know `kinds` *after* the handle has already left the one variant that held
  it. Since `HotplugHandle` itself was kept to exactly the two fields specified
  (no third top-level `kinds` field), the smallest fix consistent with that
  constraint is carrying `kinds` inside `Pulling`/`CallbackBridge` too — each variant
  that might need to (re)construct later keeps its own copy. Cheap (`Vec<DeviceKind>`
  of `Copy` elements) and documented at each field.
- **`poll_event`/`register_callback`'s shared "on `Idle`, construct" logic is factored
  into private `poll_event_impl`/`register_callback_impl` functions taking `construct:
  impl Fn.../FnOnce...` as a parameter**, rather than hardcoding `open_hotplug` inline.
  Production call sites (the `extern "C"` functions) always pass `open_hotplug`; tests
  inject a mock. This was necessary because the lazy-construction design means the
  real backend is built fresh on *every* Idle -> {Pulling,Pushing} transition, not
  once at `open()` time — so a test can no longer just pre-build a mock and expect the
  public C ABI functions to keep reusing it once mode-switching is exercised.
  `hotplug_tests.rs` was substantially rewritten around this (`handle_idle`,
  `mock_constructor`, `construct_must_not_be_called`, calling `poll_event_impl`/
  `register_callback_impl`/`join_callback` directly for callback-mode tests); tests
  that only need an already-constructed backend still go through the public
  `mediaway_device_hotplug_*` functions unchanged (`handle_with` now builds a
  `Pulling` handle directly).

**Windows dispatch wired in**: `open_hotplug` now calls
`mediaway_device_windows::WindowsDeviceHotplug::open(kinds)` under `#[cfg(windows)]`
(boxed as `Box<dyn DeviceHotplug>`, no `+ Send` needed — confirmed compiling, since the
object never crosses a thread boundary under this design). The Linux arm is unchanged
(`NoBackend` — no backend exists there). 18 sibling unit tests
(`hotplug_tests.rs`) pass against a mock `DeviceHotplug`, including a new
`mode_switch_idle_pushing_idle_pulling_pushing` regression covering the exact
Idle -> Pushing -> Idle -> Pulling -> Pushing sequence this revision's own worked
example describes, and `idle_handle_does_not_construct_until_first_touch` asserting
the lazy-construction property directly. `cargo clippy -p mediaway-device-ffi
--all-features --all-targets -- -D warnings` and `cargo clippy --workspace
--all-targets --all-features -- -D warnings` are both clean.

**New finding: `mediaway_device_hotplug_open` -> `poll_event` against the real
`WindowsDeviceHotplug` now genuinely succeed** (confirmed on real hardware,
`hotplug_tests.rs::open_hotplug_real_windows_backend_wires_through_or_skip`) — this is
the dispatch-wiring fix this addendum set out to make, and it works. **But calling
`close()` on that real, successfully-constructed handle crashes the process with
`STATUS_ACCESS_VIOLATION`**, reproduced reliably (3/3 runs) two ways: through this
crate's C ABI, and by bypassing every line this crate added entirely — calling
`mediaway_device_windows::WindowsDeviceHotplug::open`/`poll_event`/`close` directly,
concrete type, no `Box<dyn DeviceHotplug>`, no raw pointers, no `catch_unwind`. The
*identical* call sequence (including a 3x-poll-with-20ms-sleep variant, matching
`mediaway-device-windows`'s own timing) passes reliably (5/5 runs) when run from
`mediaway-device-windows`'s own test binary
(`lib_tests::open_hotplug_mic_loopback_poll_or_skip`). Isolated (via `std::mem::forget`
in place of `close()`) to specifically `WindowsDeviceHotplug::close()`'s own body —
skipping `close()`/`Drop` entirely avoids the crash. `windows-core` is pinned to a
single version (0.62.2) throughout the dependency graph (`cargo tree -i windows-core`
checked — no version skew), so this is not an ABI-mismatch-via-duplicate-dependency
issue; the root cause remains unidentified and appears tied to something about
`mediaway-device-ffi`'s specific binary/link environment interacting with this COM
teardown path, not to any code this task added or changed.

`STATUS_ACCESS_VIOLATION` is a hardware fault, not a Rust panic — `catch_unwind` cannot
catch it. Per this task's explicit instruction not to ship a hardware test that could
cause disruptive interaction, `open_hotplug_real_windows_backend_wires_through_or_skip`
deliberately never calls `close()`/lets `Drop` run on a successfully-constructed real
handle (leaks it via `std::mem::forget`, acceptable for a short-lived test process) —
the alternative would make `cargo test -p mediaway-device-ffi --all-features` reliably
crash the whole test process on any Windows machine with a working default audio
endpoint. **Fixing the root cause is out of this task's scope** (`mediaway-device-windows`
is not touched by this pass, per instruction), but it means the real Windows hotplug
backend, as wired in here, is not yet safe to `close()` in a real consuming
application either — a concrete, named follow-up for `mediaway-device-windows`,
tracked in this crate's `docs/roadmap.md`.

## Implementation addendum (2026-07-31): close() crash root-caused and fixed in `mediaway-device-windows`

Root-caused via a real Win32 SEH exception filter (`SetUnhandledExceptionFilter`),
registered temporarily in a throwaway diagnostic test in this crate to identify which
module the fault address belonged to, since `STATUS_ACCESS_VIOLATION` gives no Rust
backtrace and no interactive debugger was available. Resolving the fault RVA against the
test binary's own PDB (`llvm-symbolizer --obj=<exe> --relative-address <rva>`) pinpointed
the crash inside `windows::Win32::Media::Audio::IMMDeviceEnumerator::UnregisterEndpointNotificationCallback`
itself — a real vtable call fault, not this crate's code.

**Root cause**: `WindowsDeviceHotplug::open()` (in `mediaway-device-windows/src/hotplug.rs`)
called `CoInitializeEx` in a function-local scope whose `ComGuard` ran `CoUninitialize()`
*before `open()` returned*, while still storing the `IMMDeviceEnumerator`/
`IMMNotificationClient` obtained under that now-torn-down apartment inside the returned
`HotplugSession`. `close()` later called `UnregisterEndpointNotificationCallback` through
that stale `enumerator` — a real, reproduced instance of the exact undefined behavior
COM's own documentation warns `CoUninitialize` causes for outstanding interface pointers.
It only failed to reproduce in `mediaway-device-windows`'s own hardware test because that
test's calling thread happened to have an unrelated, pre-existing `CoInitializeEx`
refcount from elsewhere, accidentally keeping the apartment alive — not because the
per-call-scope design was actually sound. This was independently confirmed by first
ruling out a simpler theory (test-process concurrency): running the crashing sequence
via `cargo test ... --test-threads=1` in full isolation reproduced the crash identically,
proving it was never a concurrent-test artifact.

**Fix** (`mediaway-device-windows/src/hotplug.rs`): `HotplugSession` now owns the
`ComGuard` itself — `open()` moves it in instead of letting it drop at the end of the
function, so the same COM apartment initialization spans from a successful `open()`
through `close()`'s `UnregisterEndpointNotificationCallback` call, `CoUninitialize()`
only running when `HotplugSession` itself is dropped, strictly after that call. See that
file's type-level doc for the full write-up, including the real (pre-existing, not newly
introduced) narrower thread-affinity requirement this makes explicit: `open()`, every
`poll_event()`, and `close()`/`Drop` must run on the *same* thread for a given instance —
already satisfied by this crate's own lazy-construction design (the Revision section
above), which keeps a `WindowsDeviceHotplug` confined to one thread for its whole life
for an unrelated reason.

**Verified**: `open_hotplug_real_windows_backend_wires_through_or_skip`
(`hotplug_tests.rs`) no longer leaks the handle — it now calls `close()` on the real,
successfully-constructed backend and asserts `MEDIAWAY_DEVICE_STATUS_OK`, confirmed
passing in isolation (`--test-threads=1`) as well as in the default parallel suite.
`mediaway-device-windows`'s own `open_hotplug_mic_loopback_poll_or_skip` continues to
pass. `cargo fmt`/`cargo clippy --all-features --all-targets -- -D warnings` clean for
both crates, and `cargo clippy --workspace --all-targets --all-features -- -D warnings`
clean workspace-wide. The real Windows hotplug backend is now genuinely safe to
`close()`/drop — the follow-up this addendum's predecessor named is resolved.

ADRs are **English**. Numbering is local to this `adr/` folder.
