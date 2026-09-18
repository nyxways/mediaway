# ADR-0008: Cursor capture as a config field; the WGC border as a Windows option

- **Status**: Accepted (WGC border hardware-verified 2026-09-18 — see § Verification)
- **Date**: 2026-09-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device`

## Context

Two things a screen recorder's user can see were decided by hard-coded lines nobody could
change.

**The mouse pointer.** Each backend picked its own answer, and they disagreed:

| Backend | Before |
|---|---|
| Windows WGC (window) | hidden — `SetIsCursorCaptureEnabled(false)` |
| Windows DXGI (screen) | absent — Desktop Duplication returns the pointer separately, never composited |
| Linux portal | hidden — `CursorMode::Hidden` |
| macOS `ScreenCaptureKit` | **shown** — `setShowsCursor(true)` |

A QA recorder built on this (qarec) needs the pointer: "I clicked here and nothing happened"
is half of a bug report. No caller could ask for it.

**The yellow WGC border.** Windows draws a border around any window being captured through
WGC. `wgc.rs` said, above the cursor call:

```rust
// Best-effort: hide yellow border / cursor when the OS build supports it.
let _ = session.SetIsCursorCaptureEnabled(false);
```

Nothing there touched the border. `SetIsBorderRequired` was never called. The comment was the
only evidence of the feature, and it was wrong.

## Decision

> **Cursor:** `DesktopVideoCaptureConfig::cursor: CursorCapture { Excluded (default),
> Included }`. A backend honours it or rejects `Included` at `open` with
> `CaptureError::Unsupported`. It never ignores it.
>
> **Border:** `windows_desktop::CaptureBorder { Shown (default), Hidden }`, passed to
> `WindowsWindowCapture::open_with`. A refused `Hidden` does not fail the open;
> `WindowsWindowCapture::border_hidden()` reports what the OS actually did.

### Why the two are handled differently

The pointer is **content**: it is in the frames or it is not. A backend that silently records
without it produces a file that looks right and is wrong. Hence reject.

The border is **not content**. The compositor draws it on screen, over the captured window,
and it never reaches a frame. Failing the capture because the OS kept its privacy indicator
would lose the recording to protect something the recording does not contain. Hence best
effort, reported back.

The border is also Windows-only — only WGC draws one — so it is a Windows argument, not a
field every platform would have to document as ignored.

### Per backend

| Backend | `Excluded` | `Included` |
|---|---|---|
| Windows WGC | `SetIsCursorCaptureEnabled(false)` | `SetIsCursorCaptureEnabled(true)` |
| Windows DXGI | as before | **rejected** — compositing the DDA pointer shape is not implemented |
| Linux portal | `CursorMode::Hidden` | `CursorMode::Embedded` |
| macOS `ScreenCaptureKit` | `setShowsCursor(false)` | `setShowsCursor(true)` |
| iOS via `mediaway::platform::ScreenCapture` | as before | **rejected** — no pointer |

On WGC, failing to apply `cursor` is an error **in both directions**. WGC's own default is to
include the pointer, so on a build without `SetIsCursorCaptureEnabled` (before Windows 10
2004) an `Excluded` request would otherwise record a pointer nobody asked for. That replaces a
`let _ =` that discarded the result.

Android `MediaProjection` and iOS `ReplayKit` take their own configs, not this one, and are
unaffected.

### The border, concretely

Two steps, either of which can refuse. First, `GraphicsCaptureAccess::RequestAccessAsync(
Borderless)` must return `Allowed`. Then the session must accept `SetIsBorderRequired(false)`.
The result is read back from `IsBorderRequired` rather than inferred from the request.

### Even-cropped frames (added with the options struct)

`WindowCaptureOptions::dimensions: FrameDimensions { Native (default), EvenCropped }`. With
`EvenCropped`, each odd axis loses its last column or row, so frames always suit a hardware
encoder: 4:2:0 has no half chroma sample, and a live Unity window at 1137×636 could not be
encoded at all.

It costs nothing, because **WGC crops to its frame pool rather than scaling** — measured
2026-09-19 against a 2560×1392 window: a 2459×1341 pool produced a 2459×1341 texture whose
3 297 519 pixels all matched the full capture's top-left region. So the pool is simply
created one pixel smaller on an odd axis.

Two things had to change for that to be safe:

- **Resize detection compares against the target pool size, not `ContentSize`.**
  `ContentSize` keeps reporting the full window when the pool is smaller (measured), so
  comparing it directly would recreate the pool on every frame.
- **A delivered frame's size is read from its texture.** After a resize, the frame in hand
  came out of the *old* pool, and the code reported the *new* geometry for it. That was
  wrong before this change too, but it only became visible once pool and content sizes could
  legitimately differ.

Cropping rather than padding: a padded frame contains pixels the source never drew, which
in a recording used as evidence can be mistaken for a rendering bug.

Arbitrary-rectangle crop — recording a region of a window — is **not** this. It needs a GPU
copy (the pool trick only removes right/bottom edges) and remains open.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Default `cursor` to `Included` | Changes behaviour for every Windows and Linux caller, and makes DXGI screen capture fail by default. qarec opts in instead. |
| Keep per-backend defaults (`Option<CursorCapture>`, `None` = "whatever the backend did") | Freezes an accident — macOS disagreeing with everyone else — into the API. |
| Silently ignore `Included` where unsupported | The failure this ADR exists to prevent. |
| Composite the DDA pointer on DXGI | Real work: the pointer shape comes back as monochrome, colour or masked-colour bitmaps that have to be blended into a GPU texture. Worth doing, but separate. Until then `Included` is refused, so nobody is misled. |
| Make the border a cross-platform config field | Every non-Windows backend would carry a field it must document as meaningless. |
| Fail `open` when `Hidden` is refused | Loses a recording over something not in it. |
| Always hide the border | Removes an OS privacy indicator on behalf of callers who never asked. Some apps want it. |

## Consequences

### Positive

- Callers can have the pointer where the platform can provide it, and are told plainly
  where it cannot.
- The WGC border can be hidden, and the caller learns whether it was.
- The misleading `wgc.rs` comment is gone. The `let _ =` that hid a cursor-setting failure
  is gone.

### Negative / Trade-offs

- **Breaking:** `DesktopVideoCaptureConfig` gains a public field, so struct-literal callers
  must add it. The two constructors set it.
- **Behaviour change on macOS:** the pointer was always shown and is now hidden by default.
- `Included` on DXGI is refused until the pointer is composited. A screen recorder that
  needs the pointer on Windows must use WGC window capture for now.
- Asking for `Hidden` blocks `open` on `RequestAccessAsync`. That is immediate for an
  unpackaged process (measured), but may be a user prompt for a packaged app.
- The C ABI (`mediaway-ffi`) exposes neither setting yet. It passes `Excluded`, which is
  what every C caller already got.

## Verification

- `RequestAccessAsync(Borderless)` → `AppCapabilityAccessStatus::Allowed` for an unpackaged
  desktop process, Windows 11 Pro 26100, no prompt (2026-09-18).
- Default-suite tests: the default is `Excluded` for both constructors; DXGI rejects
  `Included` before touching a device.
- `wgc_window_capture_applies_cursor_border_and_even_crop` (`#[ignore]`d — it shows a
  window) opens a real WGC session with `Included` + `Hidden` + `EvenCropped`. It asserts
  `border_hidden()`, that a 321×243 capture is delivered at exactly 320×242, and that plain
  `open` leaves the border shown. It also asserts the uncropped size really was odd, so it
  cannot pass without exercising the crop. **Run 2026-09-19 on Windows 11 Pro 26100:
  passes.** It was also shown to *fail* with `EvenCropped` turned into a no-op. An earlier
  version computed its expectation with the function under test and passed that mutant.
- Through qarec, on a live Unity editor (2026-09-19):
  - **Cursor:** the pointer was moved over the window during a 2560×1392 recording and
    appears in the frames. A frame diff found a 20–40 px region moving in 361 of 479
    frames, and a zoomed crop shows it.
  - **Border:** no border-refused warning was logged.
  - **Even crop:** the window was resized so WGC captured 1187×695. The recording came out
    1186×694 HEVC, with all 86 frames decoded, distinct presentation timestamps, and no
    ffmpeg warnings. At that size the hardware encoder had refused to open before this
    change.

## References

- [`GraphicsCaptureSession.IsBorderRequired`](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired)
- [`GraphicsCaptureAccess.RequestAccessAsync`](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscaptureaccess.requestaccessasync)
- [`GraphicsCaptureSession.IsCursorCaptureEnabled`](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.iscursorcaptureenabled)
- `adr/windows/0004-wgc-window-capture.md`, `adr/linux/0001-portal-pipewire-screen-capture.md`
