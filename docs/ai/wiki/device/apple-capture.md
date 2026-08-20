# Apple capture (camera + mic + screen + macOS window) — implemented, zero compile verification until CI

- Module: `mediaway-device::apple`, `cfg(any(target_os = "macos", target_os = "ios"))` for
  camera/mic, split further by OS for screen capture (`cfg(target_os = "macos")` /
  `cfg(target_os = "ios")`): `apple/{camera,mic,pixel,screencapturekit,replaykit,capabilities}.rs`.
  `pixel.rs` is the one shared `CMSampleBuffer` → `CVPixelBuffer` CPU readback routine reused by
  camera/screencapturekit/replaykit.
- ADRs (all **Accepted**):
  [`adr/apple/0001`](../../../../crates/mediaway-device/adr/apple/0001-avfoundation-camera-capture.md) (camera) ·
  [`0002`](../../../../crates/mediaway-device/adr/apple/0002-avaudioengine-microphone-capture.md) (mic) ·
  [`0003`](../../../../crates/mediaway-device/adr/apple/0003-screencapturekit-macos-screen-capture.md) (macOS screen) ·
  [`0004`](../../../../crates/mediaway-device/adr/apple/0004-replaykit-ios-inapp-screen-capture.md) (iOS screen).
- **Zero compile verification, zero real-hardware verification** — no macOS/Xcode in this dev
  environment; `apple-macos`/`apple-ios` CI jobs now lint `mediaway-device` alongside
  `mediaway-encoder`.

## Camera (`AppleCameraCapture`, `AVCaptureSession` + `objc2`)

Real Cocoa/Objective-C (unlike VideoToolbox's plain-C API) — needs the full `objc2`/
`objc2-foundation` runtime bridge. Frame delivery is a delegate protocol
(`AVCaptureVideoDataOutputSampleBufferDelegate`), implemented via `objc2`'s `define_class!` — the
first Objective-C delegate-class pattern in this workspace; delegate ivars hold plain Rust types
directly (`Arc<Mutex<VecDeque<VideoFrame>>>`), `#[thread_kind = AnyThread]` (no `MainThreadOnly`
bound on the protocol), stays headless-usable. Real correction found while implementing:
`AVCaptureSession::startRunning()` is synchronous — `open()` needs no worker-thread bridge, just
calls it directly. Frames: `CMSampleBuffer` → `image_buffer()` → `downcast::<CVPixelBuffer>()` →
`lock_base_address`/`base_address_of_plane`/`unlock_base_address` (`apple::pixel::extract_nv12`,
shared with the two screen backends). Color range: `VideoRange` (real camera hardware convention,
diverging from the encoder's `FullRange` choice for synthetic input).

## Mic (`AppleMicrophoneCapture`, `AVAudioEngine` tap)

`AVAudioEngine.inputNode` + `installTapOnBus:bufferSize:format:block:` — a plain `block2::RcBlock`
closure, no delegate class. Real finding: `AVAudioPCMBuffer::floatChannelData` is **planar** (one
pointer per channel); `AudioFrame::data` is documented interleaved — every callback interleaves N
channels into one buffer (`interleave_pcm_f32`), a real per-frame cost this domain alone pays
among this crate's mic backends.

## Screen — two backends (macOS `ScreenCaptureKit` / iOS `ReplayKit`)

**macOS**: `SCStream` + `SCContentFilter` + `SCStreamConfiguration`, two delegates
(`SCStreamDelegate` + `SCStreamOutput`, reusing the camera delegate pattern). `open()` bridges two
real async completion-handler calls (`getShareableContentWithCompletionHandler`,
`startCaptureWithCompletionHandler`) via a bounded channel — the first genuinely-async `open()` in
this crate. Permission is materially simpler than Android's `MediaProjection`:
`CGPreflightScreenCaptureAccess`/`CGRequestScreenCaptureAccess` are two plain C functions (a
one-time system TCC grant). `SCStreamOutputType::Microphone` (mic folded into the same stream)
exists but is confirmed unused — `AppleMicrophoneCapture` stays the sole mic path.

**iOS**: **two** entry points. `AppleScreenCapture` (`RPScreenRecorder.startCaptureWithHandler`,
in-app only, no extension needed) captures video + app audio + mic audio by default
(`AudioApp`→`DesktopAudioCapture`, `AudioMic`→`crate::audio::AudioCapture`). A real, subtle
asymmetry found while implementing: `startCaptureWithHandler`'s completion handler is
`block2::SendableBlock`, but `stopCaptureWithHandler`'s is a plain, non-`Sendable` `block2::Block`
— confirmed by direct signature comparison, not assumed symmetric. `AppleBroadcastExtensionCapture`
is the second entry point: a push-in/pull-out sink (`push_sample_buffer`, no owned OS session) for
a host project's **Broadcast Upload Extension** (`.appex` target, `RPBroadcastSampleHandler`
subclass) — this crate cannot build that target itself; the host-extension contract (set
`RPBroadcastProcessMode = RPBroadcastProcessModeSampleBuffer`, forward each
`processSampleBuffer:withType:` call in one line) is documented in ADR-0004. The real C-callable
boundary Swift would call belongs in `mediaway-ffi`'s `device` module (not `extern "C"` in this
crate — a correction against this workspace's own C-FFI rule), named as future work, not built.
Audio extraction (`extract_pcm`, `CMBlockBuffer` + `CMAudioFormatDescription`/
`AudioStreamBasicDescription`) is shared by both iOS entry points via one dispatch helper,
`classify_and_queue_sample_buffer`.

## Window (`AppleWindowCapture`, macOS only)

Added 2026-08-19, resolving ADR-0003 § Open questions #5. Shares `screencapturekit.rs`'s
`SCStream` session recipe with `AppleScreenCapture` via one internal `Session`/`open_stream` (the
same shared-session shape `mediaway-device::linux`'s `LinuxScreenCapture`/`LinuxWindowCapture` use)
— only `SCContentFilter` construction differs:
`SCContentFilter::initWithDesktopIndependentWindow` instead of
`initWithDisplay_excludingWindows`. The `DesktopCaptureSource::Window` handle's bits are read as a
`CGWindowID` and matched against `SCShareableContent::windows()` — a real, programmatic
window-target capability (unlike the Linux portal backend, whose picker UI ignores the handle and
lets the user choose interactively). No iOS equivalent: `ReplayKit` has no other-window capture
concept, so `AppleWindowCapture` is `#[cfg(target_os = "macos")]`-only (plus the usual off-Apple
`host_stub`).

## Cross-cutting

Minimum OS floor is not verifiable from local `objc2` source (no `@available` annotations in the
generated bindings, unlike Android's `api-level-26` Cargo-feature proof) — `ScreenCaptureKit`'s
public macOS 12.3+ floor is the strictest domain if confirmed. `apple-macos`/`apple-ios` CI jobs
(`.github/workflows/ci.yml`) now also lint `mediaway-device` — no new jobs added.
`AppleBroadcastExtensionCapture`'s verification ceiling is lower than every other backend in this
workspace: no CI configuration here can ever exercise a real `.appex`/`RPBroadcastSampleHandler`
integration, since building one requires a host Xcode project this crate doesn't own.

## Related

- [`platform/android-encode`](../platform/android-encode.md) · [`android-capture`](android-capture.md)
  — binding-choice/CI-honesty precedent this design mirrors
- `mediaway-encoder::apple` (`crates/mediaway-encoder/adr/apple/0001-...`) — dependency-review
  baseline, existing CI jobs this design reuses
- [scaffold](scaffold.md) · [capabilities](capabilities.md)
