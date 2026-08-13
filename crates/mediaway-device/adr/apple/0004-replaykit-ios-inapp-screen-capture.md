# ADR-0004: `ReplayKit` screen capture (iOS) — `RPScreenRecorder` in-app capture + Broadcast Upload Extension host contract

- **Status**: Accepted
- **Date**: 2026-08-12
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device` (module `mediaway-device::apple`, iOS-only surface within it — see
  ADR-0003 § Why a separate ADR for the macOS/iOS split rationale, which applies symmetrically
  here)

## Context

iOS has no `ScreenCaptureKit` (macOS-only, ADR-0003). The iOS screen-capture framework is
**`ReplayKit`**, which has a real, important architectural split the task explicitly asked to be
confirmed rather than assumed: `RPScreenRecorder.shared().startCapture(handler:completionHandler:)`
(in-app-only — records the calling app's own content) vs. a **Broadcast Upload Extension** (a
genuinely separate Xcode extension target, for system-wide/other-app screen recording). That split
is confirmed real (§ Research, unchanged from this ADR's first pass).

**Scope, revised after user confirmation**: the first version of this ADR stopped at "Broadcast
Upload Extension is out of scope, defer to a follow-up ADR." **The user confirmed they want it
designed in this same pass** — not implemented (this crate genuinely cannot build or ship a
`.appex` target itself, that finding still stands), but the **host-extension contract** and a
concrete Rust-side entry point for it are now first-class parts of this ADR, mirroring exactly how
`adr/android/0003-mediaprojection-jni-screen-capture.md` documented Android's host-app-`Activity`
contract as a real ADR section rather than a deferred stub. **Audio inclusion is also confirmed
in scope** (app audio + microphone), for both the in-app and extension paths — see § Audio
inclusion.

## Research: two real, structurally different `ReplayKit` surfaces

Confirmed via `local/vendor-ref/objc2/generated/ReplayKit/{RPScreenRecorder,RPBroadcastExtension,
RPBroadcast,RPBroadcastConfiguration}.rs` and `objc2-replay-kit`'s own `Cargo.toml`.

### Surface A — `RPScreenRecorder` (in-app capture)

`RPScreenRecorder::sharedRecorder()` — a singleton, confirmed real
(`generated/ReplayKit/RPScreenRecorder.rs`). Real method confirmed:

```text
#[unsafe(method(startCaptureWithHandler:completionHandler:))]
pub unsafe fn startCaptureWithHandler_completionHandler(
    &self,
    capture_handler: Option<&block2::Block<'static, fn(NonNull<CMSampleBuffer>, RPSampleBufferType, *mut NSError)>>,
    completion_handler: Option<&block2::SendableBlock<'static, fn(*mut NSError)>>,
);
```

Doc comment (read in full): *"Starts screen and audio capture and continually calls the supplied
handler with the current sampleBuffer and bufferType and passed it back to the application. Note
that before recording actually starts, the user may be prompted with UI to confirm recording."*
No mention of any extension-target requirement in this method's own documentation. `RPSampleBufferType`
(confirmed real, `RPBroadcastExtension.rs`) has three cases: `Video`, `AudioApp`, `AudioMic` — a
**single stream API surface delivering screen video, app audio, and (optionally, via
`isMicrophoneEnabled`) microphone audio all through the same continual handler**, tagged by type
per invocation.

**Important, easy-to-misread detail found this pass**: `startCaptureWithHandler_completionHandler`
is gated `#[cfg(all(feature = "RPBroadcastExtension", feature = "block2", feature =
"objc2-core-media"))]` in the generated source — i.e. it lives in the same `header-translator`
output *file* grouping as the genuinely-extension-only APIs (§ Surface B), and is gated behind a
Cargo feature literally named `"RPBroadcastExtension"`. **This is a naming/grouping artifact of
the header-translator's source-file organization, not evidence that this method itself requires a
Broadcast Upload Extension target** — confirmed by reading the method's own doc comment (which
describes ordinary in-app recording with a possible system consent prompt, the same pattern
`startRecordingWithHandler`/`stopRecordingWithHandler` on the same class use, both of which sit
under no such feature gate). Flagged explicitly because this is exactly the kind of "grouping name
looks scarier than the real requirement" trap this workspace's clone-and-read discipline exists to
catch — worth the user's attention even though it resolves in the "simpler" direction.

### Surface B — Broadcast Upload Extension: two real classes, one relevant to this crate

`generated/ReplayKit/RPBroadcastExtension.rs` declares **two** classes, a real distinction this
ADR's first pass under-documented:

```text
extern_class!(pub struct RPBroadcastHandler);         // base class, "handles video and audio data"
extern_conformance!(unsafe impl NSExtensionRequestHandling for RPBroadcastHandler {});

