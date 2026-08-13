# ADR-0001: `AVFoundation` `AVCaptureSession` via `objc2`, camera capture (macOS + iOS)

- **Status**: Accepted
- **Date**: 2026-08-12
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device` (new module `mediaway-device::apple`, ADR-0021 `#[cfg]`-gated
  backend — no separate `mediaway-device-apple` crate)

## Context

This is the **first Apple backend in `mediaway-device`**, direct sibling of the just-landed
`mediaway-device::android` (camera + mic + screen, same session, `adr/android/0001`–`0003`) and
of `mediaway-encoder::apple` (`adr/apple/0001-videotoolbox-h264-cpu-upload.md`, same session).
Per `docs/spec/crate-packaging.md`'s platform suffix table, `apple` is a single reserved module
name covering **both** macOS and iOS — this ADR follows the encoder's precedent of a single
`apple` module unless a real split is warranted (it is not, for camera — see § Single-module
justification).

**Same hard environment constraint as the encoder's Apple ADR**: this dev environment (Windows
host) cannot compile-check Apple code at all — no legal cross-compile path outside macOS/Xcode.
Every claim below is a direct read of the locally cloned
[`objc2`](https://github.com/madsmtm/objc2) monorepo (`local/vendor-ref/objc2/`,
`generated/AVFoundation/`, `generated/CoreVideo/`, `generated/CoreMedia/`, plus the macro
implementation and tests under `crates/objc2/`), not web-fetched summaries or memory. Where local
source does not settle a question, this ADR says so explicitly (§ Open questions).

**This is a materially different kind of Apple API than the encoder's VideoToolbox** — read in
full before assuming continuity: VideoToolbox/CoreMedia/CoreVideo are plain C APIs (encoder
ADR-0001 §"A finding that simplifies the dependency graph"), needing **none** of
`objc2`/`objc2-foundation`/`block2`. `AVFoundation` is the opposite: `AVCaptureSession`,
`AVCaptureDevice`, `AVCaptureDeviceInput`, `AVCaptureVideoDataOutput` are all real Cocoa/
Objective-C classes (`extern_class!` in the generated source, not `#[repr(C)]` CF opaque types),
and the frame-delivery mechanism is a **delegate protocol**
(`AVCaptureVideoDataOutputSampleBufferDelegate`), not a C function pointer. This flips the
encoder ADR's "smaller dependency graph than a naive plan" finding on its head for this domain —
documented explicitly below, not glossed over.

## Research: `objc2-av-foundation` is the only realistic binding

Confirmed via `local/vendor-ref/objc2/framework-crates/objc2-av-foundation/Cargo.toml`: this
framework crate has a **required, non-optional** dependency on `objc2` (`features = ["std"]`,
not gated behind any feature flag) — every `AVFoundation` symbol needs the full Objective-C
runtime bridging layer, unlike VideoToolbox's plain C calls. `objc2-core-video` (`CVPixelBuffer`
read side) and `objc2-core-media` (`CMSampleBuffer`) are reused verbatim from the encoder's
already-reviewed dependency set (same license, same maintainer, same `"0.3"` version line —
confirmed `local/vendor-ref/objc2/Cargo.toml` `[workspace.package] version = "0.3.2"`, not
re-litigated here).

### The delegate pattern — a genuinely new pattern for this workspace

`AVCaptureVideoDataOutputSampleBufferDelegate` is declared via `extern_protocol!` in
`generated/AVFoundation/AVCaptureVideoDataOutput.rs`:

```text
pub unsafe trait AVCaptureVideoDataOutputSampleBufferDelegate: NSObjectProtocol {
    #[optional]
    unsafe fn captureOutput_didOutputSampleBuffer_fromConnection(&self, output: &AVCaptureOutput,
        sample_buffer: &CMSampleBuffer, connection: &AVCaptureConnection);
    #[optional]
    unsafe fn captureOutput_didDropSampleBuffer_fromConnection(&self, ...);
}
```

