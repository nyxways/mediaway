# ADR-0005: Capture-to-encode bridge C ABI

- **Status**: Accepted
- **Date**: 2026-08-05
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi`

## Context

`mediaway-ffi`'s `device` module (Camera/Screen capture) and `pipeline` module
(auto video encode) are both real and hardware-verified, but a C caller wanting
"record the screen/camera to MP4" has to do the frame plumbing by hand: poll a
`mediaway_camera_frame_t`/`mediaway_desktop_frame_t` from `device.h`, copy its
fields into a `mediaway_video_frame_t`, call
`mediaway_encode_session_write_frame`, then free/release the capture-side frame —
tracked as an explicit gap in both `docs/roadmap.md` and this crate's own
`docs/pipeline/roadmap.md` since the `device` module's capture surface landed.

At the Rust level this plumbing is close to a no-op: `CameraCapture::poll_frame`/
`DesktopVideoCapture::poll_frame` and `mediaway::EncodeSession::write_frame` all
operate on the exact same `mediaway_common::VideoFrame` type — confirmed by
reading both trait definitions
(`crates/mediaway-device/src/{camera/capture.rs,desktop/video.rs}`) and
`crates/mediaway/src/session.rs`. Post-[ADR-0021](../../../../docs/adr/0021-workspace-consolidation.md),
`device` and `pipeline` are modules of the same crate, so nothing prevents a
function in `pipeline` from taking a `device`-module handle pointer directly.

## Decision

> Add two functions to `pipeline.h` (ABI v4): `mediaway_encode_session_write_frame_from_camera_capture`
> and `mediaway_encode_session_write_frame_from_desktop_capture`, each polling one
> frame from a `device`-module capture handle and pushing it straight into an
> `EncodeSessionHandle` — no intermediate `mediaway_{camera,desktop}_frame_t`
> exposed to the caller, no extra copy (the `VideoFrame` moves from poll to push
> inside one Rust call).

### 1. Function shape

```c
mediaway_pipeline_status_t mediaway_encode_session_write_frame_from_camera_capture(
    mediaway_encode_session_t *session,
    mediaway_camera_capture_t *capture,
    bool *out_wrote_frame);

mediaway_pipeline_status_t mediaway_encode_session_write_frame_from_desktop_capture(
    mediaway_encode_session_t *session,
    mediaway_desktop_capture_t *capture,
    bool *out_wrote_frame);
