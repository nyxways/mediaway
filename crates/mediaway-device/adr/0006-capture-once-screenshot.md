# ADR-0006: Single-shot zero-copy frame capture — `capture_next_frame_blocking` + `capture_video_once`

- **Status**: Accepted (2026-07-31 — implemented, hardware-verified against the real DXGI backend)
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device`

## Context

Today [`VideoCapture`](../src/video.rs) only exposes a **continuous session** shape:
`poll_frame()` (non-blocking, `Ok(None)` = not ready) → consume → `release_frame()` → …
→ `close()`. There is no "just grab one frame" primitive anywhere in the workspace —
not in the facade, not in `mediaway-device-windows`. The product framing driving this
ADR: video *recording* is conceptually "take zero-copy frames repeatedly and
stream/stack them," so a single-shot capture primitive and continuous recording should
share the same session type and the same `VideoFrame`/`GpuBufferHandle`, not be two
unrelated capability shapes.

### Real session-open cost — grounded in the actual DXGI backend

[`WindowsScreenCapture::open`](../../mediaway-device-windows/src/dxgi.rs) does, in order:
cast `ID3D11Device` → `IDXGIDevice`, `GetAdapter()`, resolve+enumerate the target
`IDXGIOutput` (`EnumOutputs`/`GetDesc`, cheap COM calls), cast to `IDXGIOutput1`, then
**`IDXGIOutput1::DuplicateOutput`**. `DuplicateOutput` is the real cost center: it is a
driver-level resource allocation, not a cheap COM query, and it carries two documented
OS-level constraints (Microsoft Learn, `IDXGIOutputDuplication` interface reference):

1. **One duplication interface per output per process.** "A process can have only one
   desktop duplication interface on a single desktop output" — a second `DuplicateOutput`
   call for the *same output*, from the *same process*, while the first is still live,
   fails (`E_INVALIDARG`-class error). In this backend that failure is mapped to
   `CaptureError::AccessDenied` (see `open`'s `.map_err(|_| CaptureError::AccessDenied)`
   on the `DuplicateOutput` call).
2. **System-wide cap.** By default only four processes total may hold a live
   `IDXGIOutputDuplication` in a session at once.

This is not just "slower" — it is a **hard compatibility constraint**: a naive
"open a session, grab one frame, close it" implementation, if called while a
`WindowsScreenCapture` continuous session on the *same output* is already open **in
the same process**, does not degrade gracefully to a slow path — it **fails outright**
with `CaptureError::AccessDenied`. Any single-shot design that ignores this is not just
a perf trap, it is a correctness trap for the exact "I'm already recording, also grab me
a thumbnail" use case the product framing cares about.

No benchmark for `DuplicateOutput`'s wall-clock cost exists in this repository (no
`mediaway-device-windows` bench harness calls it in a loop today) — this ADR does not
fabricate a number. The *repeated-poll* cost after a session is open is cheap by
contrast: `AcquireNextFrame(POLL_TIMEOUT_MS, ..)` with `POLL_TIMEOUT_MS = 16`, a single
OS wait call bounded at 16 ms, already exercised every `poll_frame()` in the existing
continuous path. Characterizing `DuplicateOutput`'s absolute cost with a real
[`machine_id`](../../../docs/benchmarks/machines.md) benchmark is listed under Deferred.

### `AcquireNextFrame` frame-availability semantics (a second, distinct caveat)

Per Microsoft's DDA docs and the DDA sample: `AcquireNextFrame` only delivers a new
frame when the desktop image actually **changes** (composited update), **except** the
very first call after a fresh `DuplicateOutput` — that first frame reliably returns the
complete current desktop image as a baseline, regardless of whether anything changed
since duplication was created. Consequence for this ADR's two shapes:

- **Fresh session (the `capture_once`-style path)**: always eventually gets one frame,
  bounded by first-present latency — a fresh duplication does not need a desktop change
  to deliver its first frame.
- **Already-open session, called repeatedly** (e.g. an app polling
  `capture_next_frame_blocking` every few seconds for periodic thumbnails): a genuinely
  static desktop since the caller's last `release_frame` can legitimately produce **no**
  frame for the whole timeout. That is not a bug or a backend failure — it is DDA's
  actual delta semantics, and must not be silently reported as if it were a generic
  backend error.

### Window (WGC) and Camera (Media Foundation) — do they share the same cost profile?

- **Window** ([`wgc.rs`](../../mediaway-device-windows/src/wgc.rs)): `open` does
  `CreateForWindow` → `Direct3D11CaptureFramePool::CreateFreeThreaded` →
  `CreateCaptureSession` → `StartCapture` — a comparable real session-setup chain to
  DXGI's. Its `poll_frame` uses `TryGetNextFrame`, which is **non-blocking** — it returns
  immediately with no frame if none is ready, unlike DXGI's `AcquireNextFrame` which
  already blocks up to 16 ms internally. This matters for the blocking primitive's
  default implementation (see Decision): a naive poll-and-retry loop with **no** pacing
  would busy-spin for WGC (and Camera) where it would not for DXGI.
- **Camera** ([`camera.rs`](../../mediaway-device-windows/src/camera.rs)): CPU-only
  today — every frame is `VideoFrameStorage::Cpu`, never `Gpu` (see
  `docs/ai/wiki/device/windows-camera.md`: "No DX11 Zero-Copy path yet… CPU copy is the
  only mode"). `open` blocks on `rx_info.recv()` while a spawned worker thread runs
  `MFEnumDeviceSources` + `MFCreateSourceReaderFromMediaSource` + native-type negotiation
  — a real, plausibly *more* expensive session-open cost than DXGI's (thread spawn +
  MF activation + format negotiation vs. one `DuplicateOutput` call), but it is
  **orthogonal to this ADR's Zero-Copy concern**: there is no GPU-handle screenshot story
  for Camera to unify with yet, because continuous Camera capture itself is not Zero-Copy.

## Decision

> Add a session-level blocking single-frame method to `VideoCapture` (works on an
> **already-open** session — the "build a recorder, or grab an occasional frame from a
> live session" case) **and** a facade-level generic free function composed on top of it
> (the "just get me one frame, I don't want to manage a session" case). Both return the
> exact same `mediaway_common::VideoFrame` (`VideoFrameStorage::Gpu(GpuBufferHandle)` on
> the Zero-Copy path) that `poll_frame` already returns — no separate "screenshot frame"
> type.

This is Option 3 from the task framing (session-level primitive **and** a standalone
convenience), chosen over either alone because of the concrete finding above: Option 1
alone (a `capture_once`-only free function that internally opens+polls+closes every
call) is not merely a perf trap for a recorder loop built on it — for Screen sources it
is an outright **correctness** trap when a continuous session on the same output is
already live in-process (`DXGI`'s one-duplication-per-output-per-process constraint).
Option 2 alone leaves no zero-session-lifecycle entry point for the genuinely common
"hotkey screenshot, no session bookkeeping" caller. Building the standalone convenience
**as a composition on top of** the session-level method (not a parallel
open+poll+close reimplementation) is also what
[`api-layers.md`](../../../docs/spec/api-layers.md) rule 1 requires ("convenience is
composition only").

### 1. `VideoCapture::capture_next_frame_blocking` — default-provided trait method

```rust
// mediaway-device/src/video.rs
pub trait VideoCapture {
    fn stream_info(&self) -> &StreamInfo;
    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, CaptureError>;
    fn release_frame(&mut self) -> Result<(), CaptureError>;
    fn close(&mut self) -> Result<(), CaptureError>;