Unlike Android's C-callback-with-context-pointer (`ACameraDevice_StateCallbacks`, ADR
`android/0001`) or the encoder's `VTCompressionOutputCallback` C function pointer
(`encoder/adr/apple/0001`), this is a **full Objective-C protocol conformance** — Rust code must
define a real Objective-C class that implements this protocol, register it with the runtime, and
hand an instance to `setSampleBufferDelegate:queue:`.

`objc2`'s current mechanism for this is `define_class!` (confirmed: no `declare_class!` exists in
this checkout — `define_class!` is the sole, current macro; grepped
`crates/objc2/src/__macros/mod.rs`, no legacy alias found). Read directly, not assumed:

- `crates/objc2/src/__macros/define_class/mod.rs`'s own doc example shows a class with plain
  Rust-typed ivars (`foo: u8`, `bar: c_int`, `object: Retained<NSObject>`) — **ivars are not
  restricted to `Encode`-compatible types**; the whole `Ivars` struct is boxed once and accessed
  through generated getters. This means our delegate's ivars can directly hold
  `Arc<Mutex<VecDeque<VideoFrame>>>` (the exact shared-queue shape every other backend in this
  crate already uses — WASAPI, PipeWire, AAudio's blocking-read path) with **no** extra `void*`
  refcon bridge the way the encoder's C-callback design needed (`Arc::into_raw`/`Arc::from_raw`
  dance, encoder ADR-0001 § Callback design).
- `crates/test-assembly/crates/test_define_class_drop_ivars/lib.rs` is a real, compiling (under
  `#[cfg(target_vendor = "apple")]`) example of `define_class!` with ivars and an `init` override
  calling `set_ivars` — confirms the concrete syntax this ADR's design below uses.
