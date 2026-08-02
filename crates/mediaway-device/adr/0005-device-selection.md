# ADR-0005: Device selection — `DeviceId`, `Select`, enumeration, and hotplug (audio v1)

- **Status**: Accepted — `DeviceId`/`Select`/`DeviceInfo` types, the breaking field
  changes, `mediaway-device-windows`'s `enumerate` bodies, `DeviceLost` wiring, and
  `DeviceHotplug`'s Windows backend (`WindowsDeviceHotplug` / `IMMNotificationClient`,
  Microphone/Loopback only per § Hotplug) are all implemented and hardware-verified;
  see `docs/ai/wiki/device/selection.md` for current status.
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device`

## Context

Every capture/playback config today selects a device with a raw integer:

- `AudioCaptureSource::Microphone { device_index: u32 }` / `Loopback { device_index: u32 }`
  ([`audio.rs`](../src/audio.rs))
- `AudioPlaybackConfig.device_index: u32` ([`playback.rs`](../src/playback.rs), ADR-0004,
  added this same session)
- `CaptureSource::Camera { device: usize }` ([`video.rs`](../src/video.rs)), left
  deliberately untyped by
  [ADR-0013](../../../docs/adr/0013-native-handle-and-gpu-device.md) pending this decision
  (see `docs/ai/wiki/device/camera-device-handle.md`)
- `CaptureSource::Screen { output_index: u32 }` — a DXGI output ordinal on the adapter that
  owns the caller's `gpu_device`

Every Windows backend today **rejects any non-zero index outright**
(`wasapi.rs::open_wasapi_client`, `wasapi_playback.rs::open_wasapi_render_client`) — only the
OS default endpoint is reachable. `camera.rs::activate_for_index` is the sole exception: it
already resolves a real ordinal against `MFEnumDeviceSources`. None of this exposes a stable
identity a caller can persist, log, or show a picker for — the classic PortAudio integer-index
failure mode: an index silently means a different physical device after a hotplug/replug
re-orders the OS enumeration. There is also no enumeration API returning device names/IDs
anywhere in the public surface, and no device-change (hotplug/default-changed) event surface at
all.

Two backend files already contain real, unused precedent for what a proper answer looks like:

- `camera.rs` has `#[allow(dead_code)] fn enumerate_camera_names()` and `fn friendly_name()` —
  a full `MFEnumDeviceSources` + `MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME` enumeration path,
  written but never wired to a public API.
- `capabilities.rs::endpoint_support` already calls `IMMDeviceEnumerator::EnumAudioEndpoints`
  read-only, confirming that endpoint enumeration is cheap and already linked; it just doesn't
  surface names or persistent IDs.

This ADR decides `DeviceId`'s shape, an enumeration surface, a `Select`-based replacement for
every raw index/ordinal field above, and how much hotplug is realistically buildable in v1 —
grounded in the real Windows APIs this crate already links, not a platform-agnostic guess.

## Decision

> Replace every raw device index/ordinal field with `Select` (`Default | Id(DeviceId) |
> NameContains(String)`). Add `DeviceId` (opaque, backend-tagged, `Display`/`FromStr`),
> `DeviceInfo` (owned enumeration snapshot), and a `DeviceHotplug` sync-poll trait scoped to
> **audio only** in v1. Breaking change, applied now (pre-1.0, `AudioPlaybackConfig` is new
> this session).

### `DeviceId` — opaque newtype over a backend-tagged repr, not per-kind types

```rust
// mediaway-device/src/device_id.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceId(DeviceIdRepr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
enum DeviceIdRepr {
    /// `IMMDevice::GetId()` — persistent WASAPI endpoint ID string
    /// (mic / render / loopback endpoints).
    Wasapi(String),
    /// `MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK` — persistent
    /// Media Foundation camera symbolic link (same attribute family already
    /// queried for `MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME` in `camera.rs`).
    MediaFoundation(String),
    /// `DXGI_OUTPUT_DESC.DeviceName` (e.g. `"\\\\.\\DISPLAY1"`) — see Caveats.
    DxgiOutput(String),
}

impl DeviceId {
    #[must_use] pub fn from_wasapi_endpoint_id(id: impl Into<String>) -> Self { .. }
    #[must_use] pub fn from_media_foundation_symbolic_link(link: impl Into<String>) -> Self { .. }
    #[must_use] pub fn from_dxgi_output_device_name(name: impl Into<String>) -> Self { .. }

    #[must_use] pub fn as_wasapi_endpoint_id(&self) -> Option<&str> { .. }
    #[must_use] pub fn as_media_foundation_symbolic_link(&self) -> Option<&str> { .. }
    #[must_use] pub fn as_dxgi_output_device_name(&self) -> Option<&str> { .. }
}

impl std::fmt::Display for DeviceId { /* "wasapi:<id>" / "mf-symlink:<link>" / "dxgi-output:<name>" */ }
impl std::str::FromStr for DeviceId { /* parses the tagged prefix back */ }
```