extern_class!(
    /// Subclass this class to handle CMSampleBuffer objects as they are captured by ReplayKit.
    /// To enable this mode of handling, set the RPBroadcastProcessMode in the extension's
    /// info.plist to RPBroadcastProcessModeSampleBuffer.
    #[unsafe(super(RPBroadcastHandler, NSObject))]
    pub struct RPBroadcastSampleHandler;
);
```

**`RPBroadcastSampleHandler`** (a subclass of `RPBroadcastHandler`, confirmed real superclass
chain) is the class the host extension's own Swift/Objective-C code must subclass for this crate's
purposes — confirmed real methods on it:

```text
pub unsafe fn broadcastStartedWithSetupInfo(&self, setup_info: Option<&NSDictionary<NSString, NSObject>>);
pub unsafe fn broadcastPaused(&self);
pub unsafe fn broadcastResumed(&self);
pub unsafe fn broadcastFinished(&self);
pub unsafe fn broadcastAnnotatedWithApplicationInfo(&self, application_info: &NSDictionary);
pub unsafe fn processSampleBuffer_withType(&self, sample_buffer: &CMSampleBuffer, sample_buffer_type: RPSampleBufferType);
pub unsafe fn finishBroadcastWithError(&self, error: &NSError);
```

`processSampleBuffer:withType:`'s doc comment (read in full): *"Method is called as video and
audio data become available during a broadcast session and is delivered as CMSampleBuffer
objects."* — **confirmed**, this is the per-frame delivery point the task asked about, and it is
tagged with the same `RPSampleBufferType` (`Video`/`AudioApp`/`AudioMic`) Surface A uses — the two
surfaces share one data model, not two.

**Real, previously-unflagged gap found this pass**: the doc comment's phrase "To enable **this
mode** of handling, set `RPBroadcastProcessMode` ... to `RPBroadcastProcessModeSampleBuffer`"
implies at least one **other** `RPBroadcastProcessMode` value exists (a direct-upload mode that
hands the extension a destination and expects it to stream data itself, with no local
sample-buffer callback at all — the literal "upload" half of "Broadcast **Upload** Extension").
**No corresponding class or the mode's other value was found in this local clone** — only
`RPBroadcastHandler`/`RPBroadcastSampleHandler` are declared. This crate's host-extension contract
(§ below) targets `RPBroadcastProcessModeSampleBuffer` specifically, since that is the only mode
with a confirmed, grounded API surface this crate can plug into; the other mode is out of scope
and its exact name/shape is flagged as unconfirmed, not guessed.

`NSExtensionRequestHandling` conformance (on both classes) remains the real, load-bearing signal
this ADR's first pass already established: `RPBroadcastSampleHandler` must be subclassed **inside
a genuine `.appex` app-extension target** with its own `Info.plist`
(`NSExtensionPointIdentifier: com.apple.broadcast-services-upload`), its own build product, and
its own process — this crate cannot create, build, or link that target itself. That finding is
unchanged; what changes is that this ADR now documents, precisely, what that target's code must
do to hand frames to this crate (§ Host-extension contract) instead of stopping at "out of scope."

## Research: audio delivery needs `CMBlockBuffer` after all

ADR-0002 (mic) deliberately chose `AVAudioEngine` over `AVCaptureSession`+`AVCaptureAudioDataOutput`
specifically to avoid extracting PCM out of a `CMSampleBuffer`'s `CMBlockBuffer` (the encoder
ADR-0001's own extra, non-default `objc2-core-media` feature). **`ReplayKit` offers no such
choice** — both `RPScreenRecorder`'s handler and `RPBroadcastSampleHandler::processSampleBuffer`
deliver `AudioApp`/`AudioMic` samples as `CMSampleBuffer`s, the same type video frames arrive as,
distinguished only by the `RPSampleBufferType` tag. Extracting interleaved PCM therefore needs
`CMSampleBufferGetDataBuffer` → `CMBlockBuffer::copy_data_bytes`/`data_pointer`/`data_length` (the
same API the encoder ADR already reviewed and added as an extra feature) plus reading the real
sample rate/channel count/format off the sample buffer's `CMAudioFormatDescription` — **the exact
audio-format-description accessor names were not read this pass** (§ Open questions), unlike the
H.264 parameter-set accessors the encoder ADR confirmed for its own, different `CMFormatDescription`
use.

## Decision

> Design **both** entry points this pass, not just Surface A:
>
> 1. **`AppleScreenCapture`** (`mediaway-device::apple::replaykit`, `#[cfg(target_os = "ios")]`) —
>    in-app capture via `RPScreenRecorder`, unchanged public type name from this ADR's first pass
>    (mirrors ADR-0003's macOS `AppleScreenCapture`, matching how `mediaway-encoder::apple`'s
>    `AppleVideoEncoder` presents one name across OSes even though the internal backend differs).
> 2. **`AppleBroadcastExtensionCapture`** (same module) — a **new**, second capture entry point
>    for use **from inside** a host-project-supplied `.appex` extension process, described fully
>    in § `AppleBroadcastExtensionCapture` design. This crate still cannot build the extension
>    target itself — this type is the Rust-side half of a contract the host project's own
>    extremely thin Swift/Objective-C subclass code fulfills (§ Host-extension contract).

