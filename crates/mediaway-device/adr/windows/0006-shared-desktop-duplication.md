# ADR-0006: Shared, refcounted DXGI Desktop Duplication sessions

- **Status**: Accepted (2026-07-31 — implemented in `dxgi_shared.rs`, hardware-verified: two concurrent sessions on the same output both receive independent frames)
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-windows`

## Context

`WindowsScreenCapture::open` ([`dxgi.rs`](../src/dxgi.rs)) maps a second
`IDXGIOutput1::DuplicateOutput` call for the **same output**, from the **same
process**, while a first duplication on that output is still live, to
`CaptureError::AccessDenied` — a hard OS-level constraint (Microsoft Learn,
`IDXGIOutputDuplication`: "a process can have only one desktop duplication
interface on a single desktop output"), not a soft/tunable limit. This is
narrower than "no concurrent captures": different outputs (multi-monitor) are
unaffected, and Window capture (WGC, [`wgc.rs`](../src/wgc.rs)) uses a
different API with no such constraint at all. The failure only fires when a
caller opens the *same* monitor's output a second time while a first session
on it (e.g. a continuous recorder) is already open in-process — for example,
"I'm recording monitor A, also give me a one-off screenshot of monitor A"
([`mediaway-device` ADR-0006](../../mediaway-device/adr/0006-capture-once-screenshot.md),
which introduces exactly that one-off `capture_video_once` convenience and
originally rejected a general "session pool / cache" as too broad a design
for its scope).

This ADR adopts a **narrower, correctness-triggered** version of that
previously-rejected idea: sharing activates only when a second `open()` on
the same OS-identified output would otherwise fail, not as a general
performance cache. It must be automatic (no registry object threaded through
call sites) and self-cleaning (no manual lifecycle management, no leak).

### Frame validity is the hard constraint, not refcounting

`poll_frame`/`release_frame` ([`dxgi.rs`](../src/dxgi.rs)) are strictly
single-consumer: `AcquireNextFrame` is refused while a frame is already held,
and the held `IDXGIResource`/`ID3D11Texture2D` is only valid between one
`AcquireNextFrame` and its matching `ReleaseFrame` — the driver may recycle
that surface's content the instant `ReleaseFrame` runs. Two independent
consumers cannot each call `AcquireNextFrame`/`ReleaseFrame` against the same
underlying `IDXGIOutputDuplication` without corrupting each other's state
machine. Refcounting the handle is the easy part (`Arc`/`Weak`); safely
handing frames to N independent consumers is the real design problem.

`GpuBufferHandle::DirectX11`/`NativeHandle` are `Copy`
([`mediaway-common/src/gpu.rs`](../../mediaway-common/src/gpu.rs)) and
`VideoFrame` is `Clone`
([`mediaway-common/src/frame.rs`](../../mediaway-common/src/frame.rs)), so
duplicating a frame *handle* to N consumers is free — but the *texture it
points to* is not independently usable by N consumers unless each has its
own copy, given the single-consumer acquire/release contract above.

## Decision

> Adopt a dedicated-driver-thread, per-consumer-copy design for **Screen
> (DXGI) only**. A shared duplication is driven exclusively by one background
> thread; each attached consumer gets its own texture, refreshed via one
> `CopyResource` per frame per consumer. The existing single-consumer
> (exclusive) path is unchanged and stays copy-free.

### Registry

Keyed by the same `DeviceId` `enumerate_outputs`/`Select::Id` already use
(`DeviceId::from_dxgi_output_device_name`, `dxgi.rs`) — no new identity type.
`static REGISTRY: OnceLock<Mutex<HashMap<DeviceId, Weak<SharedDuplication>>>>`
inside `mediaway-device-windows`. `open()` looks up the key; a live `Weak`
upgrade attaches a new consumer; otherwise it calls `DuplicateOutput` as
today (now inside the driver thread — see below), spawns the driver thread,
and registers a fresh `Weak`.

**Correction versus this ADR's first draft:** `WindowsScreenCapture::open`
must route through this registry **unconditionally**, not only as a
fallback after a direct `DuplicateOutput` fails. DXGI's one-duplication-per-
output-per-process constraint means a second opener can only "join" a first
opener's session if that first opener is *discoverable* — an `open()` that
took a private, unregistered fast path has no way to be found or handed off
to later. There is no way to "upgrade" an already-open, unregistered
duplication into a shared one after the fact without either (a) requiring
every opener to register regardless of whether sharing is ever needed, or
(b) building a live hand-off protocol between two already-running sessions
(rejected below — materially more complex, no clear win over (a)). This ADR
adopts (a): **every** `WindowsScreenCapture::open` on `CaptureSource::Screen`
now spawns/attaches via the shared driver-thread mechanism, even when it
turns out to be the only consumer for its entire lifetime.

### Frame fan-out (breaks Zero-Copy for the *shared* path only)

The driver thread owns `IDXGIOutputDuplication` exclusively and drains it as
fast as DDA allows: `AcquireNextFrame` → `CopyResource` into an internal
scratch texture → immediate `ReleaseFrame` (never blocked by any consumer).
For each attached consumer without an already-pending unread frame, it then
does one more `CopyResource` into that consumer's own dedicated texture
(allocated once at attach) and marks it pending.

**This is a real, honest cost, and must be documented as such per
[`caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md):** every
`CaptureSource::Screen` session now performs one GPU-side `CopyResource` per
frame **per attached consumer** (still GPU-resident, no CPU readback) —
including the single-consumer case, since (per the correction above) every
open now runs through the driver-thread mechanism. This is a genuine change
from today's exclusive path's zero-copy behavior, not a cost confined to an
opt-in "shared" mode. Rustdoc on `WindowsScreenCapture` must say so
explicitly.