**One type, tagged internally — not three separate `AudioDeviceId`/`CameraDeviceId`/
`ScreenDeviceId` types.** The underlying identity really is a different string shape per kind
(confirmed above: WASAPI endpoint ID, MF symbolic link, DXGI device name are unrelated
formats with unrelated stability guarantees), so the tag is real, not decorative — same
justification `GpuDeviceHandle`/`GpuBufferHandle` already use for keeping platform detail as
explicit variants inside one enum rather than as a family of parallel types
([`api-layers.md`](../../../docs/spec/api-layers.md) rule 4). A single type lets `Select::Id`
be reused unmodified across `AudioCaptureConfig`, `AudioPlaybackConfig`, and
`VideoCaptureConfig` without generics; a wrong-kind `DeviceId` (e.g. a Wasapi ID passed to
`CaptureSource::Camera`) is rejected the same way every other source/variant mismatch already
is today — `CaptureError::Unsupported` at `open()`, not a compile-time distinction.
`DeviceIdRepr` stays private; construction only through the `from_*` associated functions
(needed because `#[non_exhaustive]` blocks downstream-crate variant construction — backend
crates call the constructors, not the enum directly), matching `NativeHandle::new`'s
constructor-over-raw-field precedent (ADR-0013).

**No `unsafe`.** `DeviceId` holds owned `String`s, not pointers — it can live in this
`#![forbid(unsafe_code)]` facade crate exactly like `DeviceKind`/`Support` already do, unlike
`NativeHandle`/`GpuBufferHandle` which wrap real pointer bits.

#### Caveat: identity stability is not uniform across kinds

- **WASAPI endpoint ID**: OS-documented persistent identity, stable across unplug/replug of
  the same physical endpoint, across reboots.
- **MF symbolic link**: persistent per USB port + driver instance; moving a webcam to a
  different physical port yields a different symbolic link (a real, known Windows quirk, not
  a Mediaway limitation — same caveat V4L2 `/dev/v4l/by-id/` paths carry on Linux).
