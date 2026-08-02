# ADR-0003: Portal `SourceType::Window` for Linux window capture

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-linux`

## Context

`CaptureSource::Window` had no Linux backend (ADR-0001 explicitly deferred
it, marked "not attempted this session" in stage 2 of `docs/roadmap.md`).
`org.freedesktop.portal.ScreenCast`'s `SourceType` has carried a `Window` bit
(`= 2`, alongside `Monitor = 1` and `Virtual = 4`) since the interface's
first version — `ashpd::desktop::screencast::SourceType::Window` already
exists in the `ashpd` 0.13 dependency this crate already carries for screen
capture (ADR-0001). This is a genuine small extension of the existing
recipe, not a new subsystem.

## Decision

> Reuse the entire portal handshake + `PipeWire` stream-connect flow
> ADR-0001 established for [`LinuxScreenCapture`](../src/screencast.rs),
> factored into a shared `screencast::open_session(source_type, media_role,
> config)` function:
>
> - [`LinuxWindowCapture::open`](../src/window.rs) validates
>   `CaptureSource::Window`, then calls `open_session(SourceType::Window,
>   "Window", config)`.
> - [`LinuxScreenCapture::open`](../src/screencast.rs) validates
>   `CaptureSource::Screen { output_index: 0 }`, then calls
>   `open_session(SourceType::Monitor, "Screen", config)`.
>
> Both share one `Session` type (portal handshake, `PipeWire` main loop,
> frame queue, `stream_info`/`poll_frame`/`close`) — the only differences are
> the requested `SourceType` (which picker the portal shows) and the
> `PipeWire` stream's `MEDIA_ROLE` property (`"Screen"` vs. `"Window"`, a
> routing hint, set via a runtime `Properties::insert` since `MEDIA_ROLE`
> varies per caller and the `properties!` macro only accepts compile-time
> literals).
>
> The [`CaptureSource::Window`] `window` field (an opaque `NativeHandle`) is
> **ignored** — same status as `Screen`'s `output_index`: the portal's own
> picker UI chooses which window interactively; there is no
> `org.freedesktop.portal.ScreenCast` call to target a specific window handle
> programmatically the way `WGC`'s `CreateForWindow(HWND)` does on Windows.
> Any [`CaptureSource::Window`] value opens the picker.
>
> `CaptureOutputPreference::ZeroCopyGpu` → `CaptureError::Unsupported`, same
> reasoning as ADR-0001 (CPU-copy-only this session, never silently served
> from CPU when GPU was requested).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Duplicate `screencast.rs`'s ~300 lines into `window.rs` | The portal handshake, `PipeWire` connect/format-negotiate/`process`-callback plumbing is byte-for-byte identical for `Monitor` and `Window` sources — only the `SourceType` and `MEDIA_ROLE` differ. Duplicating it would mean every future fix (e.g. the DMA-BUF Zero-Copy follow-up ADR-0001 flags) has to land twice. |
| A generic `LinuxPortalCapture<const ROLE: …>` type shared by both public structs | No caller-visible benefit — `mediaway-device`'s `VideoCapture` trait is already the shared public contract; two thin wrapper structs (`LinuxScreenCapture`, `LinuxWindowCapture`) delegating to one internal `Session` type is simpler than a generic/const-parameterized public type, and matches this crate's and `mediaway-device-windows`'s existing convention of one file per capture kind with its own (small, duplicated) trait-impl boilerplate. |

## Consequences

### Positive

- Real, complete window-capture path — not a stub — sharing 100% of the
  hardened portal/`PipeWire` plumbing ADR-0001 already established (same
  DMA-BUF-rejection safety, same buffer-lifetime copy contract).
- Confirms the crate's existing "the portal is the mediation layer" design
  extends cleanly to a second `SourceType` without new dependencies.

### Negative / Trade-offs

- No programmatic "capture *this* window" — callers porting Win32 `HWND`
  logic 1:1 must special-case Linux (same caveat ADR-0001 already documents
  for `output_index`).
- Cursor is always hidden (`CursorMode::Hidden`, same as screen capture) —
  not re-evaluated for whether `Embedded`/`Metadata` makes more sense for a
  single-window capture UX; deferred, not a considered-and-rejected decision.

## Zero runtime hardware/session verification this session

**No real desktop portal session was exercised** — same WSL2 environment gap
ADR-0001 documents (no session bus + compositor backend). The `_or_skip` test
(`window_tests.rs`) is written to run the real path and is expected to
**skip** here for exactly this reason.

## References

- [ADR-0001](0001-portal-pipewire-screen-capture.md) — the screen-capture
  recipe this extends; all its Zero-Copy-status and dependency-review content
  applies unchanged here.
- `ashpd::desktop::screencast::SourceType`: <https://docs.rs/ashpd/latest/ashpd/desktop/screencast/enum.SourceType.html>
- Windows precedent: [`mediaway-device-windows/adr/0004-wgc-window-capture.md`](../../mediaway-device-windows/adr/0004-wgc-window-capture.md)