    /// Block the calling thread until the next frame is ready or `timeout`
    /// elapses.
    ///
    /// Same session, same held-frame contract as [`poll_frame`](Self::poll_frame) —
    /// this is not a new state machine, just a retry loop around it: calling
    /// this while the previous frame is still held (not yet
    /// [`release_frame`](Self::release_frame)d) fails the same way `poll_frame`
    /// already does.
    ///
    /// **On an already-open session, a returned [`CaptureError::Timeout`] is
    /// not necessarily a failure** — for delta-based backends (DXGI Desktop
    /// Duplication) it may legitimately mean "nothing changed since the last
    /// released frame," not a backend error. Callers wanting a guaranteed
    /// always-fresh image regardless of whether content changed should use
    /// [`capture_video_once`] (a fresh session's first frame is always a full
    /// baseline image, not delta-gated) instead of retrying this on a
    /// long-lived session.
    ///
    /// Default implementation retries [`poll_frame`](Self::poll_frame),
    /// sleeping a short bounded interval between empty (`Ok(None)`) results,
    /// until a frame arrives or the deadline passes. This default works
    /// uniformly for every backend's `poll_frame` cost profile — blocking
    /// (DXGI's `AcquireNextFrame`, ~16 ms per call) or immediate-return
    /// (WGC's `TryGetNextFrame`, Camera's queue pop) — because the sleep
    /// pacing is added unconditionally, not assumed from the backend.
    /// Backends may override this default with a native blocking wait
    /// (e.g. DXGI calling `AcquireNextFrame` once with the full `timeout` as
    /// its OS-level wait, instead of a Rust-level retry loop) as a documented
    /// perf follow-up — not required by this ADR.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Timeout`] if `timeout` elapses with no frame.
    /// Otherwise propagates the same errors as `poll_frame`.
    fn capture_next_frame_blocking(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<VideoFrame, CaptureError> {
        // default: loop poll_frame(); sleep a bounded interval on Ok(None);
        // Err(CaptureError::Timeout) once `timeout` elapses.
        // .. (implementation deferred — this ADR fixes the contract only)
    }
}
```

Adds `CaptureError::Timeout` (new `#[non_exhaustive]` variant, same additive shape
`DeviceLost` used in [ADR-0005](0005-device-selection.md)) — distinct from
`poll_frame`'s `Ok(None)` (which is a normal "not ready yet, caller decides whether to
retry" signal, not an error) precisely because a blocking call with a caller-given
deadline needs a real terminal outcome for "waited the whole deadline, still nothing."