- **DXGI `DeviceName`**: **session-scoped, not a persistent hardware identity** — GDI device
  names (`\\.\DISPLAY1`, `\\.\DISPLAY2`, …) can be reassigned when the display topology
  changes (monitor unplugged/replugged, docking/undocking). Weaker than the other two;
  documented on the `DxgiOutput` variant's rustdoc rather than silently presented as
  equally durable identity (per
  [`caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md)). A stronger
  EDID/`SetupAPI`-backed monitor identity is deferred (see Deferred).

### `Select` — data enum, owned (not borrowed)

```rust
// mediaway-device/src/device_id.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Select {
    /// OS default for this kind (existing behavior — `device_index == 0` today).
    #[default]
    Default,
    /// A specific device by stable `DeviceId` (from `enumerate`, or persisted/restored).
    Id(DeviceId),
    /// First device (per `enumerate`'s returned order) whose name contains `needle`,
    /// case-insensitively. Backend-defined enumeration order — not a promised stable
    /// global sort (same honesty already applied to `camera.rs`'s ordinal note).
    NameContains(String),
}
```

**Owned `DeviceId`/`String`, not `&DeviceId`/`&str` as the independent ideal-sketch input
suggested.** Every existing backend (`wasapi.rs`, `wasapi_playback.rs`, `camera.rs`) opens its
session by `thread::Builder::spawn(move || …)`, moving config-derived values into a worker
thread that outlives the caller's stack frame — a borrowed `Select<'a>` could not satisfy
`thread::spawn`'s `'static` bound without every config struct in this crate growing a lifetime
parameter, a far larger ergonomics regression than the sketch's borrow-to-avoid-a-clone intent
was worth. This is a concrete, code-grounded rejection, not a style preference.

**No `Select::Index(u32)` variant**, even though a raw ordinal is genuinely meaningful for
`Screen` ("capture display 2"). Kept `Select` uniform across every kind instead; `DeviceInfo`
(below) carries `ordinal: u32` so a caller who wants "the 3rd enumerated device of this kind"
gets it via `enumerate(kind)?[2].id` → `Select::Id(..)`, at the cost of one enumeration round
trip instead of a bare index. Consistency across kinds outweighs saving that round trip.

### Enumeration — `DeviceInfo`, reusing `DeviceKind` (ADR-0003)

```rust
// mediaway-device/src/enumeration.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub kind: DeviceKind,     // reused from capability.rs (ADR-0003) — no new parallel enum
    pub name: String,
    pub is_default: bool,
    /// Backend-defined position in this enumeration call's result order (0-based).
    /// Not guaranteed stable across separate calls/hotplugs — a convenience for
    /// "pick the Nth", not a persistent identity (use `id` for that).
    pub ordinal: u32,
}
```

`Clone + Send + 'static` (plain owned data — no borrows, no platform handle) satisfies the
ideal-sketch's "detached snapshot" requirement without extra work.

**`name: String`, no `Name::Restricted` variant.** The sketch proposed an infallible
permission-gated name type. Checked against real Windows behavior: `EnumAudioEndpoints` +
`IPropertyStore`/`PKEY_Device_FriendlyName` and `MFEnumDeviceSources` +
`MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME` do **not** require microphone/camera consent for a
Win32 desktop app — consent only gates opening the stream (`IAudioClient::Initialize`/
`Start`), confirmed by `capabilities.rs::endpoint_support` already calling
`EnumAudioEndpoints` with no consent handling. No backend in this workspace has a real
name-restriction case today; adding `Name::Restricted` now would be speculative abstraction
for a condition nothing exercises ("no abstractions for one-off code"). Documented as a
**deferred** extension point instead (see Deferred) — a future portal-mediated backend that
genuinely gates names would be the actual trigger to add it.

**`is_default` semantics are honest per kind, not uniformly guessed:**

| Kind | `is_default` computed from |
|------|----------------------------|
| Microphone / Loopback | Compare `id` to `GetDefaultAudioEndpoint(eCapture/eRender, eConsole)`'s `GetId()` — real, cheap |
| ProcessLoopback | N/A — not enumerable at all (see below) |
| Camera | Always `false` — Windows/Media Foundation has no "default camera" concept; guessing would be dishonest |
| Screen | `true` only for `ordinal == 0` (same "0 = primary" convention `VideoCaptureConfig::screen` already documents) — not a real EDID-based primary-monitor query (deferred) |
| Window | N/A — `enumerate(Window)` is out of scope entirely, see below |

**`enumerate(DeviceKind::ProcessLoopback)` returns `Err(CaptureError::Unsupported)`, never an
empty `Vec`.** Process-loopback targets are parameterized by PID at open time, not an OS
device list — an empty `Vec` would read as "no processes are producing audio," which is a
different and false claim.