```

`out_wrote_frame` mirrors `mediaway_{camera,desktop}_capture_poll_frame`'s own
`out_has_frame` shape: `poll_frame` returning `Ok(None)` (no new frame ready yet
— timeout/idle, not an error) is a normal, expected outcome each function must
distinguish from "wrote a frame" and from a real error, exactly as the
underlying poll functions already do. A caller drives its own loop (its own
frame-rate pacing / thread), calling one of these per tick — this is not a
blocking "record for N seconds" one-shot; that composition is left to the
caller (or a future higher-level surface), matching this crate's existing
"low-level APIs stay first-class" pattern rather than inventing a new
opinionated recording loop.

### 2. GPU release-frame handling

For `_from_desktop_capture` (Screen is GPU-only, `device.h`'s own scope), after
a successful poll the capture handle holds the GPU frame slot until
`release_frame` is called (`adr/device/0001-capture-c-abi.md` §9,
`adr/device/0003-gpu-handle-c-abi.md` §8's read-window contract). This function
calls the capture handle's `release_frame` **unconditionally after the push
attempt** (success or failure) — matching the same "must be called before the
next acquiring poll" obligation `mediaway_desktop_capture_release_frame` already
documents, just performed internally instead of left to the caller. If both the
push and the release fail, the push's status is returned (the more actionable
error for a caller trying to encode); the release failure is not silently
dropped — see §4.

`_from_camera_capture`'s `release_frame` is a documented no-op for the Camera
backend today (`adr/device/0001-capture-c-abi.md` § Panic safety /
`mediaway_camera_capture_release_frame`'s own doc), but is still called for the
same "before the next poll" contract symmetry — cheap, and future-proofs against
a Camera backend that someday does hold a resource.

### 3. No frame-type conversion, no extra copy

Internally: `capture.inner.poll_frame()` returns `Option<mediaway_common::VideoFrame>`
directly; on `Some(frame)`, `session.inner.write_frame(&frame)` consumes it by
reference — the same `VideoFrame`/`VideoFrameStorage` value moves from capture to
encode with no FFI-boundary struct translation and no CPU copy for the GPU
(Screen) case, unlike the hand-written path a caller would otherwise build via
`mediaway_desktop_frame_t` → `mediaway_video_frame_t` (which itself doesn't copy
GPU bytes either, but does require the caller to translate two distinct C
structs by hand).

### 4. Status codes — no new variants

Reuses `MediawayPipelineStatus` as-is: `HandlePoisoned` (either handle),
`InvalidArgument` (null pointers), the existing `EncoderBackendFailure`/
`EncoderClosed` for `write_frame`'s own errors, and `MediawayDeviceStatus`-class
poll/release failures are mapped onto the closest existing `MediawayPipelineStatus`
variant (`Unsupported`/`InvalidInput`/`EncoderBackendFailure`/`UnknownError`) rather
than adding a third status-fragmentation family — this bridge function's failure
modes are a strict subset of what `write_frame` alone can already report, so no
new device-specific codes are needed. `CaptureError` has no `Closed`-style variant
distinct from what device.h's own status enum already collapses, so no
`DeviceCaptureFailure`-style split is introduced here either.

### 5. Panic safety, ownership

Same `catch_unwind(AssertUnwindSafe(...))` pattern as every other function in
this header. No new opaque handle — both `session`/`capture` are pre-existing
handle types from `pipeline`/`device`; this function borrows both (`&mut`), does
not close or consume either. Thread-confined by convention, same as every other
handle in this crate.

### 6. ABI version

`MEDIAWAY_PIPELINE_FFI_ABI_VERSION` bumps 3 → 4 (new exported symbols).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| One-shot blocking "record for N seconds" function | A new opinionated recording-loop API, not a thin bridge; pacing/threading/cancellation policy belongs to the caller or a separate, explicitly-designed higher-level surface — mixing that into this ADR's scope would grow it well past "remove hand-copy boilerplate" |
| Expose `mediaway_{camera,desktop}_frame_t` and let the caller convert | The status quo gap this ADR closes — real, avoidable boilerplate + an extra struct-to-struct field copy for CPU frames |
| New status codes per underlying `CaptureError` variant | This function's failure surface is already a subset of `write_frame`'s own status set; a new status family would fragment for no caller-visible benefit (see §4) |
| Auto-release the GPU frame only on success | Leaves the capture session's GPU slot held after a failed `write_frame`, violating the existing "release before next poll" contract and risking the caller's next poll silently reusing a stale/held slot |

## Consequences

### Positive

- Closes a gap tracked since the `device` module's capture surface landed.
- Zero-Copy end-to-end for Screen capture → encode from C, with no manual struct
  translation on the caller's side.

### Negative / Trade-offs

- Two more exported functions, each depending on both `device`- and
  `pipeline`-module handle types — the first cross-module coupling at the FFI
  function-signature level in this crate (previously each module's functions
  only took its own module's handles).
- No convenience for Camera+Screen+Microphone all composed into one MP4 in a
  single call — still the caller's job to drive video/audio separately (audio
  bridge, if wanted, is its own future ADR, not covered here).

## References

- [`crates/mediaway-device/src/camera/capture.rs`](../../../mediaway-device/src/camera/capture.rs), [`desktop/video.rs`](../../../mediaway-device/src/desktop/video.rs) — `CameraCapture`/`DesktopVideoCapture` traits
- [`crates/mediaway/src/session.rs`](../../../mediaway/src/session.rs) — `EncodeSession::write_frame`
- [`adr/device/0001-capture-c-abi.md`](../device/0001-capture-c-abi.md), [`adr/device/0003-gpu-handle-c-abi.md`](../device/0003-gpu-handle-c-abi.md) — wrapped capture ABI this bridges to
- [`adr/pipeline/0001-auto-encode-c-abi.md`](0001-auto-encode-c-abi.md) — wrapped encode ABI this bridges to
- `crates/mediaway-ffi/docs/pipeline/roadmap.md`, `docs/roadmap.md` — tracked this gap

ADRs are **English**. Numbering is local to this `adr/` folder.