**Default-provided, not required per backend.** A blanket default keeps this a
trait-level addition rather than three separate backend implementations
(`WindowsScreenCapture`, `WindowsWindowCapture`, `WindowsCameraCapture` all get it for
free the moment they implement `poll_frame`/`release_frame`/`close`, which they already
do). This is also how the method **generalizes to Window and Camera automatically**
without extra code, while this ADR's *hardware-grounded cost characterization* stays
Screen-only (see Scope below) — the trait shape is source-agnostic; the *verified,
documented cost story* is not, yet.

### 2. `capture_video_once` — facade-level generic composition, not per-backend duplication

```rust
// mediaway-device/src/video.rs
/// Open a session via `open`, block for one frame, then release and close —
/// a convenience for "I don't want to manage a session" callers (e.g. a
/// hotkey screenshot command).
///
/// **Pays a full session-open cost on every call** — for
/// [`CaptureSource::Screen`](CaptureSource::Screen) that includes DXGI
/// `DuplicateOutput`'s real driver-level setup (see
/// [ADR-0006](../adr/0006-capture-once-screenshot.md) for the concrete
/// characterization). **Do not** call this in a loop to build a recorder —
/// use an already-open [`VideoCapture`] session's
/// [`poll_frame`](VideoCapture::poll_frame) or
/// [`capture_next_frame_blocking`](VideoCapture::capture_next_frame_blocking)
/// instead; repeatedly reopening a session is both slower *and*, for Screen
/// sources, can outright fail with [`CaptureError::AccessDenied`] if another
/// session on the same output is already open in this process (DXGI allows
/// only one duplication interface per output per process).
///
/// # Errors
///
/// Propagates `open`'s errors, or [`CaptureError::Timeout`] from
/// [`capture_next_frame_blocking`](VideoCapture::capture_next_frame_blocking).
pub fn capture_video_once<C: VideoCapture>(
    open: impl FnOnce() -> Result<C, CaptureError>,
    timeout: std::time::Duration,
) -> Result<VideoFrame, CaptureError> {
    let mut session = open()?;
    let result = session.capture_next_frame_blocking(timeout);
    let _ = session.release_frame();
    let _ = session.close();
    result
}
```