- `crates/test-ui/ui/define_class_delegate_not_mainthreadonly.rs` confirms: a delegate class is
  only forced to `#[thread_kind = MainThreadOnly]` if the **protocol itself** requires
  `MainThreadOnly` (as `NSApplicationDelegate` does, in that test's fake protocol).
  `AVCaptureVideoDataOutputSampleBufferDelegate`'s real declaration bounds only on
  `NSObjectProtocol` — **no `MainThreadOnly` bound** — so this crate's delegate class can be
  `#[thread_kind = AnyThread]`, consistent with the callback firing on whatever `DispatchQueue` we
  hand to `setSampleBufferDelegate:queue:`, not necessarily the main thread. This matters for this
  crate's existing design property: every backend so far is usable **without a running UI event
  loop** (headless Windows DXGI, Linux portal via `block_on`, Android's worker-thread poll loops) —
  `AnyThread` preserves that.

### Session flow (confirmed real signatures)

1. `AVCaptureDevice::defaultDeviceWithMediaType(AVMediaTypeVideo)` (± `AVCaptureDevice::
   DiscoverySession` for `Select::Id`, ordinal index into its filtered `.devices()` array — same
   "ordinal index into an OS-filtered list" convention V4L2 (`adr/linux/0002`) and Camera2
   (`adr/android/0001`) both already use).
2. `AVCaptureDeviceInput::deviceInputWithDevice_error(device)` (confirmed real,
   `AVCaptureInput.rs`).
3. `AVCaptureSession::new()` → `canAddInput`/`addInput` (confirmed real, `AVCaptureSession.rs`).
4. `AVCaptureVideoDataOutput::new()` → `setVideoSettings` (pixel format dict, see § Pixel format)
   → `setSampleBufferDelegate_queue(&delegate, &our_own_dispatch_queue)` — confirmed this
   requires a real `dispatch2::DispatchQueue` (a **new dependency**, part of the same `objc2`
   monorepo, same license line). **Deliberately our own dedicated serial queue, not the session's
   default/main queue** — keeps this backend headless-usable, matching every other backend here.
5. `session.addOutput(&output)` → `session.startRunning()` (confirmed real, `AVCaptureSession.rs`
   `startRunning`/`stopRunning`). Apple's own doc comment (read in full) does not state
   `startRunning` is guaranteed fast — this crate's convention (every backend's `open()` runs on
   its own worker/driver thread, never the caller's calling thread for anything that can block on
   device arbitration) applies here too.
6. Frame delivery: `captureOutput_didOutputSampleBuffer_fromConnection` fires on our dispatch
   queue → extract pixels (§ below) → `push_back` into the shared `Arc<Mutex<VecDeque<VideoFrame>>>`
   ivar — `poll_frame` pops the front, identical shape to every other backend.

### An unresolved retention question, flagged not assumed

`setSampleBufferDelegate:queue:`'s doc comment (read in full) does **not** state whether the
`sampleBufferDelegate` property is `weak` or `strong` — unlike `RPScreenRecorder.delegate`
(`generated/ReplayKit/RPScreenRecorder.rs`), which is explicitly annotated `[weak property]` in
the same clone. Standard Cocoa delegate convention is weak (to avoid `session → output → delegate`
retain cycles), but this is **not confirmed from local source** for this specific property. Design
decision made defensively regardless of the answer: `AppleCameraCapture` itself holds the one
strong `Retained<Delegate>` for the whole session lifetime (mirrors how the encoder's output
callback context is kept alive by the encoder struct, not by VideoToolbox) — safe whether the
property turns out weak or strong.

### Frame extraction — the decode/capture-side mirror of the encoder's upload path

`CMSampleBufferGetImageBuffer` (`sample_buffer.image_buffer() -> Option<CFRetained<CVImageBuffer>>`,
confirmed real, `generated/CoreMedia/CMSampleBuffer.rs`). Confirmed real CF type hierarchy in
`generated/CoreVideo/CVPixelBuffer.rs`: `cf_type!(unsafe impl CVPixelBuffer: CVImageBuffer {})` —
`CVPixelBuffer` is declared a CF subtype of `CVImageBuffer`, the same "deref/type-hierarchy
coercion" property the encoder ADR already relied on for `CFRetained<VTCompressionSession>`. Video
capture always yields a `CVPixelBuffer`-backed image buffer per Apple's contract, but the **exact
downcast API name** (`objc2-core-foundation`'s concrete helper, vs. an explicit unsafe cast) is
not fully confirmed this pass — flagged in § Open questions rather than guessed.

Once downcast to `&CVPixelBuffer`: `lock_base_address(CVPixelBufferLockFlags::ReadOnly)` →
`plane_count()`/`width_of_plane`/`height_of_plane`/`bytes_per_row_of_plane`/
`base_address_of_plane` (all confirmed real, `CVPixelBuffer.rs` lines 882–1140) → copy each plane
into an owned `Bytes` (`VideoFrameStorage::Cpu`) → `unlock_base_address`. This is the **real
opposite direction** of the encoder's `upload_cpu_nv12` (encoder ADR-0001 §"CPU-upload input
strategy") — name it **`download_cpu_nv12`** (readback FROM a `CVPixelBuffer`, not upload TO one)
per `docs/spec/caveats-and-clarity.md`'s honest-naming rule. One real, named `memcpy` per frame —
mark **🆗**, same class every other CPU camera path in this crate carries (`windows_camera`,
`linux::camera`).

## Pixel format request

`AVCaptureVideoDataOutput::setVideoSettings` with `{kCVPixelBufferPixelFormatTypeKey:
kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange}` (or `...FullRange`) — both real constants,
confirmed in `objc2-core-video`'s `CVPixelBuffer.rs` (same file the encoder ADR already cited).
`availableVideoCVPixelFormatTypes()` (confirmed real, `AVCaptureVideoDataOutput.rs`) lists what
the device actually supports; this crate should verify the requested format is in that list before
setting it, same "never silently mis-read a layout we didn't verify" discipline V4L2/Camera2
already established, rather than assuming every camera supports bi-planar 4:2:0.

**Real, unresolved tension with the encoder's own choice**: `mediaway-encoder::apple` picked
`...FullRange` (0–255) for its **synthetic** encode input. Real camera hardware conventionally
outputs `...VideoRange` (16–235, broadcast/studio range) — using `FullRange` here without
verifying the device's native range risks silently mis-tagging genuinely video-range camera
pixels as full-range. This is a real product decision, not resolved by local grounding alone (see
§ Open questions).

## ZCA / typestate shape

| Existing precedent | AVFoundation camera | Difference |
|---|---|---|
| Camera2 NDK: raw pointer chain, each resource its own `_delete`/`_close` call, manual teardown ordering (`adr/android/0001`) | `Retained<AVCaptureSession>` / `Retained<AVCaptureDeviceInput>` / `Retained<AVCaptureVideoDataOutput>` / `Retained<Delegate>` / `dispatch2::DispatchQueue` — every resource is CF/Obj-C ref-counted, `Drop` handles release automatically | No manual `_delete` chain to get wrong — smaller teardown-ordering surface than Android's raw-FFI camera path |
| VideoToolbox: `Arc<Mutex<VecDeque<Packet>>>` bridged into a C callback via `Arc::into_raw`/`Arc::from_raw` refcon dance (`encoder/adr/apple/0001`) | Same shared-queue shape, held directly as a `define_class!` ivar (`Arc<Mutex<VecDeque<VideoFrame>>>` field on the delegate struct) — no raw-pointer refcon bridge needed | `objc2`'s ivar mechanism removes an entire unsafe layer VideoToolbox's plain-C callback needed |
| `GpuBufferHandle::AndroidSurface` (deferred, `adr/android/0001`) | `GpuBufferHandle::Metal` (deferred, `CVPixelBuffer`/`IOSurface` token) — **already exists** in `mediaway-common`, same as encoder ADR-0001 found | Same "type exists, wiring deferred" shape every platform's first pass has landed with |

No `Box<dyn _>` planned. `AppleCameraCapture` wraps a concrete `Option<Inner>` (closed-after-move
sentinel), matching every other backend. The delegate is one concrete `define_class!`-generated
Rust type (nameable directly, e.g. `SampleBufferDelegate`) — no `dyn` dispatch needed, since
`ProtocolObject<dyn AVCaptureVideoDataOutputSampleBufferDelegate>` is only a runtime-facing type
erasure `objc2` requires for the ObjC-side property, not a Rust-side vtable this crate's own code
pays for beyond one pointer.

## Authorization

`AVCaptureDevice::authorizationStatusForMediaType(AVMediaTypeVideo)` — confirmed real, **plain
synchronous class method**, no `block2` needed (`generated/AVFoundation/AVCaptureDevice.rs` line
~2318). Cheap enough for `capabilities::support(Camera)`'s live probe, mirroring
`CGPreflightScreenCaptureAccess`'s cheapness for macOS screen capture (ADR-0003 in this set).

`AVCaptureDevice::requestAccessForMediaType_completionHandler` — confirmed **requires the
`block2` feature** (`#[cfg(all(feature = "AVMediaFormat", feature = "block2"))]`), a **new
dependency this domain adds that VideoToolbox never needed**. The completion handler "is called
on an arbitrary dispatch queue" (Apple's own doc comment, read in full) — `capabilities::
request_permission(Camera)` needs a bounded channel/condvar bridge to turn this into the
synchronous return this crate's `request_permission` signature already commits to (same shape
class as Android's Camera2 "open a real session and observe the result," but here backed by a
real, dedicated authorization API rather than inferring denial from an open failure).

## Single-module (`apple`) justification

Every symbol cited above (`AVCaptureSession`, `AVCaptureDevice`, `AVCaptureDeviceInput`,
`AVCaptureVideoDataOutput`, `CVPixelBuffer`, `CMSampleBuffer`) is declared with no
`#[cfg(target_os = "macos")]`/`"ios"` item-level split in the generated files read this pass —
`AVFoundation` capture has been a unified cross-Apple-platform framework since it shipped on both
OSes, same conclusion the encoder ADR reached for VideoToolbox. The one real per-OS difference
(front/back lens-facing being a meaningful concept on iOS in a way it usually isn't on Mac
desktops/laptops) is a **behavioral/UX** difference, not an API-shape one — deferred to
§ Open questions, not a reason to split modules.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `AVCaptureSession` + `AVCapturePhotoOutput`/`AVCaptureStillImageOutput` | Still/photo capture only — wrong capability for a streaming `CameraCapture` trait. |
| `AVCaptureVideoPreviewLayer` + `CALayer` snapshot polling | A UI/rendering-layer hack for on-screen preview, not a real frame-data API; would also force a `CALayer`/window dependency this headless-usable crate does not want. |
| `objc2::runtime::ClassBuilder` (imperative class definition) | `define_class!`'s own docs explicitly recommend it over `ClassBuilder` for "a lot of extra debug assertions and niceties that help ensure soundness" — no reason to hand-roll the imperative form. |
| `CoreMediaIO` DAL plugin surface (`generated/CoreMediaIO/`) | For **exposing** a virtual camera device to other apps, not **consuming** an existing one — wrong direction for this trait. |

## Dependency review (`docs/conventions/deps-policy.md`)

| Question | Answer |
|----------|--------|
| Need | Real — `AVCaptureSession` is the only modern, still-supported camera capture API on Apple platforms. |
| License | `Zlib OR Apache-2.0 OR MIT` (same `local/vendor-ref/objc2/Cargo.toml` workspace license line the encoder ADR already reviewed) — allowed, already on `deny.toml`'s allow-list. |
| New surface vs. encoder's dependency set | `objc2` + `objc2-foundation` + `dispatch2` are **new** to this crate (not needed by `mediaway-encoder::apple`'s VideoToolbox-only path) — real, larger `unsafe`/ObjC-runtime surface than the encoder's C-API-only design. `block2` also new, for `requestAccessForMediaType`. |
| Maintenance | Same `madsmtm/objc2` project already reviewed in encoder ADR-0001 — not re-litigated. |
| Unsafe surface | Every `AVFoundation`/`objc2` call is `unsafe fn`; additionally, `define_class!` itself carries its own `# Safety` obligations (superclass invariants, `Drop` ordering rules) beyond a plain `unsafe fn` call — a genuinely larger unsafe-discipline surface than the encoder's VideoToolbox module. `#![allow(unsafe_code)]` required, same as `mediaway-encoder::apple`. |
| Alternatives | See § Alternatives Considered. |

## ⚠️ CI verification plan — reuse the encoder's existing Apple jobs

**Zero compile verification as authored** — same structural reason as the encoder's Apple ADR (no
legal cross-compile path outside Apple hardware/tooling). This ADR set does **not** propose new CI
jobs: `.github/workflows/ci.yml` already has `apple-macos` (native, `macos-14`, compile+clippy)
and `apple-ios` (cross-compiled, compile-only) from `mediaway-encoder`'s Apple work. Both should be
**extended** with a `mediaway-device` step (`cargo clippy -p mediaway-device --all-features
--all-targets -- -D warnings`, and for iOS the same with `--target aarch64-apple-ios`), and their
`if:` conditions' `contains(needs.affected.outputs.set, 'mediaway-encoder')` checks widened to
also match `mediaway-device` — not a new job, matching the task's explicit direction to reuse the
encoder's jobs. Not implemented in this pass (design/ADR only, per this task's scope).