### `AppleScreenCapture` (in-app, `RPScreenRecorder`) — unchanged plumbing, revised audio scope

- `open()`: `RPScreenRecorder::sharedRecorder()` → `setMicrophoneEnabled(true)` (**changed from
  this ADR's first pass**, which set `false`; see § Audio inclusion) →
  `startCaptureWithHandler_completionHandler(capture_handler, completion_handler)` — both
  parameters async, `block2`-based, bridged to a synchronous `open()` via a bounded channel on a
  worker thread, same design ADR-0003 already established for `SCStream`'s completion handlers.
- The `capture_handler` block fires continually with `(CMSampleBuffer, RPSampleBufferType,
  NSError*)` for **all three** `RPSampleBufferType` values now (not filtered to `Video` only) —
  dispatched through one shared helper, `classify_and_queue_sample_buffer` (§ Audio inclusion),
  reused verbatim by `AppleBroadcastExtensionCapture` below.
- **No `define_class!` needed for frame delivery** — unchanged from the first pass;
  `startCaptureWithHandler` takes a plain `block2::Block` closure. `RPScreenRecorderDelegate`
  remains optional, still deferred (§ Open questions).
- `close()`: `stopCaptureWithHandler(handler)` — unchanged.

## Host-extension contract (Broadcast Upload Extension, `.appex` target)

Mirrors `adr/android/0003`'s "The exact host-app contract" section directly — the irreducible
minimum of **real Swift/Objective-C code this crate cannot write**, that the host project's own
Xcode project must supply:

1. Add a new **Broadcast Upload Extension** target to the host Xcode project (Apple's own project
   template; not something `cargo`/this crate can generate).
2. In that target's `Info.plist`, set `RPBroadcastProcessMode` to `RPBroadcastProcessModeSampleBuffer`
   (§ Research — the only mode this crate's contract targets).
3. Subclass `RPBroadcastSampleHandler` (real Swift/Obj-C code, e.g. `class
   MediawaySampleHandler: RPBroadcastSampleHandler`).
4. In `processSampleBuffer(_:with:)`, call into this crate's exposed entry point (§ FFI boundary
   below), passing the raw `CMSampleBufferRef` and the `RPSampleBufferType` value through
   unmodified — **this is the entire per-frame contract**, a single forwarding call, not
   Objective-C/Swift-side frame processing logic.
5. In `broadcastStartedWithSetupInfo`, construct/open the extension-side capture sink (§
   `AppleBroadcastExtensionCapture` design) before the first `processSampleBuffer` call arrives.
6. In `broadcastFinished` (and `finishBroadcastWithError` for the abnormal-stop path), close the
   sink and release its resources.
7. `broadcastPaused`/`broadcastResumed`/`broadcastAnnotatedWithApplicationInfo` have **no required
   Rust-side call** this slice — the host extension may no-op them, or forward them if the host
   application wants pause/resume signaled into its own capture-consuming code (not part of this
   crate's contract; `DesktopVideoCapture`/`DesktopAudioCapture`/`AudioCapture` have no
   pause/resume concept today).

### FFI boundary — a real architecture correction from the coordinator's suggested shape

The coordinator's brief suggested a `#[no_mangle] extern "C"` function directly in this crate.
**That would violate this workspace's own C-FFI rule** (`docs/spec/c-ffi.md` / `AGENTS.md` §
Architecture & API shape: *"C-FFI only in `*-ffi` crates... Do not add `extern "C"` to sans-io/
platform cores"*) — `mediaway-device` is a platform-backend core, not an FFI crate. Corrected
design, still answering the same real question ("what does the extension's Swift glue call"):

- **This ADR** (`mediaway-device::apple::replaykit`) defines only the **safe(ish) Rust API** —
  `AppleBroadcastExtensionCapture::push_sample_buffer(&self, sample_buffer: &CMSampleBuffer, kind:
  RPSampleBufferType) -> Result<(), CaptureError>` — callable from Rust code linked into the same
  extension process.
- **The actual C-callable entry point Swift invokes** belongs in `mediaway-ffi`'s existing
  `device` module (`docs/spec/c-ffi.md` — ADR-0021 merged all `*-ffi` crates into this one,
  feature-gated by capability; there is no separate `mediaway-device-ffi` crate to add this to).
  Building that entry point is **explicitly out of scope for this ADR** — named here as the
  correct, real future location (e.g. a new `mediaway_ffi_apple_broadcast_push_sample_buffer(handle,
  sample_buffer: *const c_void, kind: i64) -> i32` under a new or existing `mediaway-ffi` feature),
  not designed in detail this pass. Swift would reach it via a bridging header
  (`#include <mediaway/device.h>` or a new header this future work adds) — the same "C ABI, not
  Rust FFI directly" boundary every other non-Rust host in this workspace already crosses.

This split keeps `mediaway-device` free of `extern "C"` while still giving a concrete, correct
answer to "what does the extension's one-line forwarding call look like" — the extension's Swift
override becomes a two-line function: reinterpret its `CMSampleBuffer` parameter as the C ABI's
opaque pointer type, call the future `mediaway-ffi` entry point, done.

## `AppleBroadcastExtensionCapture` design

New type, same module (`mediaway-device::apple::replaykit`), `#[cfg(target_os = "ios")]`:

- **Not opened the way every other backend's capture type is** — there is no OS session for this
  type to start; `RPBroadcastSampleHandler`'s host-extension lifecycle already owns that (driven
  by the system's broadcast picker UI, outside this crate entirely). `AppleBroadcastExtensionCapture::new()`
  only allocates the shared queues (video / app-audio / mic-audio) and `StreamInfo` placeholders —
  no `block2` callback registration, no `AVCaptureSession`-shaped `open()`.
- **`push_sample_buffer(&self, sample_buffer: &CMSampleBuffer, kind: RPSampleBufferType) ->
  Result<(), CaptureError>`** — the sink's one real entry point, called once per
  `processSampleBuffer:withType:` invocation (via the future `mediaway-ffi` shim, § above).
  Internally calls the **same** `classify_and_queue_sample_buffer` helper `AppleScreenCapture`'s
  in-app `block2` closure calls — one shared dispatch routine for both entry points, not two
  independent implementations, reusing ADR-0001/0003's `CVPixelBuffer` extraction for `Video` and
  this ADR's new `CMBlockBuffer` extraction for `AudioApp`/`AudioMic`.
- Implements `DesktopVideoCapture` + `DesktopAudioCapture` + `crate::audio::AudioCapture` — same
  trait set as `AppleScreenCapture` (§ Audio inclusion) — the host extension's own code polls
  frames back out via the ordinary `poll_frame` shape on whichever trait it needs, after handing
  sample buffers in via `push_sample_buffer`. **This is a push-in / pull-out shape unique to this
  one backend** — every other capture type in this crate is pull-only from its own internal OS
  session; this type has no OS session of its own to pull from, `push_sample_buffer` is the only
  way data enters it.
- `close()`: releases the queues; does **not** call anything ReplayKit-related (the host
  extension's own `broadcastFinished`/`finishBroadcastWithError` handlers own that, per § Host-
  extension contract step 6).

### A real, honest verification-gap note this crate cannot close alone

**This crate cannot build a `.appex` extension target at all** — there is no Cargo-buildable
artifact type for an app extension; it is purely an Xcode project-structure concept. Even with a
real macOS/Xcode environment (which this dev environment lacks entirely), verifying
`AppleBroadcastExtensionCapture` end-to-end would require a **host Xcode project this crate does
not own**, a real Broadcast Upload Extension target, and a real broadcast session started from
Control Center — a materially higher verification bar than any other backend's CI plan in this
workspace, including Android's JNI-signature gap (`adr/android/0003` § CI verification plan),
which at least ships inside one crate's own compile step. `push_sample_buffer`'s **internal**
dispatch logic (the `classify_and_queue_sample_buffer` helper) is unit-testable with synthetic
inputs the way `camera.rs`'s `pack_frame_bytes`-style helpers are elsewhere in this crate — but the
real `CMSampleBuffer`/`RPBroadcastSampleHandler` integration is not testable by this crate's CI
under any configuration.

## Audio inclusion

**Confirmed with the user**: both app audio and microphone audio are in scope for
`AppleScreenCapture` (in-app), not video-only. `setMicrophoneEnabled(true)` is now this backend's
default (§ Decision above); `RPSampleBufferType::AudioApp`/`AudioMic` frames are extracted and
queued, not discarded.

### Trait-shape decision: three independent trait implementations on one struct

`crate::desktop::DesktopVideoCapture` only yields `VideoFrame` (`crates/mediaway-device/src/
desktop/video.rs`) — it has no slot for audio. Reviewed `crate::desktop::DesktopAudioCapture`
(`crates/mediaway-device/src/desktop/audio.rs`, Windows' existing loopback/process-loopback
model) and `crate::audio::AudioCapture` (`crates/mediaway-device/src/audio/capture.rs`, the
microphone-shaped trait). Decision made, not deferred:

- **`AudioApp`** → `crate::desktop::DesktopAudioCapture` — `AppleScreenCapture` (both in-app and
  extension variants) implements this trait too. `RPSampleBufferType::AudioApp` is documented by
  Apple as *"an audio capture sample buffer"* belonging to the app being recorded — for in-app
  `RPScreenRecorder` capture, "the app being recorded" **is** the calling app itself, so "what
  this app is playing" is a reasonable, honest fit for `DesktopAudioCapture`'s stated "what's
  playing" purpose (unlike Windows' system-wide render-endpoint `Loopback`, this is inherently
  scoped to one app — closer in spirit to `DesktopAudioSource::ProcessLoopback`'s per-process
  scoping than to system-wide `Loopback`, though no `process_id` field is meaningful here since
  the "process" is implicitly "self"). **`DesktopAudioCaptureConfig`/`DesktopAudioSource` are
  *not* reused as an *opening* config** for this path (same reasoning ADR-0003 in the Android set
  used to justify a dedicated config instead of forcing an ill-fitting existing type) — there is
  no independent `DesktopAudioCapture::open()` call; the audio-capture trait is simply implemented
  on the same struct `AppleScreenCapture`'s video-path `open()` already produces, populated for
  free once `RPSampleBufferType::AudioApp` frames start arriving.
