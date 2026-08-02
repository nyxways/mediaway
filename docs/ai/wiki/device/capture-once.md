# Single-shot capture — `capture_next_frame_blocking` / `capture_video_once`

ADR: [`mediaway-device` ADR-0006](../../../crates/mediaway-device/adr/0006-capture-once-screenshot.md).
Implemented and hardware-verified against the real DXGI backend
(`crates/mediaway-device/src/video.rs`). Screen (DXGI) is no longer
per-session Zero-Copy — see [windows-capture § Shared sessions](windows-capture.md)
for why (`mediaway-device-windows` ADR-0006, a distinct, later decision).

## Why

`VideoCapture` only had continuous-session shape (`poll_frame`/`release_frame`/`close`) —
no "just grab one frame" primitive anywhere. Product framing: recording is conceptually
repeated screenshotting, so single-shot and continuous capture should share the same
session type and `VideoFrame`, not be two unrelated shapes.

## The real tension: naive single-shot is not just slow, it can fail

`WindowsScreenCapture::open`'s real cost center is `IDXGIOutput1::DuplicateOutput`
(driver-level resource, not a cheap COM query). Per Microsoft's DDA docs, DXGI allows
**only one duplication interface per output per process** — a naive "open, grab one
frame, close" implementation called while a continuous session on the *same output* is
already open **in the same process** does not degrade, it **fails**
(`CaptureError::AccessDenied`). System-wide cap: only 4 processes may hold a live
duplication per session. No benchmark for `DuplicateOutput`'s wall-clock cost exists in
this repo — ADR-0006 does not fabricate one (Deferred).

Second caveat: `AcquireNextFrame` only delivers a new frame on desktop *change* —
**except** the very first call after a fresh `DuplicateOutput`, which always returns a
full baseline image. So a fresh session (single-shot path) reliably gets a frame soon;
an *already-open* session polled repeatedly can legitimately time out on a static
desktop (not a bug).

## Decision shape

- **`VideoCapture::capture_next_frame_blocking(&mut self, timeout: Duration) ->
  Result<VideoFrame, CaptureError>`** — default-provided trait method (works on an
  already-open session). Default: retry `poll_frame` with a bounded sleep between empty
  results (uniform pacing, since WGC's `TryGetNextFrame` and Camera's queue pop are
  non-blocking unlike DXGI's 16 ms-blocking `AcquireNextFrame` — a naive unsynced retry
  would busy-spin for those two). New `CaptureError::Timeout` variant.
- **`capture_video_once<C: VideoCapture>(open: impl FnOnce() -> Result<C, CaptureError>,
  timeout) -> Result<VideoFrame, CaptureError>`** — facade-level **generic free
  function** (no `Box`/`dyn`), composed as `open()` → `capture_next_frame_blocking` →
  `release_frame` → `close`. Rustdoc loudly states the full session-open cost **and**
  the concrete DXGI same-output-same-process failure mode — do not loop this to build a
  recorder.
- Both return the **same** `mediaway_common::VideoFrame` /
  `VideoFrameStorage::Gpu(GpuBufferHandle)` `poll_frame` already returns — no separate
  "screenshot frame" type, no readback.

Chosen over standalone-only (fails the DXGI concurrency finding) or session-method-only
(forces every one-off caller to hand-roll the open/block/close composition this ADR
centralizes once) — see ADR-0006 § Alternatives Considered for the full comparison.

## Scope

Trait-level default generalizes to Window/Camera automatically (same `poll_frame`-based
default, no source-specific code) — but v1's **hardware-grounded characterization and
verification is Screen (DXGI) only**. Camera has no Zero-Copy story at all yet
(`VideoFrameStorage::Cpu` always) — orthogonal to this ADR. Window (WGC) and Camera
(Media Foundation) get the shape, not the verification, this pass.

## Status

Implemented: `CaptureError::Timeout`, `VideoCapture::capture_next_frame_blocking`
default method, `capture_video_once` facade function
(`crates/mediaway-device/src/video.rs`, `video_tests.rs`). Window/Camera get the same
shape via the default method but remain unverified, per Scope above.

**Correction found while exposing this via `mediaway-device-ffi`
([`adr/0003-gpu-handle-c-abi.md`](../../../../crates/mediaway-device-ffi/adr/0003-gpu-handle-c-abi.md)):**
`capture_video_once` closed the session *before* returning the captured frame — for a
GPU-backed frame (Screen) this could dangle the just-captured handle once a solo/last
shared session's `close()` tore down the texture. Fixed: `capture_video_once` now
returns `Err(CaptureError::Unsupported)` instead of a `VideoFrameStorage::Gpu` frame;
GPU-backed capture must use an explicitly-managed session (`poll_frame`/
`capture_next_frame_blocking`) instead. Hardware-verified against the real DXGI backend
both ways: `capture_video_once_screen_is_unsupported_for_gpu_storage_or_skip` (this
crate) confirms the refusal; `mediaway-device-ffi`'s
`mediaway_video_capture_poll_frame_blocking` is the C-ABI-facing replacement for
Screen's single-frame capture.

## References

- [windows-capture](windows-capture.md) — DXGI screen capture this composes with
- [windows-window](windows-window.md), [windows-camera](windows-camera.md) — the other
  two `VideoCapture` backends the default method generalizes to (unverified)
- [selection](selection.md) — `Select`/`DeviceId` this ADR leaves unchanged
