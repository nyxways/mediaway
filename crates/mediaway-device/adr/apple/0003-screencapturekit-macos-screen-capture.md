# ADR-0003: `ScreenCaptureKit` for macOS screen capture

- **Status**: Accepted
- **Date**: 2026-08-12
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device` (module `mediaway-device::apple`, macOS-only surface within it —
  see § Why a separate ADR from ADR-0004 (iOS))

## Context

Screen capture is the hardest domain in this set, mirroring `adr/android/0003`'s framing ("the
hardest domain, deserves the most research effort"). **Unlike camera (ADR-0001) and mic
(ADR-0002), macOS and iOS screen capture use genuinely different frameworks with no shared API
surface** — `ScreenCaptureKit` (macOS 12.3+, confirmed macOS-only via
`objc2-screen-capture-kit`'s own `Cargo.toml` `keywords = [..., "macos"]`, no `"ios"`) vs.
`ReplayKit` (both OSes, but with a materially different in-app-vs-extension split on iOS — see
ADR-0004). This ADR covers **macOS only**; ADR-0004 covers iOS.

### Why a separate ADR from ADR-0004 (iOS), unlike camera/mic sharing one ADR

ADR-0001/0002 found camera/mic APIs (`AVCaptureSession`, `AVAudioEngine`) declared with **no**
per-OS `#[cfg(target_os = ...)]` split at the item level in the generated bindings — a real,
grounded reason to keep one ADR/module file. Screen capture has the opposite property:
`ScreenCaptureKit`'s framework crate itself is scoped macOS-only (confirmed via its `Cargo.toml`),
and `ReplayKit`'s in-app-capture entry point (`RPScreenRecorder`) is a **completely different**
class/protocol surface with no relationship to `SCStream`. Forcing both into one ADR (or one
source file) would hide a real architectural split behind an artificial "one apple screen ADR"
framing — same reasoning ADR-0003 in the Android set used to justify a **dedicated**
`AndroidScreenCaptureConfig` instead of reusing `DesktopVideoCaptureConfig`. Both macOS and iOS
screen backends still live under the single `mediaway-device::apple` **module** (per
`crate-packaging.md`'s "apple" platform suffix), just in separate files
(`apple/screencapturekit.rs` macOS-only, `apple/replaykit.rs` iOS-only) and separate ADRs.

## Research: `ScreenCaptureKit` is the modern, correct API

Confirmed via `local/vendor-ref/objc2/framework-crates/objc2-screen-capture-kit/` (`generated/
ScreenCaptureKit/{SCStream,SCShareableContent,SCContentSharingPicker,SCRecordingOutput,
SCScreenshotManager,SCError}.rs`) — Apple's stated replacement for the deprecated `CGDisplayStream`
(`CoreGraphics`) capture path. No `CGDisplayStream` symbols were found in the local
`generated/CoreGraphics/` clone during this pass (not exhaustively searched, but no hits for
"DisplayStream") — consistent with it being legacy/deprecated, reinforcing `ScreenCaptureKit` as
the only realistic modern choice, matching how `windows-capture.md`'s DXGI Desktop Duplication
superseded GDI screen capture and `linux-capture.md`'s portal+PipeWire superseded raw X11.

### Session flow (confirmed real signatures)

1. `SCShareableContent::getShareableContentWithCompletionHandler` (confirmed real,
   `SCShareableContent.rs` line ~237) — **async, completion-handler-based** (`block2` required),
   returns available displays/windows/applications for filter construction. Real cost: this alone
   means `open()` cannot be a plain synchronous call the way DXGI's `DuplicateOutput` or Media
   Foundation's `MFEnumDeviceSources` are — needs a bounded channel/condvar bridge to stay
   synchronous at this crate's trait boundary, similar in spirit to (but architecturally distinct
   from) Android's originally-proposed-then-retracted Camera2 async-open bridge (`adr/android/
   0001`'s "Real correction" section) — except here the async completion-handler pattern is
   **confirmed real**, not a misreading.
2. `SCContentFilter::initWithDisplay_excludingWindows` (or `..includingWindows`,
   `..includingApplications_exceptingWindows`, `..excludingApplications_exceptingWindows` — all
   confirmed real, `SCStream.rs` lines 254–329) — selects **what** to capture. `Select::Default` →
   primary display, all windows included (`excludingWindows` with an empty array) mirrors the
   "primary output, everything" default every other screen-capture backend in this crate uses.
3. `SCStreamConfiguration::new()` → `.setWidth`/`.setHeight` (defaults 1920×1080, confirmed real)
   → `.setPixelFormat` (`OSType`, real property confirmed line ~448 — accepts the same
   `kCVPixelFormatType_*` constants ADR-0001 uses) → `.setShowsCursor`/`.setMinimumFrameInterval`
   (confirmed real).
4. `SCStream::initWithFilter_configuration_delegate(filter, config, delegate)` — confirmed real,
   requires an `Option<&ProtocolObject<dyn SCStreamDelegate>>` (a **third** delegate protocol this
   `apple` module needs, alongside ADR-0001's `AVCaptureVideoDataOutputSampleBufferDelegate`
   — `SCStreamDelegate` reports stream-level errors, e.g. `didStopWithError`).
5. `stream.addStreamOutput_type_sampleHandlerQueue_error(&output, SCStreamOutputType::Screen,
   Some(queue))` — confirmed real, `SCStream.rs` line ~924 — requires a **fourth** protocol
   conformance, `SCStreamOutput` (delivers `CMSampleBuffer` frames, same
   `define_class!`-with-ivars pattern ADR-0001 already establishes for
   `AVCaptureVideoDataOutputSampleBufferDelegate`, reused here rather than re-derived).
6. `stream.startCaptureWithCompletionHandler(handler)` — confirmed real, **async**
   (`block2::SendableBlock`, line ~988) — same synchronous-bridge need as step 1.
7. Frame delivery: `SCStreamOutput`'s callback fires with a `CMSampleBuffer` — same
   `image_buffer()` → `CVPixelBuffer` → lock/read-plane extraction ADR-0001 already establishes,
   directly reusable (not re-derived) for this domain.
8. `stream.stopCaptureWithCompletionHandler(handler)` on `close()` — same async-completion
   pattern.

### A real, notable finding: `SCStreamOutputType` includes a `Microphone` case

`generated/ScreenCaptureKit/SCStream.rs` (`SCStreamOutputType`, confirmed real): `Screen`,
`Audio` (system/app audio being captured alongside the screen), **and `Microphone`** — a newer
macOS capability letting `SCStream` itself capture a selected microphone synchronized with the
screen/system-audio stream, without a separate `AVAudioEngine` session. **Out of scope this ADR**
(this ADR's slice is `Screen` output type only, matching every other backend's video-only first
cut), but worth flagging: a later stage could unify "screen + system audio + mic, all
time-synchronized" behind one `SCStream` session rather than three independent captures — a real
architectural option this crate does not have on any other platform today.

## Research: permission model — genuinely simpler than Android's `MediaProjection`

Confirmed real, plain (non-`unsafe fn`!) C functions in `generated/CoreGraphics/CGWindow.rs`:

```rust
pub fn CGPreflightScreenCaptureAccess() -> bool;  // cheap, no dialog, no side effect
pub fn CGRequestScreenCaptureAccess() -> bool;    // triggers the TCC consent dialog if not yet decided
```

This is **materially simpler than Android's `MediaProjection`** (`adr/android/0003`): macOS Screen
Recording access is a **one-time system TCC (Transparency, Consent, and Control) permission
grant**, checked/requested via these two plain functions — no per-session `Intent`/`Activity`
consent dance, no host-app-supplied JNI handle, no Android-14-style "consent is single-use, ask
again every session" gotcha. Once granted, it persists across app launches (revocable only via
System Settings). This maps cleanly onto this crate's existing `capabilities.rs` two-tier design
(`support`/`request_permission`, ADR-0003 workspace-level): `support(Screen)` → `CGPreflightScreenCaptureAccess()`
(cheap, no dialog); `request_permission(Screen)` → `CGRequestScreenCaptureAccess()` (real,
possibly-dialog-showing probe, same cost class as Windows mic's "open a real endpoint and observe"
pattern, but for screen instead — a domain Windows itself cannot cheaply probe at all,
`capabilities.md`'s own documented gap).

**One real, unconfirmed detail**: whether `SCShareableContent.current`/`getShareableContentWithCompletionHandler`
itself independently triggers or requires the same TCC prompt (Apple's documented behavior,
**not verified from local `objc2` source** — the generated bindings carry no runtime-permission
annotations). Flagged in § Open questions rather than assumed either way.

## Decision

> Depend on **`objc2-screen-capture-kit`** (features: `SCStream`, `SCShareableContent`, `block2`,
> `objc2-core-media`, `dispatch2`) + reuse ADR-0001's `objc2-core-video`/`objc2-core-foundation`
> feature set, as a **`[target.'cfg(target_os = "macos")'.dependencies]`** entry (not `any(macos,
> ios)` — this domain is genuinely macOS-only, unlike ADR-0001/0002). New module
> `mediaway-device::apple::screencapturekit` (`AppleScreenCapture`, implementing
> `crate::desktop::DesktopVideoCapture`), `#[cfg(target_os = "macos")]`.