- **`AudioMic`** → `crate::audio::AudioCapture` — a live, ordinary microphone signal, not a
  "what's playing" concept at all; forcing it through `DesktopAudioCapture` would misrepresent
  what it is (the coordinator's own flagged tension). `crate::audio::AudioCapture`'s trait shape
  (`stream_info`/`poll_frame`/`close`, no `open()` on the trait itself) is satisfied the same way
  — `AppleScreenCapture` additionally implements `AudioCapture`, populated from `RPSampleBufferType::
  AudioMic` frames. **This does not replace `AppleMicrophoneCapture` (ADR-0002)** — it is a second,
  independent `AudioCapture` implementation that happens to exist only while a `ReplayKit` session
  with `isMicrophoneEnabled(true)` is open, for callers who specifically want mic audio
  time-synchronized with the screen recording rather than as an independently-opened session. A
  caller who wants mic audio without screen capture still uses `AppleMicrophoneCapture`.
- Each trait's `poll_frame` pops from its own independent bounded queue (video / app-audio /
  mic-audio) — three queues fed by one dispatch helper, not one interleaved queue three consumers
  would need to filter — matching this crate's existing "each data kind gets its own typed queue"
  convention rather than inventing a new multiplexed-poll API.
- `close()` (identical across all three trait impls, since they're one struct) tears down the
  whole session once, regardless of which trait a caller invoked it through.

### Extension path carries audio too — the same dispatch, no extra cost

**Confirmed, the coordinator's own question answered**: `AppleBroadcastExtensionCapture` also
implements all three traits and reuses the same `classify_and_queue_sample_buffer` helper.
`processSampleBuffer:withType:` already receives `AudioApp`/`AudioMic` sample buffers whenever the
system broadcast actually includes them — routing them through the same dispatch helper the in-app
path uses costs nothing extra to design (one shared function, two call sites) and keeps the two
entry points behaviorally symmetric. **One real asymmetry, flagged rather than glossed over**:
`RPScreenRecorder.setMicrophoneEnabled` is a real API call the in-app path controls; **no
equivalent extension-side API call was found in this local clone**. Broadcast Upload Extension
sessions are started from the system's Control Center broadcast picker, which (per general Apple
platform knowledge, **not confirmed from local `objc2` source**) presents its own system-level
microphone toggle the user controls at broadcast-start time — the extension's
`RPBroadcastSampleHandler` simply receives (or does not receive) `AudioMic` sample buffers
depending on that system UI choice, with no code-level equivalent of `setMicrophoneEnabled` to
call. Flagged in § Open questions rather than asserted as fully confirmed.