**Deferred optimization, named not hidden:** a solo consumer (the common
case) could in principle skip the copy entirely by referencing the driver
thread's directly-acquired texture instead of its own dedicated copy, falling
back to per-consumer copies only once a second consumer attaches. This would
restore today's zero-copy behavior for the overwhelmingly common case. Not
implemented in this pass — it requires safely handling a consumer already
mid-hold on the direct-reference texture at the moment a second consumer
attaches (a real, separate design question), and this ADR ships the simpler,
always-copies version first per "no large coding before approval." Tracked
here as a named follow-up, not silently dropped.

**Slow consumers**: a consumer that hasn't consumed its pending frame is
skipped that tick — it misses the frame rather than applying backpressure to
the driver thread or other consumers. This mirrors DDA's own existing
delta-coalescing behavior for a single slow-polling consumer, so it is not a
new class of surprise.

**Cross-device consumers**: D3D11 forbids a plain `CopyResource` across
device instances without `OpenSharedResource`. v1 requires a second caller's
`gpu_device` to be the **same device instance** as the first attacher;
otherwise `open()` returns `CaptureError::InvalidInput`. This is a real
limitation, not silently ignored — a future ADR can add
`OpenSharedResource`-based cross-device sharing if needed.

### Lifecycle and the `close()` contract change

Teardown is immediate on last-handle-drop, no debounce: dropping the last
`Arc<SharedDuplication>` reference signals shutdown, joins the driver thread,
and releases `duplication`/`device`. This means, **for a shared handle**,
[`VideoCapture::close`](../../mediaway-device/src/video.rs)'s documented
contract ("end the session and free OS resources") changes: it now means
"release my interest in this shared session" — the real OS resource is freed
only when the last attached consumer closes. `mediaway-device`'s
`VideoCapture::close` rustdoc must gain a paragraph stating this explicitly;
callers that rely on `close()` as a hard "OS resource is now gone" guarantee
(e.g. before a privilege-sensitive re-open) would otherwise be silently
wrong.

### Type shape (sketch, not final code)

```rust
// mediaway-device-windows/src/dxgi_shared.rs
static REGISTRY: OnceLock<Mutex<HashMap<DeviceId, Weak<SharedDuplication>>>> = OnceLock::new();

struct SharedDuplication {
    consumers: Mutex<Vec<ConsumerSlot>>,
    shutdown: Arc<AtomicBool>,
    driver_thread: JoinHandle<()>,
    stream_info: StreamInfo,
}
// Drop (last Arc ref) -> signal shutdown, join thread, release duplication/device.

struct ConsumerSlot {
    id: u64,
    pending: Option<VideoFrame>, // own texture; overwritten, never queued
}

pub struct WindowsScreenCapture {
    inner: Backing,
}
enum Backing {
    Exclusive(CaptureSession),  // today's type, unchanged, zero overhead
    Shared { shared: Arc<SharedDuplication>, consumer_id: u64 },
}
// WindowsScreenCapture keeps implementing VideoCapture via a match on `inner`
// — enum dispatch, no Box<dyn>, no public API/call-site change.
```

### Scope: Screen (DXGI) only, not Window/Camera