- Two new `define_class!` types: a minimal `SCStreamDelegate` conformance (stream-error
  reporting → `CaptureError::Backend`/`DeviceLost`) and an `SCStreamOutput` conformance (frame
  delivery → the same `Arc<Mutex<VecDeque<VideoFrame>>>` shared-queue shape ADR-0001 already
  establishes for the camera delegate — genuinely reused, not re-derived).
- `open()` bridges the two async completion-handler calls (`getShareableContentWithCompletionHandler`,
  `startCaptureWithCompletionHandler`) to a synchronous return via a bounded channel, on a
  dedicated worker thread — the **first** genuinely-confirmed-async open sequence in this crate
  (every other backend's `open()` is synchronous or `block_on`-wrapped over a sync-shaped async
  call, e.g. Linux portal's D-Bus round trip).
- `DesktopCaptureSource::Screen { select }` (the **existing** type, ADR-0001 in the base
  `mediaway-device` ADR set) is reused as-is — unlike Android's screen capture, this domain needs
  no foreign-object handle from a host app, so there is no reason to introduce a
  macOS-specific config type the way `AndroidScreenCaptureConfig` was needed.

## ZCA / typestate shape

| Existing precedent | ScreenCaptureKit | Difference |
|---|---|---|
| DXGI/portal: synchronous or `block_on`-wrapped session setup | Two real async completion-handler calls bridged via a bounded channel on a worker thread | First **genuinely async** `open()` sequence in this crate (Android's Camera2 async-open was a misreading later corrected; this one is confirmed real) |
| ADR-0001's camera delegate (`AVCaptureVideoDataOutputSampleBufferDelegate`) | Two delegates (`SCStreamDelegate` + `SCStreamOutput`), reusing the exact same `define_class!`-with-`Arc<Mutex<VecDeque<VideoFrame>>>`-ivar pattern | Direct reuse of ADR-0001's established pattern, not a new one |
| `DesktopCaptureSource::Screen { select }` — existing, platform-agnostic type | Reused verbatim | No new config type needed, unlike Android screen capture |
| `GpuBufferHandle::Metal` — deferred (ADR-0001) | Same, sourced from the `CVPixelBuffer`/`IOSurface` the `SCStreamOutput` callback receives | No new type-design work |

No `Box<dyn _>` planned. `AppleScreenCapture` wraps a concrete `Option<Inner>`, matching every
other backend.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `CGDisplayStream` (`CoreGraphics`, older API) | Deprecated in favor of `ScreenCaptureKit` per Apple's own platform direction; no symbols found in this pass's local grounding search, reinforcing it as legacy. Rejected. |
| `AVCaptureScreenInput` (older `AVFoundation` screen-to-`AVCaptureSession` bridge) | An older, narrower API predating `ScreenCaptureKit`; not found as a distinctly reviewed surface this pass — `ScreenCaptureKit` is Apple's current stated direction for screen capture specifically, so this alternative was not pursued further. |
| Defer macOS screen capture to a follow-up ADR, ship camera+mic only this pass | Contradicts this task's explicit direction to cover all three domains as one research pass (mirroring Android's "camera + mic + screen 모두" vertical-slice precedent) — not taken, though implementation itself may still land in stages. |

## Dependency review (`docs/conventions/deps-policy.md`)

| Question | Answer |
|----------|--------|
| Need | Real — `ScreenCaptureKit` is Apple's current, non-deprecated screen capture API on macOS. |
| License | Same `Zlib OR Apache-2.0 OR MIT` workspace line, same `madsmtm/objc2` monorepo as ADR-0001/0002 — not re-litigated. |
| New surface vs. ADR-0001/0002 | `objc2-screen-capture-kit` itself is new; its own `Cargo.toml` shows it optionally depends on `objc2-av-foundation` (for `AVMediaFormat`/`AVVideoSettings` types) — already a dependency from ADR-0001, so this is additive, not a fork. `dispatch2`/`block2` already introduced by ADR-0001/0002. |
| Platform scope | **macOS-only** dependency (`[target.'cfg(target_os = "macos")'.dependencies]`) — unlike ADR-0001/0002's `any(macos, ios)` gate, this crate must not pull `objc2-screen-capture-kit` into iOS builds at all (it would not compile there; the framework does not exist on iOS). |
| Alternatives | See § Alternatives Considered. |

## ⚠️ CI verification plan

Reuses the `apple-macos` job only (native macOS runner) — **not** `apple-ios` (this domain is
macOS-only, so no iOS cross-compile step is meaningful for it; `apple-ios`'s existing
`mediaway-encoder` lint stays unaffected by this ADR). **Zero compile verification as authored.**

## Open questions (for user confirmation)

1. **`SCShareableContent`/TCC prompt interaction** — whether requesting shareable content itself
   can trigger the consent dialog independent of `CGRequestScreenCaptureAccess`, per Apple's
   official documentation (not verified from local `objc2` source this pass).
2. **Async-bridge design for `open()`** — channel timeout value, and whether a failed/timed-out
   bridge should retry or fail the whole `open()` — no real hardware this session to tune against.
3. **`SCStreamOutputType::Microphone`** — confirm this is genuinely out of scope for this ADR
   (Screen output type only) rather than something the user wants folded in now, given the real
   architectural opportunity noted in § A real, notable finding.
4. **Minimum OS floor**: **macOS 12.3+ is the well-known public floor for `ScreenCaptureKit`**
   (general Apple platform knowledge — **not verified from local `objc2` source**, which carries
   no `@available` annotations as compile-time facts). If confirmed, this is the **strictest**
   floor across this whole ADR set (stricter than camera/mic's much older floors) — see ADR-0001
   § Open questions #1 for the cross-domain floor-unification question this reinforces.
5. **Window-level capture** (`DesktopCaptureSource::Window`) via `SCContentFilter::
   initWithDesktopIndependentWindow` (confirmed real, § Research above) — in scope alongside
   `Screen`, or deferred to a follow-up ADR? Not decided in this pass; the API clearly supports it.

## Decisions confirmed with the user (2026-08-12)

1. **`SCStreamOutputType::Microphone` (§ Open questions #3) — resolved: not used this pass**.
   `SCStream` stays `Screen`-output-type only (§ Decision, unchanged) — this crate does **not**
   fold microphone capture into the macOS `ScreenCaptureKit` session. `AppleMicrophoneCapture`
   (ADR-0002, `AVAudioEngine`) remains the sole microphone backend on macOS, same as it is on iOS
   — see ADR-0002 § Decisions confirmed with the user for the reciprocal note. The real
   architectural option this finding raised (screen + system audio + mic, all time-synchronized in
   one `SCStream` session) remains available to revisit later but is not adopted now.

## Decisions confirmed with the user (2026-08-12)

1. **`SCStreamOutputType::Microphone`**: confirmed **not used** — `AppleMicrophoneCapture`
   (ADR-0002, `AVAudioEngine`) stays the sole mic backend on both macOS and iOS. `SCStream`'s
   own microphone-synchronized capture is a real, documented architectural option (§ "A real,
   notable finding") but deliberately not adopted this pass.

## Implementation notes (2026-08-13, written alongside the code)

- All three completion-handler bridges (`getShareableContentWithCompletionHandler`,
  `startCaptureWithCompletionHandler`, `stopCaptureWithCompletionHandler`) are confirmed real
  `block2::SendableBlock` (`Send + Sync`-bounded) calls, resolving open question #2's design
  shape — each bridged via a `Mutex<Option<SyncSender<...>>>` "take once" pattern so a
  (contract-violating) multiple-fire completion handler can never panic.
- `SCShareableContent`/`SCStream`/`SCStreamConfiguration`/`SCContentFilter`'s setter methods
  (`setWidth`/`setHeight`/`setPixelFormat`/`setShowsCursor`, etc.) are confirmed plain, safe
  `pub fn` — no `unsafe` needed for stream configuration, only for object construction/session
  wiring calls that take ownership or cross a delegate-protocol boundary.
- Reuses `apple::pixel::extract_nv12` (shared with `apple::camera`) verbatim for `SCStreamOutput`
  frame delivery — no duplicated `CVPixelBuffer` extraction code.

## Consequences

### Positive

- Identified the modern, correct, non-deprecated macOS screen-capture API with real API-shape
  grounding, including two delegate protocols this domain needs and their reuse of ADR-0001's
  already-established `define_class!` pattern.
- Confirmed a genuinely simpler permission model than Android's `MediaProjection` — two plain,
  safe C function calls (`CGPreflightScreenCaptureAccess`/`CGRequestScreenCaptureAccess`), no
  host-app JNI/Activity contract needed at all.
- Found a real architectural opportunity (`SCStreamOutputType::Microphone`) unique to this
  platform among everything else in this crate — flagged for a deliberate future decision rather
  than silently ignored.

### Negative / Trade-offs

- **Zero compile verification as authored**, same class as every Apple ADR this session.
- First genuinely-confirmed-async `open()` sequence in this crate — a new synchronous-bridge
  pattern with no exact precedent to copy verbatim (Linux portal's `block_on` is architecturally
  different: wraps a sync-shaped protocol, not two independent completion-handler calls).
- Real, unconfirmed detail (`SCShareableContent`'s own TCC interaction) left open, not resolved.
- Adds a **macOS-only** dependency to a crate whose other Apple-domain dependencies (ADR-0001/
  0002) are `any(macos, ios)` — a real, deliberate asymmetry within the same `apple` module that
  must be modeled correctly in `Cargo.toml`'s target-cfg sections, not glossed over.

## References

- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
- [`docs/spec/crate-packaging.md`](../../../../docs/spec/crate-packaging.md) — `apple` platform
  suffix (single module; per-domain files, not per-domain modules)
- `adr/android/0003-mediaprojection-jni-screen-capture.md` — "hardest domain, most research"
  precedent this ADR mirrors, and the permission-model contrast this ADR's § Research draws
  against
- ADR-0001 (this set) — reused `define_class!` delegate pattern, `CVPixelBuffer` frame-extraction
  path, shared minimum-OS-floor open question
- `mediaway-device`'s existing `DesktopCaptureSource`/`DesktopVideoCaptureConfig`
  (`crates/mediaway-device/src/desktop/video.rs`) — reused as-is, unlike Android's dedicated config
- `windows-capture.md`/`linux-capture.md` — the DXGI/portal precedent for "modern API supersedes
  a legacy one" this ADR's `CGDisplayStream` → `ScreenCaptureKit` framing mirrors
- Local grounding source (read directly, not web-fetched):
  `local/vendor-ref/objc2/generated/ScreenCaptureKit/{SCStream,SCShareableContent}.rs`,
  `local/vendor-ref/objc2/generated/CoreGraphics/CGWindow.rs`,
  `local/vendor-ref/objc2/framework-crates/objc2-screen-capture-kit/Cargo.toml`
- [`objc2` on GitHub](https://github.com/madsmtm/objc2) (`Zlib OR Apache-2.0 OR MIT`)
- [ScreenCaptureKit — Apple Developer](https://developer.apple.com/documentation/screencapturekit)
  (public knowledge reference for the macOS 12.3+ floor — not fetched this pass, see § Open
  questions #4)