**Free-function shape, not a `Devices` struct/trait — matching ADR-0003's precedent exactly.**
`mediaway-device` declares only the vocabulary types (`DeviceId`, `Select`, `DeviceInfo`); each
platform crate exposes its own `enumerate(kind: DeviceKind) -> Result<Vec<DeviceInfo>,
CaptureError>` free function (e.g. `mediaway-device-windows::enumeration::enumerate`),
dispatched later by `mediaway_pipeline::platform` — the same boundary already established for
`support`/`request_permission`. ADR-0003 already rejected a stateful capability trait/object
for exactly this reason ("no dynamic-dispatch use case exists… an abstraction with no caller
that needs it") and that reasoning applies unchanged to plain enumeration. **This ADR does not
implement `enumerate` bodies or pipeline wiring** — it fixes the contract; backend
implementation is follow-up work (see Deferred), consistent with "no large coding before
approval."

`enumerate(Window)` is **not part of this ADR** — a window is not a persistent device; there is
no OS-level "list of windows with stable identity" the way there is a device list (`EnumWindows`
gives ephemeral HWNDs that change identity every process restart). `CaptureSource::Window`
keeps `NativeHandle` exactly as ADR-0013 left it.

### Breaking field changes

| Type | Before | After |
|------|--------|-------|
| `AudioCaptureSource::Microphone` | `{ device_index: u32 }` | `{ select: Select }` |
| `AudioCaptureSource::Loopback` | `{ device_index: u32 }` | `{ select: Select }` |
| `AudioCaptureSource::ProcessLoopback` | unchanged | unchanged (never index-based) |
| `AudioPlaybackConfig` | `device_index: u32` | `select: Select` |
| `CaptureSource::Camera` | `{ device: usize }` | `{ select: Select }` |
| `CaptureSource::Screen` | `{ output_index: u32 }` | `{ select: Select }` |
| `CaptureSource::Window` | `{ window: NativeHandle }` | **unchanged** — not a device |

Applied now, not as a parallel additive path: `AudioPlaybackConfig` was added this same
session (ADR-0004), so there are no external callers to break; `AudioCaptureConfig` and
`CaptureSource` are pre-1.0 and every existing Windows backend already hard-rejects any index
other than `0` (`Unsupported`), so no real caller-observable capability regresses — only the
type of "give me the default" changes shape (`0` → `Select::Default`). Keeping a parallel
`device_index` field alongside `select` would let both disagree and force every backend to
define (and document) a precedence rule for no real benefit — status.md's pre-1.0 API-churn
allowance (already exercised the same way by ADR-0013's `d3d11_device: usize` →
`gpu_device: Option<GpuDeviceHandle>` breaking change) makes the clean break strictly simpler.

`Screen` is included even though the task's grounding material focused on audio/camera: leaving
it on a bare `output_index: u32` while every other kind gains `Select` would preserve exactly
the antipattern this ADR exists to fix on the one remaining kind, for no principled reason —
DXGI output enumeration is real and already linked (`enum_output`/`EnumAdapters1`/
`EnumOutputs`). **Scoping constraint preserved as-is**: `Select` for `Screen` only resolves
among the outputs of the adapter that owns `config.gpu_device` (today's `dxgi_device.GetAdapter()`
scoping) — a `Select::Id` naming an output on a *different* adapter is `CaptureError::InvalidInput`,
matching the existing device-vs-adapter-ownership contract, not a global cross-adapter search.

### Hotplug — audio only in v1, everything else deferred and named why

```rust
// mediaway-device/src/hotplug.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeviceEvent {
    Added { id: DeviceId, kind: DeviceKind },
    Removed { id: DeviceId, kind: DeviceKind },
    DefaultChanged { kind: DeviceKind, id: Option<DeviceId> },
    StateChanged { id: DeviceId, kind: DeviceKind },
}

/// Sync-poll device-change notifications, mirroring `AudioCapture::poll_frame`'s
/// idle convention (`Ok(None)` = nothing pending) per
/// `docs/spec/async-and-streaming.md`'s sync/poll policy for platform sessions.
pub trait DeviceHotplug {
    fn poll_event(&mut self) -> Result<Option<DeviceEvent>, CaptureError>;
    fn close(&mut self) -> Result<(), CaptureError>;
}
```

A concrete backend type (`WindowsDeviceHotplug::open(kinds: &[DeviceKind]) -> Result<Self,
CaptureError>`, `Type::open` shape per
[`code-style.md`](../../../docs/conventions/code-style.md) § Public Rust API shape) owns the
real OS registration and unregisters it on `close`/`Drop` — **a genuine RAII resource**, unlike
`support`/`request_permission`'s momentary calls. That is the concrete, technical reason a
stateful type is justified here where ADR-0003 rejected one for the probe surface: a
registered OS callback that is never unregistered is a real leak/dangling-callback risk, not
just an ergonomics question.

**v1 scope is Microphone/Loopback only, via `IMMNotificationClient`.** This is a real,
mature, well-documented WASAPI COM callback interface
(`IMMDeviceEnumerator::RegisterEndpointNotificationCallback`) giving `OnDeviceAdded` /
`OnDeviceRemoved` / `OnDeviceStateChanged` / `OnDefaultDeviceChanged` directly — a clean match
for `DeviceEvent`. `ProcessLoopback` has no device identity to watch (moot). **Buildability
caveat, corrected from this ADR's original text**: almost every existing Windows backend in
this crate only *consumes* COM interfaces; `wasapi_process.rs` is the one existing exception
(`#[implement(IActivateAudioInterfaceCompletionHandler)]`, a one-shot completion callback), so
implementing `IMMNotificationClient` is not literally this crate's *first* provided COM
interface as originally claimed here, but it is the first **long-lived, OS-driven, arbitrary-
thread callback** server object (`wasapi_process.rs`'s handler fires exactly once, synchronously
awaited by the same call that registered it) — genuinely more COM-threading surface than any
prior module, via the same standard, supported `windows`-rs `#[implement]` pattern.

**Camera and Screen hotplug are deferred, not v1** — not from lack of interest, but because
their real Windows mechanism is structurally different from `IMMNotificationClient`:
device-arrival notification for arbitrary device classes (camera: `KSCATEGORY_VIDEO_INPUT_DEVICE`;
display: monitor GUID) goes through `RegisterDeviceNotification` + `WM_DEVICECHANGE` (camera)
or `WM_DISPLAYCHANGE` (screen) — **window-message-based**, requiring a message-only `HWND` and
a message pump thread, a mechanism no module in this crate currently owns. Bolting that onto
this ADR would mix two unrelated Windows subsystems into one "v1"; each deserves its own
crate-local ADR when built, referencing `DeviceEvent`/`DeviceHotplug` from this one.

**Per-session `DeviceLost`, not just the global watcher.** New variant on both `CaptureError`
and `PlaybackError`:

```rust
/// The device this session was opened against disappeared while live (unplugged,
/// disabled, or otherwise invalidated). The session is no longer usable — open a new one.
DeviceLost,
```

This closes a **real existing gap**, not a hypothetical one: `wasapi.rs::pump_capture_loop`
and `wasapi_playback.rs::pump_playback_loop` currently `break` their loop silently on any
`GetNextPacketSize`/`GetBuffer`/`GetCurrentPadding` failure (which is exactly what
`AUDCLNT_E_DEVICE_INVALIDATED` produces when the open endpoint is unplugged) — the worker
thread just stops, and `poll_frame`/`write_frame` on the live session then look like a
permanently idle/silent stream forever, with **no error ever surfacing to the caller**. Wiring
`DeviceLost` requires the worker to record *why* it stopped (not just `stop.store(true, ..)`)
and `poll_frame`/`write_frame` to report it once — real implementation work, not done in this
ADR, but the contract is fixed here so backend work has a named target.

## Composition with ADR-0003 (capability/permission probe)

Selection and capability are orthogonal questions that compose through the shared
`DeviceKind` enum, not through a shared function or type:

1. `support(kind)` (ADR-0003) — cheap, "does this kind exist on this machine at all right
   now." Callers should still check this **before** `enumerate`, same guidance
   `request_permission` already carries.
2. `enumerate(kind)` (this ADR) — heavier (may query `IPropertyStore`/friendly names per
   endpoint), "list the actual devices," returns `DeviceInfo` snapshots with `id`/`name`.
3. `request_permission(kind)` (ADR-0003) — coarse, **kind-level** OS consent, unchanged by
   this ADR. Windows has no per-device consent granularity to expose (confirmed: WASAPI/MF
   consent gates are per-app-and-kind, not per-endpoint) — `enumerate` does not add a
   per-device permission axis that doesn't exist on the platform.
4. `open(config)` — unchanged shape, now takes `select: Select` instead of a raw index.

`enumerate` does not replace, wrap, or duplicate `support`/`request_permission` — it answers
"which one", not "can I at all" or "did the OS say yes." A typical flow: `support` →
`enumerate` → pick via `Select::Id`/`NameContains` → `request_permission` (still cached,
still kind-level) → `open`.

## Relationship to ADR-0013 and `camera-device-handle.md`

**Supersedes only the Camera-specific micro-decision** in
[ADR-0013](../../../docs/adr/0013-native-handle-and-gpu-device.md) ("`CaptureSource::Camera {
device: usize }` is left unchanged — undecided whether it is an index or a pointer") and the
wiki page tracking it (`docs/ai/wiki/device/camera-device-handle.md`, resolved this session to
"real enumeration index"). This ADR resolves the field to `select: Select`, which internally
still supports "open the Nth enumerated camera" (`Select::Id` from `enumerate(Camera)[n].id`,
or a bare ordinal via `DeviceInfo::ordinal`) — so `activate_for_index`'s ordinal-resolution
logic is reused, not thrown away, just no longer the *only* addressing mode. ADR-0013's other
three decisions (`NativeHandle`, `GpuDeviceHandle`, `StreamInfo` geometry split) are **untouched
and remain in effect** — this ADR does not reopen them. ADR-0013 itself is not edited (it
correctly records the historical state at its own date); the wiki page is updated instead
(see Wiki upkeep).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Per-kind `DeviceId` types (`AudioDeviceId`, `CameraDeviceId`, `ScreenDeviceId`) | Compile-time safety is real, but forces `Select`/`DeviceInfo` to become generic or triplicated across `AudioCaptureConfig`/`AudioPlaybackConfig`/`VideoCaptureConfig`; the existing pattern (`GpuDeviceHandle` as one tagged enum) already solves the "different underlying shape" problem without that cost, and wrong-kind mismatches are already routinely runtime-checked elsewhere in this crate (`CaptureSource` variant matches). |
| `Select<'a>` borrowing `&'a DeviceId`/`&'a str` (the ideal-sketch's suggestion) | Every backend spawns a `'static` worker thread (`thread::spawn(move || ..)`) with config-derived values — a borrowed `Select` cannot cross that boundary without infecting every config struct with a lifetime, a larger regression than the clone it avoids. |
| Keep `device_index: u32` alongside a new `select: Select` (additive, non-breaking) | Two fields that can disagree, with no real caller to protect (every current index-based caller is already limited to `0`); pre-1.0 status and ADR-0013's own precedent (`d3d11_device: usize` → `gpu_device`) both favor the clean break. |
| `DeviceName::Restricted` permission-gated name variant (ideal-sketch) | No backend in this workspace has a real name-restriction condition today (Windows enumeration/friendly-name queries are unconsented); speculative for a case nothing exercises — deferred until a real backend needs it. |
| `Devices::watch()` returning a bare event `Receiver` (ideal-sketch) | A `DeviceHotplug` sync-poll **trait** (mirroring `AudioCapture`/`VideoCapture`) keeps this crate's established idiom — concrete backend type, `Type::open` constructor, `Drop`-driven OS-callback unregistration — instead of introducing a second, differently-shaped API family (channel-based) alongside the poll-based one every other capability already uses. |
| `Select::Index(u32)` alongside `Id`/`NameContains`/`Default` | Real ergonomic value for `Screen`, but kind-specific special-casing; `DeviceInfo::ordinal` gives the same "pick the Nth" outcome uniformly across every kind at the cost of one enumeration call. |
| Build camera/screen hotplug in this same ADR | Real Windows mechanism (`WM_DEVICECHANGE`/`WM_DISPLAYCHANGE`, message-pump-based) is structurally unlike `IMMNotificationClient`; conflating them would misrepresent buildability and blur two separate follow-up ADRs. |