The forcing constraint is DXGI-specific. WGC has no documented
one-instance-per-process cap; Camera is CPU-only with no comparable OS
resource. `mediaway-device` ADR-0006's trait-default philosophy generalizes
for *free* when a default method costs nothing extra per backend — this
sharing machinery is the opposite: expensive, stateful, backend-specific
plumbing with no forcing need for WGC/Camera today. Revisit only if real
demand appears, same deferral stance `mediaway-device` ADR-0006 already uses
for its own rejected alternatives.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| General-purpose session pool/cache (the original, broader idea `mediaway-device` ADR-0006 rejected) | Unclear staleness/ownership as a standing performance optimization across arbitrary reuse patterns — this ADR adopts a narrower version that activates only on the OS-enforced same-output-same-process conflict, not as a general cache. |
| Explicit registry object threaded through call sites (caller-owned, no global state) | Avoids the "hidden global state" concern cleanly, but the user explicitly wants this automatic — opening the same output twice should just work without every call site managing a registry handle. |
| CPU readback + broadcast instead of per-consumer GPU copy | Strictly worse: adds a GPU→CPU stall on top of the copy this design already needs, for no benefit — `CopyResource` GPU→GPU already achieves the fan-out without leaving the GPU. |
| Backpressure instead of drop-on-slow-consumer | A stalled slow consumer would stall the shared driver thread and therefore every other consumer (including the exclusive-cost-sensitive recorder) — rejected as a correctness/latency risk to unrelated callers. |
| Cross-device sharing via `OpenSharedResource` in v1 | Real added complexity (shared handle lifetime across two `ID3D11Device`s) with no concrete caller need yet; deferred, same-device-instance requirement documented instead. |
| Generalize to Window (WGC) / Camera now | No forcing constraint exists for either backend today; would be speculative plumbing with no real caller need — against "no features beyond the request." |
| Direct `DuplicateOutput` fast path first, fall back to shared registry only when it fails | Does not work: a second opener that fails only learns *that* another duplication exists, not *which* live `WindowsScreenCapture` holds it — an unregistered first opener cannot be found, joined, or handed off to after the fact. Universal registration (this ADR's Decision) is the only way a second opener can ever succeed without the first knowing in advance it would need to share. |
| Live hand-off protocol: an unregistered exclusive session detects a second opener's failure and hands its duplication to a newly spawned driver thread | Would preserve zero-copy for the true solo case without the registration-everywhere cost, but requires moving a *live* `IDXGIOutputDuplication`/`ID3D11Device` between threads after the fact — exactly the COM cross-thread hazard this ADR otherwise avoids by construction (see the `Send`-soundness discipline note). Rejected as materially riskier than universal registration for an unproven perf win; the deferred solo-consumer copy-skip optimization gets most of the same benefit without ever moving a live COM object. |

## Consequences

### Positive

- Opening the same DXGI output twice in-process now succeeds instead of
  failing with `AccessDenied`, without any call-site API change.
- Self-cleaning via `Arc`/`Weak` — no manual registry management, no leak, no
  staleness window to reason about.
- Every `WindowsScreenCapture` (COM object creation, duplication, per-consumer
  textures) stays confined to the one driver thread that owns it — no COM
  type crosses a thread boundary, sidestepping the exact `Send`-soundness
  trap `mediaway-device-ffi` ADR-0002 hit for `WindowsDeviceHotplug`.

### Negative / Trade-offs

- **Every** `CaptureSource::Screen` session now pays one GPU-side
  `CopyResource` per frame — not Zero-Copy — including the single-consumer
  case, since universal registration (see Decision § Registry correction) is
  what makes joining possible at all. This is a real, workspace-visible
  regression from today's exclusive path's zero-copy behavior, not a cost
  confined to an opt-in "shared" mode; the deferred solo-consumer
  optimization (above) is the named path back to zero-copy for the common
  case.
- `VideoCapture::close`'s contract is now conditional (may or may not free
  the real OS resource depending on whether other consumers remain attached)
  — a real behavior change callers must understand, not just an
  implementation detail.
- A slow shared consumer silently misses frames rather than erroring —
  correct by design, but a caller expecting every frame delivered must poll
  promptly, same as any other DDA consumer.
- Cross-`ID3D11Device` sharing is not supported in v1; a second caller with a
  different device instance for the same output gets `InvalidInput` rather
  than a working (if slower) fallback.
- One background driver thread per live output (not per consumer) is new
  standing resource usage that does not exist today, for every screen
  capture session, not only shared ones.

## References

- [`mediaway-device` ADR-0006](../../mediaway-device/adr/0006-capture-once-screenshot.md)
  — single-shot capture this ADR's sharing need grew out of; its
  "Alternatives Considered" session-pool/cache row is superseded by this
  ADR's narrower adoption
- [ADR-0001](0001-dxgi-desktop-duplication.md) — DXGI Zero-Copy decision this
  ADR extends; the exclusive path stays exactly as ADR-0001 specified
- [ADR-0004](0004-wgc-window-capture.md) — WGC, referenced for why Window
  capture has no equivalent constraint
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md)
  — honest-cost documentation requirement this ADR's shared path follows
- [`docs/spec/zero-cost-abstractions.md`](../../../docs/spec/zero-cost-abstractions.md)
  — enum dispatch (`Backing`) over `Box`/`dyn` for the exclusive/shared split
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md) —
  `GpuBufferHandle` vocabulary this ADR does not add a new variant to
- Microsoft Learn, [`IDXGIOutputDuplication` interface
  (dxgi1_2.h)](https://learn.microsoft.com/en-us/windows/win32/api/DXGI1_2/nn-dxgi1_2-idxgioutputduplication)
  — source for the one-duplication-per-output-per-process constraint

ADRs are **English**. Numbering is local to this `adr/` folder.