Generic over any `VideoCapture` implementor, taking the backend's own `Type::open` as a
closure — monomorphized per call site, **no `Box`/`dyn`**, matching
[zero-cost-abstractions.md](../../../docs/spec/zero-cost-abstractions.md). Called as
`mediaway_device::capture_video_once(|| WindowsScreenCapture::open(&config), timeout)`.
This single definition composes `open` + the trait default from §1 + `release_frame` +
`close` **once**, in the facade, rather than every backend crate re-deriving its own
open→block→close dance — the load-bearing "pays full session-open cost, do not loop
this" rustdoc lives in exactly one place. A backend crate MAY additionally expose a
thin, discoverable inherent wrapper (e.g. `WindowsScreenCapture::capture_once(config,
timeout)` calling straight into `capture_video_once`) matching
[`code-style.md`](../../../docs/conventions/code-style.md)'s `Type::open`-style
preference for primary APIs — **not decided or required by this ADR**, left as
low-risk, backend-local follow-up polish once this contract lands.

### Zero-Copy — same type, no readback

Both surfaces return `mediaway_common::VideoFrame` with `storage:
VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 { .. })` on the DXGI Zero-Copy path —
identical to what `poll_frame` already returns today. Neither introduces a CPU
readback, a new frame type, or a different `PixelFormat`. `capture_video_once` on a
`CaptureOutputPreference::CpuFramesOk` config (e.g. future Camera use) returns
`VideoFrameStorage::Cpu` exactly as that backend's own `poll_frame` would — the
composition never changes what the underlying session would have produced.

### Scope for v1 — Screen (DXGI) characterized and targeted; Window/Camera inherit the shape, not the verification

Both `capture_next_frame_blocking` and `capture_video_once` are defined at the
`VideoCapture` trait / facade level, so **Window and Camera get the same API shape for
free** the moment their existing `poll_frame` implementations exist — there is no
source-specific branch anywhere in this design. What is explicitly **not** in scope for
v1:

- **Hardware verification** of `capture_next_frame_blocking`'s default loop and
  `capture_video_once`'s composition is Screen (DXGI) only. Window (WGC) and Camera
  (Media Foundation) are believed to work identically (same trait default, same
  composition function) but are **not exercised** by this ADR.
- **A `DuplicateOutput` wall-clock benchmark** — Deferred (see below), not fabricated.
- **A backend-native override** of `capture_next_frame_blocking` (e.g. DXGI calling
  `AcquireNextFrame` once with the full `timeout`, instead of the generic retry loop) —
  a real, named perf follow-up, not required for this ADR's contract to be useful.
- **Camera's own Zero-Copy story** — Camera has no `Gpu`-backed frame at all today; this
  ADR does not change that. Camera gets a working `capture_next_frame_blocking`/
  `capture_video_once` for its existing CPU frames, nothing more.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| **Option 1 only** — standalone `capture_once(config, timeout)` free/associated fn that opens, polls with retry, closes; no session-level method | Fails the concrete DXGI finding: reopening a duplication on an output that already has a live session in-process does not degrade, it **fails** (`CaptureError::AccessDenied`); also does not satisfy the product framing that recording *is* repeated screenshotting at the *session* level, not just the frame-type level. |
