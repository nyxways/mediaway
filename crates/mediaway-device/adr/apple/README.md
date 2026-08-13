# mediaway-device::apple — ADRs

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-avfoundation-camera-capture.md) | `AVFoundation` `AVCaptureSession` via `objc2`, camera capture (macOS + iOS) | Proposed |
| [0002](0002-avaudioengine-microphone-capture.md) | `AVAudioEngine` input-node tap via `objc2`, microphone capture (macOS + iOS) | Proposed |
| [0003](0003-screencapturekit-macos-screen-capture.md) | `ScreenCaptureKit` for macOS screen capture | Proposed |
| [0004](0004-replaykit-ios-inapp-screen-capture.md) | `ReplayKit` screen capture (iOS) — `RPScreenRecorder` in-app capture + Broadcast Upload Extension host contract | Proposed |

First Apple backend for this crate — camera + microphone + screen capture researched as one
vertical slice, mirroring `adr/android/`'s one-ADR-per-domain shape. **Screen capture splits into
two ADRs** (0003 macOS `ScreenCaptureKit`, 0004 iOS `ReplayKit`) — unlike camera/mic, which share
one ADR each because their underlying APIs (`AVCaptureSession`, `AVAudioEngine`) have no
per-OS split in the generated bindings, screen capture genuinely uses two unrelated frameworks;
see 0003 § "Why a separate ADR from ADR-0004" for the full reasoning. All four backends still live
in one `mediaway-device::apple` module (`crate-packaging.md`'s single `apple` platform suffix),
split by domain file, not by OS.

**ADR-0004's scope was revised after user review**: it now designs **two** iOS capture entry
points — `AppleScreenCapture` (in-app, `RPScreenRecorder`, includes app + mic audio) and
`AppleBroadcastExtensionCapture` (a host-extension-facing sink type, for use from inside a
separate `.appex` Broadcast Upload Extension target this crate cannot build itself) — plus the
full host-extension contract for the second entry point, mirroring
`adr/android/0003`'s host-`Activity` contract section rather than deferring it to a follow-up ADR.
Both entry points remain **design only** — see ADR-0004 § "A real, honest verification-gap note"
for why this domain's verification ceiling is lower than every other ADR in this set.

**Design/research only — no implementation code landed this pass.** Every ADR is **Proposed**,
each carrying its own § Open questions the user should confirm before implementation starts
(unlike `adr/android/`'s set, which is already Accepted and implemented). Grounded entirely in the
locally cloned [`objc2`](https://github.com/madsmtm/objc2) monorepo
(`local/vendor-ref/objc2/`, already present from `mediaway-encoder::apple`'s work) — no macOS/
Xcode exists in this dev environment, so zero compile verification is possible even after
implementation.

Sibling precedents: `crates/mediaway-device/adr/android/` (methodology, one-ADR-per-domain shape,
CI-verification honesty) and `crates/mediaway-encoder/adr/apple/0001-videotoolbox-h264-cpu-upload.md`
(binding-choice methodology, `objc2` dependency-review baseline, existing `apple-macos`/`apple-ios`
CI jobs this set's own CI plans reuse rather than duplicate).

Template: copy from [`mediaway-device/adr/template.md`](../../mediaway-device/adr/template.md).

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../docs/adr/).