## Permission model

`RPScreenRecorder.isAvailable` (confirmed real, cheap synchronous property) remains the closest
analog to `support(Screen)` — unchanged from the first pass. **`ReplayKit`'s own consent UI is
presented automatically by `startCaptureWithHandler` itself** when needed; no separate
"preflight without prompting" call confirmed. `request_permission(Screen)` on iOS may have no
cheaper option than actually starting capture (unchanged finding, still an open question).
`AppleBroadcastExtensionCapture` has **no permission model of its own** — the system Control
Center broadcast picker (entirely outside this crate, and outside the host extension's control
too) is the real consent surface for that path.

## ZCA / typestate shape

| Existing precedent | ReplayKit (`RPScreenRecorder`) | `AppleBroadcastExtensionCapture` |
|---|---|---|
| ADR-0003's `SCStream`: two `define_class!` delegates | Zero delegate classes — plain `block2::Block` closure | Zero delegate classes — plain method call (`push_sample_buffer`), no callback registration of any kind |
| Every other backend: `open()` starts a real OS session this struct owns | `open()` starts `RPScreenRecorder`'s session (async-bridged, § Decision) | **No OS session to open** — `new()` only allocates queues; data arrives via external push, not an internally-owned callback/poll source |
| One queue per data kind, fed by one poll-driven or callback-driven source | Same (three queues, fed by the in-app `block2` closure) | Same three queues, fed by `push_sample_buffer` instead of an internal callback |
| `GpuBufferHandle::Metal` — deferred (ADR-0001/0003) | Same, sourced from the delivered `CVPixelBuffer` | Same |