## Open questions (for user confirmation)

1. **Minimum OS floor** — genuinely **not verifiable from local `objc2` source alone**, a real,
   structural difference from Android's `api-level-26`-in-`Cargo.toml` mechanical proof
   (`adr/android/0002`). `objc2`-generated bindings do not carry Apple's `@available`
   minimum-OS annotations as compile-time facts the way `ndk`'s Cargo features did. Camera
   capture via `AVCaptureVideoDataOutput` is known (from general Apple platform knowledge, **not
   confirmed this pass**) to be usable on very old macOS/iOS versions — much older than
   ScreenCaptureKit's macOS 12.3+ floor (ADR-0003 in this set). Worth noting: **`mediaway-encoder::
   apple`'s own ADR-0001 does not state an explicit minimum OS version either** — an omission in
   that ADR, not a differing number to match. Recommendation: pick a floor once, for the whole
   `apple` module across encoder+device (not per-domain), driven by the strictest domain actually
   shipped (today, ScreenCaptureKit's 12.3+ if macOS screen capture ships in the same module).
2. **`sampleBufferDelegate` weak/strong retention** — not settled by local source (see § An
   unresolved retention question). This ADR's defensive design (crate always holds its own strong
   `Retained<Delegate>`) is believed safe either way, but should be confirmed against Apple's
   official documentation before Accepted status.
3. **Exact `CVImageBuffer` → `CVPixelBuffer` downcast API** — the CF type-hierarchy relationship
   is confirmed real (`cf_type!` macro), but the precise `objc2-core-foundation` helper call this
   crate should use for the downcast was not pinned down this pass.
4. **Camera selection scope**: pure ordinal index (V4L2/Camera2 precedent) vs. an explicit
   lens-facing (`front`/`back`) preference field — iOS almost always exposes a meaningful
   front/back distinction, more consequential there than on most Mac hardware.
5. **Color range**: `VideoRange` (real camera hardware convention) vs. `FullRange` (this crate's
   `mediaway-encoder::apple` choice for *synthetic* encode input, ADR-0001 there) — a real product
   decision, not resolvable from local grounding; recommend `VideoRange` for camera specifically,
   diverging deliberately from the encoder's own choice, with the reason documented in code.
6. **`AVCaptureSession::startRunning()` call-site**: always on a dedicated worker thread
   (matching every other backend's `open()` shape), and whether `open()` should carry an internal
   timeout given Apple's doc comment does not bound how long device-arbitration blocking can take.

## Decisions confirmed with the user (2026-08-12)

1. **Color range**: confirmed **`VideoRange`** (`kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange`),
   not `FullRange` — the real camera-hardware convention flagged in § Open questions #5, chosen
   over reusing `mediaway-encoder::apple`'s `FullRange` choice for its own synthetic encode input
   (that choice stays as-is; the two are independently scoped, same pattern as the encoder's own
   `adr/apple/0001` § "Decisions confirmed" for its own color-range pick). No other change to this
   ADR's design.

## Implementation notes (2026-08-13, written alongside the code)

- **Open question #3 resolved**: the `CVImageBuffer` → `CVPixelBuffer` downcast is
  `CFRetained<CVImageBuffer>::downcast::<CVPixelBuffer>() -> Result<CFRetained<CVPixelBuffer>, _>`
  — `objc2-core-foundation`'s generic `ConcreteType`-bounded downcast, confirmed real via direct
  source read (`framework-crates/objc2-core-foundation/src/retained.rs`), not the `cf_type!`
  hierarchy relationship alone.
- **Open question #6 resolved**: `AVCaptureSession::startRunning()` is synchronous (confirmed
  real signature — no error out-param, no completion handler); `open()` calls it directly on the
  calling thread, no worker-thread bridge needed. See `camera.rs` module docs § "A real
  correction found while implementing this ADR".
- The pixel-format-request `NSDictionary` key (`kCVPixelBufferPixelFormatTypeKey`, a `CFString`
  constant) is bridged to `&NSString` via a documented toll-free-bridging raw pointer
  reinterpret — Apple's own documented `CFString`/`NSString` ABI equivalence, not a coincidental
  cast. See `camera.rs`'s inline `SAFETY` comment.
- Frame extraction (`extract_nv12`) was factored into a shared `apple::pixel` module once
  ADR-0003's design also needed the identical routine — one implementation, not three, per this
  ADR's own original intent.

## Consequences

### Positive

- Grounded entirely in real local `objc2` source, including the macro/test-suite level (not just
  generated framework bindings) — found the exact `define_class!` pattern this workspace needs for
  its first delegate-protocol callback, with a concrete, compiling reference example
  (`test_define_class_drop_ivars`).
- `objc2`'s ivar mechanism removes an entire unsafe-refcon layer the encoder's plain-C-callback
  design needed — a genuinely simpler shared-queue bridge than VideoToolbox's.
- `GpuBufferHandle::Metal` already existing means the deferred Zero-Copy path has no blocking
  type-design work left.
- Reuses the encoder's already-accepted `apple-macos`/`apple-ios` CI jobs — no new CI surface to
  design or maintain.

### Negative / Trade-offs

- **Larger dependency/unsafe surface than the encoder's VideoToolbox module** — `objc2` +
  `objc2-foundation` + `dispatch2` (+ `block2` for authorization) are all new to this crate,
  unlike the encoder's C-API-only design. This is the direct opposite of the encoder ADR's
  "smaller dependency graph than a naive plan" finding — an honest, stated reversal for this
  domain, not a hidden cost.
- **Zero compile verification as authored**, same structural class as every other Apple ADR this
  session.
- Real, unresolved retention-semantics question (delegate property weak/strong) — mitigated
  defensively, not confirmed.
- Minimum OS floor genuinely underdetermined by local grounding — a real gap this ADR cannot close
  alone (§ Open questions #1).

## References

- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
- [`docs/spec/crate-packaging.md`](../../../../docs/spec/crate-packaging.md) — `apple` platform
  suffix (single module for macOS+iOS)
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) — honest
  cost-naming (`download_cpu_nv12`) this ADR follows
- `mediaway-device` [ADR-0021](../../../../docs/adr/0021-workspace-consolidation.md)
- `adr/android/0001-camera2-ndk-native-camera-capture.md` — ordinal-index device selection,
  "never silently mis-read a layout" precedent, structure/honesty template this ADR mirrors
- `mediaway-encoder` `adr/apple/0001-videotoolbox-h264-cpu-upload.md` — dependency-review
  baseline, `upload_cpu_nv12`/`FullRange` precedent this ADR extends and partly diverges from,
  existing `apple-macos`/`apple-ios` CI jobs this ADR reuses
- Local grounding source (read directly, not web-fetched):
  `local/vendor-ref/objc2/generated/AVFoundation/{AVCaptureSession,AVCaptureDevice,AVCaptureInput,
  AVCaptureVideoDataOutput,AVCaptureOutput}.rs`,
  `local/vendor-ref/objc2/generated/CoreVideo/CVPixelBuffer.rs`,
  `local/vendor-ref/objc2/generated/CoreMedia/CMSampleBuffer.rs`,
  `local/vendor-ref/objc2/framework-crates/objc2-av-foundation/Cargo.toml`,
  `local/vendor-ref/objc2/crates/objc2/src/__macros/define_class/mod.rs`,
  `local/vendor-ref/objc2/crates/test-assembly/crates/test_define_class_drop_ivars/lib.rs`,
  `local/vendor-ref/objc2/crates/test-ui/ui/define_class_delegate_not_mainthreadonly.rs`
- [`objc2` on GitHub](https://github.com/madsmtm/objc2) (`Zlib OR Apache-2.0 OR MIT`)