| **Option 2 only** — session-level `capture_next_frame_blocking` with no standalone convenience | Leaves every "hotkey screenshot" / "CLI screenshot" caller to hand-roll `open` → block → `release_frame` → `close` at each call site, duplicating the exact composition this ADR's Decision centralizes once — a real ergonomics regression with no offsetting benefit, since the composition is safe (it does not reintroduce the naive-loop perf trap by construction — it is not meant to be called repeatedly). |
| A stateful **session pool / cache** that transparently reuses an already-open duplication across `capture_once` calls | Would solve both the perf and the one-duplication-per-output constraint, but introduces global mutable state (a process-wide cache keyed by output) with its own lifetime/ownership questions (who closes it, thread-safety, staleness after a topology change) — a materially bigger design than this ADR's scope; `capture_next_frame_blocking` on a caller-owned session already gives the same benefit without hidden global state. Worth a future ADR if real demand appears. **Update 2026-07-31:** real demand appeared — a narrower, correctness-triggered version (activates only on the OS-enforced same-output-same-process conflict, not as a general reuse cache) is adopted in [`mediaway-device-windows` ADR-0006](../../mediaway-device-windows/adr/0006-shared-desktop-duplication.md). |
| A separate `ScreenshotFrame` type (owns a fresh CPU or GPU snapshot, distinct from `VideoFrame`) | Directly rejected by the task framing and by [api-layers.md](../../../docs/spec/api-layers.md) rule 4/§Anti-patterns: a second frame type either duplicates `VideoFrame`'s shape for no reason, or quietly downgrades to a CPU readback while "looking equivalent" — exactly the anti-pattern `caveats-and-clarity.md` forbids. |
| `capture_next_frame_blocking` as a **required** (non-default) trait method, forcing each backend to hand-write its own retry/blocking logic | More honest per-backend tuning opportunity, but forces Window/Camera backend work as a hard prerequisite of this ADR landing at all, when the generic default is correct (if not maximally efficient) for all three today — against "no large coding before approval" and against shipping the contract Screen-verified now, others later. |
| Async-only shape (`Future`-returning `capture_next_frame` instead of a blocking call + `Duration` timeout) | `mediaway-device` backends already spawn OS-driven worker threads / COM sessions synchronously (`wasapi.rs`, `camera.rs`); [`async-and-streaming.md`](../../../docs/spec/async-and-streaming.md) requires sync/poll to stay available and does not mandate a runtime in this facade. A sync blocking method with an explicit timeout matches the existing sync/poll idiom this crate already uses everywhere (`poll_frame`, `DeviceHotplug::poll_event`); async wrapping (if ever wanted) composes on top later, same as everywhere else in this crate. |

## Consequences

### Positive

- Recording and single-shot capture are unified at the **session** level, not just the
  frame-type level — directly satisfies the product framing that motivated this ADR.
