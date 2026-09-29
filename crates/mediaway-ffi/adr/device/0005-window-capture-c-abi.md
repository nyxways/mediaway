# ADR-0005: Window capture and capture options over the C ABI

- **Status**: Accepted — hardware-verified through the C ABI and the C++, C#, Python and Node.js bindings
- **Date**: 2026-09-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi` (device domain)

## Context

`mediaway_desktop_capture_open` returns `UNSUPPORTED` for every `Window` config. ADR-0001 deferred
it as "a separate HWND-input gap", and ADR-0003 unblocked only Screen. Since v0.1.8 the Rust side
gained what a window recorder needs (`mediaway-device` ADR-0008 / ADR-0009):

- `CursorCapture` (`Excluded` default, `Included`),
- `CaptureRegion` (WGC only; a region off the origin costs one GPU copy per frame),
- `WindowCaptureOptions`: `CaptureBorder::Hidden` and `FrameDimensions::EvenCropped`,
- `WindowsWindowCapture::border_hidden()`, which reports what the OS actually did.

The C ABI pins `cursor: Excluded` and `region: None` and has no way to pass an `HWND`, so none of
this is reachable from C, C++, C#, Python or Node.

## Decision

> Extend `mediaway_desktop_capture_config_t` in place, make the Window source real on Windows,
> and add one query for the border outcome. `MEDIAWAY_DEVICE_FFI_ABI_VERSION` goes 1 → 2.

1. **Append fields to the config struct; zero means today's behaviour.** Every new field's zero
   value is the previous behaviour, so a zero-initialised struct behaves exactly as before:

   | Field | Zero value | Non-zero |
   |-------|-----------|----------|
   | `uint64_t window_handle` | — (Window only) | the `HWND` bits |
   | `mediaway_capture_cursor_t cursor` | `EXCLUDED` | `INCLUDED` |
   | `bool region_enabled` + `region_x/y/width/height` (`uint32_t`) | whole surface | that rectangle |
   | `mediaway_capture_border_t border` | `SHOWN` | `HIDDEN` (Window only) |
   | `mediaway_frame_dimensions_t dimensions` | `NATIVE` | `EVEN_CROPPED` (Window only) |

   `region` is flattened to `enabled` + four values, not a nested struct with an optional, the same
   idiom `rate_control_*` uses in the pipeline ABI. `window_handle` is `uint64_t`, not
   `uintptr_t`, so the layout is identical on 32- and 64-bit and every binding mirrors one shape.
2. **`mediaway_desktop_capture_config_window(hwnd, time_base, gpu_device)`** builds a Window
   config. `hwnd == 0` is `INVALID_INPUT`, not a crash. The `HWND` stays caller-owned; the session
   does not extend its lifetime.
3. **`mediaway_desktop_capture_border_hidden(capture, bool *out_hidden)`** returns what the OS did
   with `border = HIDDEN`. A refusal is not an error at open (`mediaway-device` ADR-0008: the
   border is drawn on screen, never into frames), so without this a caller who asked for it cannot
   tell. The value is read once at open and stored in the handle, so the handle stays a
   `Box<dyn DesktopVideoCapture>` with no downcast. Non-Window handles return `UNSUPPORTED`.
4. **New status `MEDIAWAY_DEVICE_STATUS_REGION_OUT_OF_BOUNDS = 14`** for
   `CaptureError::RegionOutOfBounds`. A window that shrank below the region is a runtime state, not
   a malformed config, so it must not share `INVALID_INPUT`. The status enum is append-only.
5. **Backend scope.** Window is `HWND` + WGC, Windows only. Linux/Apple keep returning
   `UNSUPPORTED` (their window sources are portal/`SCContentFilter` tokens, not `HWND`s).
6. **Screen now forwards the new fields instead of pinning them.** DXGI cannot draw the pointer
   or crop, so `cursor = INCLUDED` or `region_enabled` on a Screen config is `UNSUPPORTED`, never
   silently ignored. `border` and `dimensions` are Window-only options; a non-default value on a
   Screen config is `INVALID_INPUT`. **Behaviour change:** before, a Screen config's `cursor` was
   fixed to `Excluded`; nothing could ask for anything else, so no existing caller changes.

Zero-cost shape: all new fields are `Copy` values in the existing by-value config; the Window arm
opens the concrete `WindowsWindowCapture` and boxes it once into the same
`Box<dyn DesktopVideoCapture>` Screen already uses. No new `Box`, `Vec`, clone or allocation site.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| A second struct `..._config_ex_t` | Two shapes to keep in sync across five bindings, for fields whose zero value already means "as before" |
| Setter functions on a config handle | Turns a plain value struct into a heap handle with a free function |
| `uintptr_t window_handle` | Layout differs between 32/64-bit; every binding would have to branch |
| Reuse `INVALID_INPUT` for a shrunken window | A runtime condition a caller may retry or recover from, indistinguishable from a bad config |
| Expose `WindowCaptureOptions` as a separate struct argument to `open` | Changes `open`'s signature for Screen callers, who use none of it |

## Consequences

### Positive

- C, C++, C#, Python and Node can record one window, with a chosen cursor, a cropped region and
  encoder-safe even dimensions.
- Zero-initialised existing code keeps working after a recompile.

### Negative / Trade-offs

- **ABI break.** The struct grows and is passed by value (`config_screen` returns it), so callers
  must recompile and every binding's struct mirror must change: `MEDIAWAY_DEVICE_FFI_ABI_VERSION`
  1 → 2. Callers that load the library at runtime should check the version first.
- A region off the origin is one GPU copy per frame (`GpuCopy`, not Zero-Copy). The header says so
  next to the field.
- `border = HIDDEN` needs Windows 11 build 22000+; older builds report `hidden == false`.

## References

- `mediaway-device` ADR-0008 (cursor and border), ADR-0009 (capture region)
- ADR-0001 § Deferred (Window), ADR-0003 (GPU handle), pipeline ADR-0001 addendum (flat fields)
- `docs/ai/wiki/device/windows-window.md`