No `Box<dyn _>` planned for either type. `AppleScreenCapture`/`AppleBroadcastExtensionCapture`
each wrap concrete `Option<Inner>` state, matching every other backend.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `#[no_mangle] extern "C"` entry point directly in `mediaway-device::apple::replaykit` | Rejected — violates this workspace's own C-FFI rule (`docs/spec/c-ffi.md`, `AGENTS.md`): `extern "C"` belongs only in `mediaway-ffi`, never in a platform-backend core. Corrected design keeps the safe Rust API here and names `mediaway-ffi`'s `device` module as the real future FFI-boundary location instead. |
| Have the host extension's Swift code do frame extraction itself (CVPixelBuffer locking, PCM extraction), handing this crate only fully-parsed `VideoFrame`/`AudioFrame`-shaped data | Rejected — would duplicate ADR-0001/0003's `CVPixelBuffer` extraction and this ADR's `CMBlockBuffer` extraction in Swift, doubling the maintenance surface for logic this crate already owns correctly for the in-app path. Keeping frame extraction entirely on the Rust side (via a thin one-line Swift forwarding call) is strictly less host-project code, mirroring how Android's ADR-0003 kept `createVirtualDisplay`/frame-pump ownership on the Rust side and pushed only the irreducible consent-flow minimum onto the host app. |
| Defer the Broadcast Extension design to a follow-up ADR (this ADR's own original decision) | Superseded — the user explicitly wants it designed this pass. Superseded text kept visible in this ADR's revision history rather than silently deleted (see § Context). |
| `AVCaptureSession` + a fabricated "screen" `AVCaptureDevice` | Unchanged from the first pass — no such device type exists on iOS. Not a real option. |

## Dependency review (`docs/conventions/deps-policy.md`)

| Question | Answer |
|----------|--------|
| Need | Real — `ReplayKit` is the only iOS screen-capture framework; no alternative exists. |
| License | Same `Zlib OR Apache-2.0 OR MIT` workspace line, same `madsmtm/objc2` monorepo — not re-litigated. |
| New surface vs. this ADR's first pass | **`objc2-core-media`'s `"CMBlockBuffer"` feature is now needed** (§ Research: audio delivery), the same extra, non-default feature the encoder ADR-0001 already added for its own reasons — not a new crate, an additive feature flag on an already-depended-on crate. |
| Platform scope | Unchanged — `mediaway-device::apple::replaykit` (both types) stays `target_os = "ios"`-gated; macOS keeps `ScreenCaptureKit` (ADR-0003). |
| `mediaway-ffi` follow-up | Not a dependency this ADR adds — flagged as future work in a **different** crate (`mediaway-ffi`'s `device` module), out of scope to design in detail here. |

## ⚠️ CI verification plan

Unchanged for `AppleScreenCapture`: reuses `apple-ios` (cross-compiled, compile-only). **New for
`AppleBroadcastExtensionCapture`**: also compiles under the same `apple-ios` job (it is ordinary
`#[cfg(target_os = "ios")]` Rust code in the same crate, no extension-specific toolchain needed to
*compile* it) — but per § "A real, honest verification-gap note," no CI configuration this
workspace could add would exercise the real `.appex`/`RPBroadcastSampleHandler` integration, since
that requires a host Xcode project and a live broadcast session this crate's CI has no path to
create. **Zero compile verification as authored for either type**, same as every Apple ADR this
session.

## Open questions (for user confirmation)

1. **`RPScreenRecorder.isAvailable` vs. an actual permission probe** — unchanged from the first
   pass, still unresolved.
2. **`RPScreenRecorderDelegate` wiring** — unchanged, still deferred-or-not undecided.
3. **Minimum OS floor** — unchanged, still not verified from local source this pass.
4. **`CMAudioFormatDescription` accessor names** — not read this pass; needed to populate
   `AudioFrame::sample_rate`/`channels`/`format` correctly for `AudioApp`/`AudioMic` extraction.
5. **`RPBroadcastProcessMode`'s other value** — real gap, § Research: only
   `RPBroadcastProcessModeSampleBuffer` is grounded from local source; the direct-upload mode's
   exact name/API (if any Rust-relevant API exists for it at all) is unconfirmed.
6. **Extension process memory constraints** — general Apple platform knowledge (**not verified
   from local `objc2` source or this pass's grounding**): Broadcast Upload Extension processes
   have historically run under a materially tighter memory limit than a normal foreground app
   (the extension is terminated by the system if it exceeds its budget). If real, this bears
   directly on `AppleBroadcastExtensionCapture`'s queue sizing (bounded, drop-oldest queues already
   this crate's convention — but the *bound* itself may need to be much smaller than other
   backends' defaults). Flagged for the user to confirm/size before implementation, not assumed.
7. **`processSampleBuffer:withType:`'s calling thread** — not confirmed this pass; affects whether
   `push_sample_buffer`'s internal locking needs to assume a single caller thread or handle
   concurrent calls (video/app-audio/mic-audio arriving interleaved but possibly not
   serialized onto one thread).
8. **Extension-side mic toggle** — confirmed no `setMicrophoneEnabled`-equivalent API found this
   pass for the extension path (§ Audio inclusion); confirm this is genuinely controlled only by
   the system Control Center picker before Accepted status.
9. **Future `mediaway-ffi` entry point's exact signature/feature-gating** — intentionally not
   designed in this ADR (§ FFI boundary); worth a placeholder decision on which existing
   `mediaway-ffi` feature (`camera`? `desktop`?) this would fall under, or whether it needs its
   own new feature, when that future work starts.

## Decisions confirmed with the user (2026-08-12)

1. **Broadcast Upload Extension scope — this ADR's central revision**: designed this same pass
   (host-extension contract + `AppleBroadcastExtensionCapture` entry point), **not** deferred to a
   follow-up ADR as this ADR's first-pass § Decision originally proposed. Still not implemented,
   and still cannot be built/tested by this crate alone (§ "A real, honest verification-gap note")
   — the change is scope of *design*, not a claim of working code.
2. **Audio inclusion**: confirmed **in scope** for `AppleScreenCapture` — both app audio
   (`RPSampleBufferType::AudioApp`) and microphone audio (`RPSampleBufferType::AudioMic`),
   default `setMicrophoneEnabled(true)` (changed from this ADR's first-pass `false`). Mapped to
   `crate::desktop::DesktopAudioCapture` (`AudioApp`) and `crate::audio::AudioCapture` (`AudioMic`)
   respectively — a reasoned, documented call (§ Audio inclusion), not a silent gap.
3. **Extension path also carries audio**: `AppleBroadcastExtensionCapture` implements the same
   three traits as `AppleScreenCapture` and reuses the identical `classify_and_queue_sample_buffer`
   dispatch helper — confirmed not deferred within the extension path, since `processSampleBuffer:
   withType:` already receives `AudioApp`/`AudioMic` buffers whenever the system broadcast
   includes them, at no extra design cost.

## Implementation notes (2026-08-13, written alongside the code)

- **Open question #4 resolved**: `CMAudioFormatDescription::stream_basic_description(&self) ->
  *const AudioStreamBasicDescription` (confirmed real, `CMFormatDescription.rs`); the real
  `AudioStreamBasicDescription` field names — `mSampleRate: f64`, `mChannelsPerFrame: u32`,
  `mFormatFlags: AudioFormatFlags`, etc. — are confirmed in `CoreAudioBaseTypes.rs`, along with
  `kAudioFormatFlagIsFloat`/`kAudioFormatFlagIsNonInterleaved` for validating the PCM layout
  before extraction (never silently mis-reads a non-`F32`-interleaved format).
- **A real, subtle asymmetry found while implementing, not assumed**:
  `startCaptureWithHandler_completionHandler`'s completion handler is `block2::SendableBlock`
  (`Send + Sync`-bounded, matching ADR-0003's completion handlers), but
  `stopCaptureWithHandler`'s handler is a **plain, non-`Sendable` `block2::Block`** — confirmed
  by direct signature comparison, not inferred from the "these are both just completion
  handlers" pattern the rest of this ADR set otherwise follows. `replaykit.rs`'s
  `stop_in_app_capture` uses the correct, non-`Send`-bounded `RcBlock` type accordingly.
- `push_sample_buffer`/`AppleBroadcastExtensionCapture::new()` and the in-app `AppleScreenCapture`
  both route through one shared `classify_and_queue_sample_buffer`, confirmed reusing
  `apple::pixel::extract_nv12` for `Video` and a new `extract_pcm` (this file) for
  `AudioApp`/`AudioMic` — no duplicated dispatch logic between the two entry points, as designed.
- `stream_info()` per data kind is populated from the **first** frame of that kind
  (`OnceLock<StreamInfo>`, immutable afterward) rather than tracked as mutable state — a real
  implementation simplification not specified in this ADR's original design, chosen because
  `ReplayKit` never renegotiates video geometry or audio format mid-session in practice, and it
  avoids an awkward `Mutex<StreamInfo>` vs. `&StreamInfo`-return-type lifetime conflict.

## Consequences

### Positive

- Confirmed, via real `objc2` source, both the in-app-vs-extension split **and** the exact
  extension-subclass override point (`RPBroadcastSampleHandler::processSampleBuffer:withType:`) —
  the host-extension contract is now written down completely (§ Host-extension contract), not left
  implicit or discovered later, the same standard `adr/android/0003` set for `MediaProjection`.
- Found and corrected a real architecture mismatch in the coordinator's own suggested design
  (`extern "C"` directly in this crate) against this workspace's own C-FFI rule, before any code
  was written — the corrected split (safe Rust core API here, C ABI shim named for `mediaway-ffi`)
  is honest about what this ADR does and does not design.
- One shared dispatch helper (`classify_and_queue_sample_buffer`) serves both entry points and all
  three sample-buffer types — no duplicated frame-extraction logic between `AppleScreenCapture` and
  `AppleBroadcastExtensionCapture`.
- Audio inclusion reuses two existing traits (`DesktopAudioCapture`, `crate::audio::AudioCapture`)
  with a reasoned, documented mapping — no new trait needed.

### Negative / Trade-offs

- **Zero compile verification as authored**, same class as every Apple ADR this session — and for
  the extension path specifically, **no CI configuration in this workspace could ever exercise the
  real integration**, a materially harder verification ceiling than any other backend, including
  Android's JNI-signature gap (which at least compiles inside one crate).
- Real, new `CMBlockBuffer` dependency this ADR's first pass did not need (audio inclusion now
  requires it).
- `AppleBroadcastExtensionCapture` is a genuinely new shape for this crate (push-in/pull-out, no
  owned OS session) — a real departure from every other backend's "owns its whole session"
  property, deliberate and documented, not accidental.
- Two real, unconfirmed platform facts (extension memory limits, `processSampleBuffer`'s calling
  thread) directly affect implementation-critical decisions (queue sizing, locking) and are not
  resolved by local `objc2` source grounding — flagged, not guessed.
- `AudioMic`-via-`AppleScreenCapture` and `AppleMicrophoneCapture` (ADR-0002) are now two
  independent, real ways to get microphone audio on iOS depending on whether a screen recording is
  also active — a real, documented API-surface duplication, not a silent redundancy.

## References

- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
- [`docs/spec/c-ffi.md`](../../../../docs/spec/c-ffi.md) — the C-FFI placement rule this ADR's
  revision corrected against (`mediaway-ffi`'s `device` module, not `extern "C"` in this crate)
- `adr/android/0003-mediaprojection-jni-screen-capture.md` — the host-app-contract-as-first-class-
  ADR-section precedent this revision now follows fully, not just for the honest-scope-boundary
  framing its first pass borrowed
- ADR-0001 (this set) — shared `CVPixelBuffer` frame-extraction routine, shared minimum-OS-floor
  open question
- ADR-0002 (this set) — `AppleMicrophoneCapture`, the independent mic path this ADR's `AudioMic`
  handling does not replace
- ADR-0003 (this set) — the macOS sibling this ADR is deliberately kept separate from, whose
  async-completion-handler bridge design and dedicated-config-type reasoning this ADR reuses
- `crates/mediaway-device/src/desktop/audio.rs` · `crates/mediaway-device/src/audio/capture.rs` —
  the two existing traits this ADR's § Audio inclusion maps `AudioApp`/`AudioMic` onto
- `linux-capture.md` — "the API *is* the consent mechanism" precedent this ADR's § Permission
  model draws a structural parallel to
- Local grounding source (read directly, not web-fetched):
  `local/vendor-ref/objc2/generated/ReplayKit/{RPScreenRecorder,RPBroadcastExtension}.rs`,
  `local/vendor-ref/objc2/framework-crates/objc2-replay-kit/Cargo.toml`
- [`objc2` on GitHub](https://github.com/madsmtm/objc2) (`Zlib OR Apache-2.0 OR MIT`)