## Consequences

### Positive

- No more silent index-drift-after-replug failure mode — the PortAudio integer-index
  anti-pattern this ADR exists to remove is gone from every capture/playback kind except the
  two (Window, ProcessLoopback) that were never index-based hardware identity in the first
  place.
- `enumerate`/`friendly_name` plumbing already exists as dead code in `camera.rs` — wiring it
  up is largely "delete `#[allow(dead_code)]` and add the symbolic-link attribute query,"
  confirmed buildable without new heavy Windows surface for that kind.
- `DeviceIdRepr`/`DeviceEvent` are `#[non_exhaustive]`, leaving room for Linux (V4L2 by-id
  path, PipeWire node ID) and Web (`MediaDeviceInfo.deviceId`) variants without another
  breaking change.
- `DeviceLost` gives a name to a real, previously-silent failure mode already latent in
  `pump_capture_loop`/`pump_playback_loop`.

### Negative / Trade-offs

- Breaking change across `mediaway-device` + `mediaway-device-windows` (3 backend files) +
  any examples referencing `device_index`/`device: usize`/`output_index`. Acceptable pre-1.0.
- `DxgiOutput` identity is honestly weaker (session/topology-scoped) than the audio/camera
  IDs — documented, not hidden, but real callers wanting durable multi-monitor identity across
  reconnects will still need the deferred EDID-based work.