- No silent perf *or* correctness trap: `capture_video_once`'s rustdoc names both the
  cost and the concrete DXGI one-duplication-per-output-per-process failure mode a naive
  loop-of-`capture_once` would hit, per
  [`caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md).
- `capture_next_frame_blocking`'s default works for every current backend
  (Screen/Window/Camera) with zero backend-specific code, while leaving room for a
  backend to override with a cheaper native wait later.
- No new frame type, no new `GpuBufferHandle` variant, no readback — `VideoFrame` /
  `VideoFrameStorage::Gpu` stay the single Zero-Copy vocabulary this crate already has.
- `capture_video_once` is a plain generic function (no `Box`/`dyn`), consistent with
  [zero-cost-abstractions.md](../../../docs/spec/zero-cost-abstractions.md).

### Negative / Trade-offs

- `CaptureError::Timeout` on an already-open session is **ambiguous by design** for
  delta-based backends (DXGI): "no frame within timeout" can mean either "backend
  problem" or "desktop genuinely didn't change." Documented, not hidden, but callers
  building on `capture_next_frame_blocking` directly (rather than
  `capture_video_once`) must understand this distinction themselves — a real caveat
  this ADR only names, it does not remove the ambiguity.
- The default `capture_next_frame_blocking` retry loop is not the most CPU/latency
  efficient shape possible for DXGI specifically (a single OS-level
  `AcquireNextFrame(timeout_ms, ..)` call would be strictly better than N Rust-level
  retries) — accepted for v1 in exchange for zero required backend work; a documented
  follow-up, not a hidden cost (the method's own rustdoc states this).
- No wall-clock number for `DuplicateOutput`'s cost is published in this ADR — an honest
  gap versus a nicer-sounding-but-fabricated number, at the cost of leaving "how much
  slower, exactly" unanswered until the Deferred benchmark lands.
- Window/Camera get the API shape without hardware verification in this pass — a real,
  if likely low, risk that WGC's or Media Foundation's actual behavior surprises the
  generic default (e.g. an unexpected blocking/non-blocking quirk in `TryGetNextFrame`
  under contention) that DXGI verification would not catch.

## Deferred (out of scope for v1)

- Implementation of `capture_next_frame_blocking`'s default body and
  `capture_video_once` (this ADR fixes the contract only, per "no large coding before
  approval").
- A real `DuplicateOutput` open-cost benchmark with a
  [`machine_id`](../../../docs/benchmarks/machines.md), comparing session-open cost to
  steady-state `AcquireNextFrame` cost, per
  [`benchmarking.md`](../../../docs/conventions/benchmarking.md).
- A DXGI-native override of `capture_next_frame_blocking` (single OS-level wait via
  `AcquireNextFrame(timeout_ms, ..)` instead of the generic Rust-level retry loop).
- Hardware verification of both surfaces against `WindowsWindowCapture` (WGC) and
  `WindowsCameraCapture` (Media Foundation).
- A session pool / cache alternative (see Alternatives Considered) if real demand for
  transparently-shared single-shot captures appears.
- An `AudioCapture` equivalent (a "record one clip synchronously" analog) — not
  requested, not designed here.
- Backend-local discoverable wrappers (e.g. `WindowsScreenCapture::capture_once`) — a
  thin, optional, low-risk composition over `capture_video_once`; each backend crate
  may add its own when convenient.

## References

- [`mediaway-device/src/video.rs`](../src/video.rs) — `VideoCapture`, `VideoFrame`
  contract this ADR extends
- [`mediaway-device-windows/src/dxgi.rs`](../../mediaway-device-windows/src/dxgi.rs) —
  `WindowsScreenCapture::open`/`poll_frame`, the concrete session-open chain
  characterized above
- [`mediaway-device-windows/src/wgc.rs`](../../mediaway-device-windows/src/wgc.rs) —
  `TryGetNextFrame` non-blocking poll, contrasted with DXGI's blocking
  `AcquireNextFrame`
- [`mediaway-device-windows/src/camera.rs`](../../mediaway-device-windows/src/camera.rs)
  — CPU-only session-open cost, orthogonal Zero-Copy status
- [ADR-0001](../../mediaway-device-windows/adr/0001-dxgi-desktop-duplication.md) — DXGI
  Zero-Copy decision this ADR builds on
- [ADR-0004](../../mediaway-device-windows/adr/0004-wgc-window-capture.md) — WGC window
  capture, referenced for the non-blocking poll contrast
- [ADR-0005](0005-device-selection.md) — `Select`/`DeviceId` this ADR composes with
  unchanged (both `capture_next_frame_blocking` and `capture_video_once` take the same
  `VideoCaptureConfig`/`CaptureSource` shape ADR-0005 left in place)
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) — the
  core constraint driving this ADR's honesty requirements
- [`docs/spec/api-layers.md`](../../../docs/spec/api-layers.md) — rule 1 (convenience is
  composition only), rule 4 (explicit typed handles, no erasure)
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md) — `GpuBufferHandle`
  Zero-Copy vocabulary this ADR does not extend or bypass
- [`docs/spec/async-and-streaming.md`](../../../docs/spec/async-and-streaming.md) —
  sync/poll policy justifying the blocking-with-timeout shape over an async-only design
- Microsoft Learn, [`IDXGIOutputDuplication` interface
  (dxgi1_2.h)](https://learn.microsoft.com/en-us/windows/win32/api/DXGI1_2/nn-dxgi1_2-idxgioutputduplication)
  — source for the one-duplication-per-output-per-process and four-process-per-session
  constraints cited in Context
- `docs/ai/wiki/device/windows-capture.md`, `windows-window.md`, `windows-camera.md` —
  updated alongside this ADR (see Wiki upkeep)

ADRs are **English**. Numbering is local to this `adr/` folder.