- `IMMNotificationClient` is COM-server-implementation ground for this crate (this
  backend module *provides* a COM interface for the OS to call into, not just consumes
  one) — more implementation risk than most backend modules, though a standard
  `windows`-rs `#[implement]` pattern this crate already had one precedent for
  (`wasapi_process.rs`'s `IActivateAudioInterfaceCompletionHandler` completion handler).
  **Implemented and hardware-verified** — `WindowsDeviceHotplug` in
  `mediaway-device-windows/src/hotplug.rs`; see Status above.
- ~~This ADR fixes the contract but implements no backend code~~ — **superseded**:
  `enumerate` bodies (`mediaway-device-windows`) and `DeviceLost` wiring into the existing
  worker loops landed in the same pass that implemented this ADR's types.
  `mediaway_pipeline::platform` dispatch for `enumerate` was **not** added (no free
  function exists in the facade to dispatch through — see § Enumeration's "Free-function
  shape" note; callers use `mediaway_device_windows::enumerate` directly, same as
  `support`/`request_permission` today).

## Deferred (out of scope for v1)

- Camera hotplug (`WM_DEVICECHANGE` + `KSCATEGORY_VIDEO_INPUT_DEVICE`) and screen/monitor
  hotplug (`WM_DISPLAYCHANGE`) — separate mechanism, separate follow-up ADR.
- ~~`DeviceLost` actually being raised from `pump_capture_loop`/`pump_playback_loop`~~ —
  **done**: both worker loops now set a `device_lost` flag on real WASAPI failures,
  distinct from a caller-requested `stop`; `poll_frame`/`write_frame` report
  `CaptureError::DeviceLost`/`PlaybackError::DeviceLost` once the flag is observed.
- EDID/`SetupAPI`-backed persistent monitor identity (stronger than `DXGI_OUTPUT_DESC.DeviceName`).
- `DeviceName::Restricted` / permission-gated name variant — add when a real backend needs it.
- Linux (V4L2 `by-id`, PipeWire node ID) and Web (`MediaDeviceInfo.deviceId`) `DeviceIdRepr`
  variants — the enum is `#[non_exhaustive]` specifically to allow this later.
- Per-device (not just per-kind) `PermissionState` — no platform this workspace targets
  exposes that granularity today.

## References

- [ADR-0001](0001-capture-traits.md) — `VideoCapture`/`AudioCapture` sync-poll shape `DeviceHotplug` mirrors
- [ADR-0002](0002-facade-platform-boundary.md) — facade/platform split this follows
- [ADR-0003](0003-capability-and-permission-probe.md) — capability/permission probe this composes with; free-function precedent
- [ADR-0004](0004-audio-playback-traits.md) — `AudioPlaybackConfig.device_index`, broken here
- [Workspace ADR-0013](../../../docs/adr/0013-native-handle-and-gpu-device.md) — `NativeHandle`/`GpuDeviceHandle` precedent this follows for `DeviceId`'s shape; Camera micro-decision superseded
- `docs/ai/wiki/device/camera-device-handle.md` — updated to point here
- `mediaway-device-windows/src/camera.rs` — existing unused `enumerate_camera_names`/`friendly_name`
- `mediaway-device-windows/src/capabilities.rs` — existing `EnumAudioEndpoints` precedent
- [`docs/spec/api-layers.md`](../../../docs/spec/api-layers.md) rule 4 — explicit typed variants over erased blobs
- [`docs/spec/async-and-streaming.md`](../../../docs/spec/async-and-streaming.md) — sync/poll policy for platform sessions
- [`docs/conventions/code-style.md`](../../../docs/conventions/code-style.md) § Public Rust API shape

ADRs are **English**. Numbering is local to this `adr/` folder.
