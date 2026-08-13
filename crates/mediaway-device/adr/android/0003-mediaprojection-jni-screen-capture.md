# ADR-0003: `MediaProjection` + JNI host-app handoff for Android screen capture

- **Status**: Accepted
- **Date**: 2026-08-12
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device` (module `mediaway-device::android`, see ADR-0001 § Open questions
  for the shared module-shape decision)

## Context

Screen capture on Android is fundamentally different from every other backend in this crate.
DXGI Desktop Duplication, `xdg-desktop-portal` `ScreenCast`, and Media Foundation camera capture
are all reachable from a native process with no managed-runtime dependency. **Android screen
capture is Java-only**: `android.media.projection.MediaProjection`,
`MediaProjectionManager.createScreenCaptureIntent()`, and the `Activity.startActivityForResult`/
`onActivityResult` consent flow have **no NDK equivalent at all** — there is no
`AMediaProjection_*` C API. This ADR spends the most research effort of the three in this set, as
directed, because it is the hardest and most novel.

This section documents, explicitly and honestly, that **this domain needs a real, load-bearing
host-app (Kotlin/Java) contract** — not a same-shape API to Windows DXGI or Linux portal capture.
That is an expected, not a disappointing, conclusion for this ADR to reach.

## Research: `android-activity` has no activity-result / consent surface

`mediaway-device::android`'s natural target audience — apps built on `android-activity`'s
`NativeActivity`/`GameActivity` glue, the same crate family `winit`/`wgpu` use, and the one the
encoder ADR already assumed for its "usable from a native/Rust entry point… without a Java
runtime attached" framing — was checked directly (`docs.rs`/GitHub source for `android-activity`,
current stable). Confirmed real methods on its `AndroidApp` handle:

- `vm_as_ptr() -> *mut c_void` — "a pointer to the Java Virtual Machine, for making JNI calls."
- `activity_as_ptr() -> *mut c_void` — an **unowned** JNI global reference to the app's `Activity`
  Java object, valid only until `AndroidApp` itself drops. Must not be wrapped as an owned
  `Global`/`Auto` reference (would try to delete a reference it doesn't own).
- `run_on_java_main_thread(Box<dyn FnOnce() + Send + 'static>)` — schedules a closure on the Java
  UI thread, for operations (like most JNI calls into Android SDK objects) that must run there.

**No method for launching an `Intent`, no `onActivityResult`/`ActivityResultLauncher` hook, no
permission-request helper exists anywhere in `AndroidApp`.** This is confirmed, not assumed: the
crate's full public surface (event loop, input, window, asset manager, IME, window flags, JNI
pointers) has no activity-result-shaped API at all. **Android's own platform design is the reason
— `onActivityResult`/`ActivityResultLauncher` callbacks are only ever delivered to a JVM-side
`Activity` subclass by the framework; there is no NDK entry point that could receive them even in
principle.** A stock `android-activity`-based app (the one the encoder ADR implicitly assumed)
**cannot** trigger or receive the MediaProjection consent flow on its own — the host app must
supply a **custom Activity subclass** with real Kotlin/Java code, something beyond what
`GameActivity`/`NativeActivity` provide out of the box. This is a materially larger host-app
burden than camera/mic (which need only a runtime permission grant — itself already a normal
Android app responsibility on every platform, see ADR-0002 § Permission).

## Research: the real Java-side `MediaProjection` flow

Cross-checked against Android's own developer documentation (not memory):

1. `Context.getSystemService(Context.MEDIA_PROJECTION_SERVICE)` → `MediaProjectionManager`.
2. `MediaProjectionManager.createScreenCaptureIntent()` → an `Intent` that, launched, shows the
   system's screen-share consent dialog.
3. `Activity.startActivityForResult(intent, requestCode)` (or the modern
   `registerForActivityResult(ActivityResultContracts.StartActivityForResult())`) — **must**
   originate from a live `Activity`; no native equivalent exists (see § Research above).
4. `onActivityResult(requestCode, resultCode, data)` (or the launcher callback) — only proceed if
   `resultCode == Activity.RESULT_OK`.
5. `MediaProjectionManager.getMediaProjection(resultCode, data)` → the `MediaProjection` object.

### Real, version-specific gotcha confirmed via Android's official behavior-changes docs

For apps **targeting Android 14 (API 34) or higher**, `MediaProjection#createVirtualDisplay`
throws `SecurityException` if either: the cached `Intent` from `createScreenCaptureIntent()` is
passed to `getMediaProjection()` **more than once**, or `createVirtualDisplay` is called **more
than once on the same `MediaProjection` instance**. **Both the consent `Intent` and the resulting
`MediaProjection` object are single-use — the user must be asked for consent again before every
capture session**, not just the first one per app-lifetime. This directly contradicts an implicit
assumption baked into this crate's existing `capture_desktop_video_once`-style convenience
pattern ("pays a full session-open cost every call" — framed as an OS-resource cost, not a
*visible consent UI* cost). On Android 14+ targets, **that composition helper degrades to
launching the system consent dialog on every single call** — this must be documented as an
honest, costly-path caveat (`docs/spec/caveats-and-clarity.md`), likely with a recommendation
against using it for Android screen capture at all, rather than silently inheriting a "just a
session-open cost" framing that stops being true here.

## Research: a clean, previously-undocumented native architecture (this session's key finding)

The naive design — marshal every captured pixel buffer across JNI per frame via a Java-side
`android.media.ImageReader` — was considered and explicitly **not** chosen. Reading the real NDK
source (`local/vendor-ref/ndk`) found a cleaner bridge that keeps the actual pixel path fully
native, with only a **one-time**, `open()`-only JNI round trip:

1. Native: `ndk::media::image_reader::ImageReader::new_with_usage(width, height,
   ImageFormat::RGBA_8888, HardwareBufferUsage::CPU_READ_OFTEN, max_images)` — confirmed real,
   requires the crate's `media` **and** `api-level-26` features (`#[cfg(feature =
   "api-level-26")]` on `new_with_usage` in the real source; the base `ImageReader`/`Image` type
   needs only `api-level-24`).
2. `.window() -> Result<NativeWindow>` (`AImageReader_getWindow`, confirmed present).
3. `unsafe { native_window.to_surface(env: *mut jni_sys::JNIEnv) -> jobject }` — wraps
   `ANativeWindow_toSurface`, **confirmed present in `ndk-sys`'s generated FFI for all four
   target arches** (`ffi_aarch64.rs`/`ffi_arm.rs`/`ffi_x86_64.rs`/`ffi_i686.rs`), gated by the
   crate's `nativewindow` **and** `api-level-26` features. Its own `# Safety` doc: "you assert
   that `env` is a valid pointer to a `JNIEnv`." This converts the native `ANativeWindow` into a
   real `android.view.Surface` Java object — the exact type `MediaProjection.createVirtualDisplay`
   expects as its target surface.
4. That raw `jobject` (wrapped as a JNI reference via the `jni` crate) is handed into a JNI method
   call on the already-obtained `MediaProjection` object:
   `createVirtualDisplay(name, width, height, densityDpi, flags, surface, callback, handler)`.
5. Frames are pumped **entirely natively** from then on:
   `image_reader.acquire_latest_image()` → `Image::plane_data(0)` (CPU path, into
   `VideoFrameStorage::Cpu`) or, deferred, `Image::hardware_buffer()` →
   `GpuBufferHandle::AndroidSurface { buffer: NativeHandle::new(ptr) }`.

This means **exactly one JNI round trip per `open()`** (handing the `Surface` into
`createVirtualDisplay`) — never a per-frame JNI cost. Worth stating explicitly and honestly per
this crate's Zero-Copy-marks culture: the CPU frame path here still pays a real `plane_data` copy
into an owned buffer (🆗, not ⚡, same mark every other CPU-path backend in this crate carries),
but that copy is a native `memcpy`, not a JNI byte-array marshal — a real, worth-documenting
distinction from the naive Java-`ImageReader` design this ADR rejects.

## Decision

> `mediaway-device::android::screencast::AndroidScreenCapture` (implementing
> `crate::desktop::DesktopVideoCapture`) **owns** `ImageReader` creation, the `NativeWindow` →
> `Surface` conversion, and the `createVirtualDisplay` JNI call — mirroring every other backend in
> this crate "owning its whole OS session." The **host app** owns only the irreducible minimum:
> obtaining a `MediaProjection` Java object via the consent flow that only a JVM `Activity` can
> receive, and handing it to this crate.

### The exact host-app contract (Kotlin/Java side — cannot be smaller than this)

1. `getSystemService(MEDIA_PROJECTION_SERVICE)` → `MediaProjectionManager`.
2. `createScreenCaptureIntent()`.
3. Launch it via `startActivityForResult`/`ActivityResultLauncher` from a real `Activity` — the
   host app's own custom subclass, since stock `android-activity` `GameActivity`/`NativeActivity`
   provides no hook for this (§ Research above).
4. In the result callback, only on `RESULT_OK`: `mediaProjectionManager.getMediaProjection
   (resultCode, data)` → `MediaProjection`.
5. Call a **native method the host app itself declares and JNI-exports** — e.g.
   `external fun nativeOnScreenCaptureConsent(nativeHandle: Long, mediaProjection: MediaProjection)`
   — converting `mediaProjection` into a JNI **global** reference before it crosses into native
   code. `mediaway-device` never sees the raw consent flow; it only ever receives an
   already-resolved `MediaProjection` object.

### The Rust-side entry point this hands into

`AndroidScreenCaptureConfig` (new, Android-only config type — does **not** reuse
`DesktopVideoCaptureConfig`/`DesktopCaptureSource::Screen`, since neither of those has a slot for
a foreign-object handle) carries:

- `media_projection: NativeHandle` — the raw `jobject` bits of the host app's global ref (same
  `mediaway_common::NativeHandle` type `DesktopCaptureSource::Window { window: NativeHandle }`
  already uses for an opaque platform handle — no new common type needed).
- `java_vm: NativeHandle` — a `JavaVM*`, obtainable either from `android_activity::AndroidApp::
  vm_as_ptr()` (if the host app is `android-activity`-based) or from `JNIEnv::get_java_vm()`
  inside the host app's own JNI entry point (if it manages its own JNI glue without
  `android-activity`). This crate attaches its own worker thread to the JVM via
  `jni::JavaVM::attach_current_thread` (or the version-appropriate equivalent — see § Open
  questions #3) rather than assuming the calling thread is already attached.
- `width` / `height` / `density_dpi` — `VirtualDisplay` creation parameters; no dynamic
  resize/rotation handling this slice (a real, current scope limit — Android rotation changes the
  effective capture geometry and is not handled).

`AndroidScreenCapture::open(config)` performs steps 1–5 of § Research's native architecture and
begins pumping frames. **On Android 14+ targets, the handed-in `MediaProjection` is single-use —
a second `open()` needs a fresh `MediaProjection` from a fresh host-app consent flow**; this
crate cannot cache or reuse one across sessions, and must fail loudly (not silently degrade) if a
caller tries to reopen with an already-consumed handle.

### `close()`

Calls `mediaProjection.stop()` via JNI (releases the projection and its associated system
screen-recording notification — a real resource that leaks if never stopped), then drops the
native `ImageReader` (`Drop` → `AImageReader_delete`).

## New dependency: `jni`

Not previously a dependency anywhere in this workspace. Real, needed here specifically because
`createVirtualDisplay`/`mediaProjection.stop()` are Java methods this crate must call.

| Question | Answer |
|----------|--------|
| Need | Real — no way to call a Java method from Rust without a JNI binding; this is the one domain in this crate that genuinely needs it. |
| License | `MIT OR Apache-2.0` (confirmed: `jni-rs/jni-rs`, dual-licensed, matches this workspace's own license exactly) — allowed. |
| Version / **API-shape caution, found this session** | `docs.rs`'s "latest" (`0.22.4`) showed a **lifetime-token-heavy** API (`JObject::from_raw<'local>(_env: &Env<'local>, raw: jobject) -> JObject<'local>`) that differs meaningfully from the older, more widely documented `jni 0.21` shape (`JNIEnv<'a>` with `call_method`/`new_global_ref` directly) — the same major-version shape `ndk`'s own **optional**, disabled-by-default `test` feature pins (`jni = "0.21"`). **Do not assume method names/signatures from memory or from older tutorials across this version boundary** — pin an exact version and verify its real API against source before writing any call, the same discipline that caught two real bugs in this workspace's earlier Apple/D3D12 encoder work. |
| Maintenance | `jni-rs` org (formerly `jni-rs/jni` under an individual maintainer, now a dedicated GitHub org) — the de-facto standard Rust↔JNI binding, used across the Android Rust ecosystem. |
| Alternatives | Hand-rolled raw JNI FFI via `jni-sys` alone (already a transitive dependency of `ndk-sys`) — rejected for the same reason raw `AMediaCodec`/Camera2 FFI was rejected elsewhere in this ADR set where a safe wrapper exists: JNI method-ID lookup/signature-string plumbing is a real, silent-failure-prone surface a maintained wrapper already solves. |

## ZCA / typestate shape

| Existing precedent | MediaProjection screen capture | Difference |
|---|---|---|
| `DesktopCaptureSource::Window { window: NativeHandle }` — opaque platform handle already modeled | `AndroidScreenCaptureConfig { media_projection: NativeHandle, java_vm: NativeHandle, .. }` | First config in this crate taking a **foreign-object** handle (a live JNI reference) rather than a platform window/output token — genuinely new shape, not just a new enum variant |
| DXGI/portal fully own OS session setup | Rust owns `ImageReader`+`Surface`+`createVirtualDisplay`+frame pump; **host app owns only the unavoidable consent-UI step** | First backend in this crate where session setup is split across a hard language/runtime boundary neither side can avoid |
| `GpuBufferHandle::AndroidSurface` — deferred (encoder ADR-0001, reused verbatim in ADR-0001 of this set) | Same, sourced from `Image::hardware_buffer()` this domain too | No new type-design work |

No `Box<dyn _>` planned; `AndroidScreenCapture` wraps a concrete inner session (JNI global refs +
`ImageReader` + worker thread) behind `Option<…>`, matching every other backend's shape.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Per-frame JNI pixel marshaling via Java `android.media.ImageReader` | Rejected — § Research's native `AImageReader`→`Surface` bridge achieves the same result with zero per-frame JNI cost, only a one-time `open()` round trip. Strictly worse on every axis once the cleaner path was found. |
| Have the host app's Kotlin code call `createVirtualDisplay` itself (Rust only supplies the `Surface`, never touches `MediaProjection`) | Considered — would shrink this crate's JNI surface to zero calls (only receiving a `Surface` back), but breaks the "Rust owns the whole session" property every other backend has, and pushes `VirtualDisplay` lifecycle/`stop()` management onto the host app instead of this crate's own `close()` — a worse API for callers who otherwise get a uniform `DesktopVideoCapture` contract. Rejected in favor of Rust owning `createVirtualDisplay`, but flagged as a real, simpler alternative if the JNI surface here proves too fragile once actually compiled (see § Open questions #2). |
| Skip screen capture entirely this pass, ship camera + mic only | Contradicts the user's explicit "camera + mic + screen 모두" direction. Not taken. |
| A same-shape API to `DesktopVideoCaptureConfig`/`CaptureSource::Screen` (reuse the existing type) | Rejected — that type has no slot for a `MediaProjection` handle, and forcing screen capture's genuinely different (host-app-JNI-dependent) contract into the existing shape would be dishonest about how different this domain really is. A dedicated `AndroidScreenCaptureConfig` documents the difference instead of hiding it. |

## ⚠️ CI verification plan — same posture as the encoder's Android ADR, plus a JNI-specific gap

**Zero `cargo check`/`clippy`/`test` run against this ADR.** The existing `android` CI job
(`.github/workflows/ci.yml`) needs, at minimum, a `mediaway-device` lint step (see ADR-0001/0002).
**A real limitation this ADR must state plainly: even a green CI compile-check cannot verify the
JNI method-signature strings** (`createVirtualDisplay`'s JNI signature,
`Landroid/hardware/display/VirtualDisplay;` etc.) **are correct** — a wrong signature string is a
runtime JNI failure (`NoSuchMethodError`), not a compile error, the same class of risk `v4l`'s
"wrong `_IOWR` ioctl number" gap was for V4L2 (ADR-0002 in the Linux set) but *worse* here because
there is no local Android emulator/device in this environment to catch it even after compiling.
This is a real, currently-unclosed verification gap beyond what compile-only CI can catch — flag
this to the user explicitly, do not let a green CI job imply more confidence than it earns.

## Open questions (for user confirmation)

1. **This ADR's central conclusion — the host-app contract shape itself** — is the most important
   thing to confirm: does `AndroidScreenCaptureConfig` taking a raw `MediaProjection`
   `NativeHandle` + `JavaVM` `NativeHandle` match what the user wants, or should this crate push
   *more* of the JNI work onto the host app (§ Alternatives — "host app calls
   createVirtualDisplay itself, hands Rust only a `Surface`")? This changes this crate's JNI
   dependency footprint materially.
2. **Should this ADR's `jni` dependency, and the JNI call surface generally, live in
   `mediaway-device::android` directly, or in a new dedicated sub-module/feature so a build that
   only wants camera+mic (ADR-0001/0002, no JNI needed at all) doesn't pull in `jni`?** No
   existing precedent in this crate for an optional sub-feature within one platform module — worth
   deciding deliberately rather than defaulting to "always-on."
3. **Exact `jni` crate version + API generation to target** (`0.21`-shape vs the newer
   lifetime-token `Env<'local>` shape seen in current `docs.rs` "latest") — real ambiguity found
   this session, needs a decision before any code is written, not an assumption.
4. **`density_dpi`/`flags` values for `createVirtualDisplay`** — Android's own docs recommend
   `DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR` for typical screen-mirroring use cases;
   unverified against a real device this session.
5. **Shared with ADR-0001/0002: overall `mediaway-device::android` minSdk floor.** This domain's
   clean architecture needs **API 26** (`nativewindow`+`api-level-26` for `to_surface`) — the same
   floor ADR-0002 (AAudio) independently needs. Recommendation, now reinforced by two independent
   domains: **`mediaway-device::android`'s floor should be 26**, differing from
   `mediaway-encoder::android`'s 21 — two separately-scoped decisions, not a contradiction. A
   `minSdkVersion < 26` fallback for screen capture would mean abandoning the clean native
   `Surface` bridge for a fully-Java `android.media.ImageReader` + per-frame JNI pixel copy
   (works down to API 21, real added JNI/copy cost per frame) — a real, presentable trade-off if
   the user wants that instead.
6. **CI**: extend the existing `android` job with a `mediaway-device` step at `-p 26` (per #5), or
   a separate job — mirrors ADR-0001/0002's open CI question, decided once for the whole module.

## Decisions confirmed with the user (2026-08-12)

1. **Host-app contract shape — this ADR's central question**: **Rust owns
   `createVirtualDisplay`** (option 1 from § Open questions), matching every other backend's
   "owns the whole OS session" property. The host app's JNI surface stays at the irreducible
   minimum: run the consent flow, convert the result to a JNI global reference, hand it (plus a
   `JavaVM*`) to `AndroidScreenCaptureConfig`.
2. **`jni` dependency scope**: always-on in `mediaway-device::android` (a single
   `[target.'cfg(target_os = "android")'.dependencies]` entry, no sub-feature gating a
   camera+mic-only build away from it) — the user's chosen vertical slice includes all three
   domains together, so there is no camera+mic-only Android build this session that would
   benefit from excluding `jni`.
3. **`jni` crate version**: **0.22.4**, the newer lifetime-token `Env<'local>`/`JavaVM` API —
   chosen over the older, more widely documented `0.21` `JNIEnv<'a>` shape despite this ADR's own
   recommendation leaning toward 0.21 for lower risk. `jni-sys` version mismatch between `ndk`
   0.9 (pinned to `jni-sys 0.3`) and `jni` 0.22 (built on `jni-sys 0.4`) is bridged via raw
   pointer casts in `screencast.rs` — see that file's module docs for the full reasoning.
4. **`density_dpi`/`flags` for `createVirtualDisplay`**: not hardcoded — both are real
   `AndroidScreenCaptureConfig` fields the host app must supply, specifically because guessing
   the real `DisplayManager.VIRTUAL_DISPLAY_FLAG_*` bit values without being able to verify them
   against real Android SDK stubs this session would risk silently shipping a wrong constant;
   the host app has real compile-time access to those Java constants and doesn't need to guess.
5. **minSdk floor**: **26**, confirmed (reinforced independently by ADR-0002's AAudio
   requirement too).
6. **CI**: the existing `android` job gets a `mediaway-device` compile+clippy step at API level
   26 (see `.github/workflows/ci.yml`) — no separate job.

## Implementation notes (2026-08-12, written alongside the code)

- `run_screencast_session` runs the **entire** session lifecycle — `ImageReader` setup,
  `NativeWindow`→`Surface` conversion, `createVirtualDisplay`, the poll loop, and
  `mediaProjection.stop()` teardown — inside one `JavaVM::attach_current_thread` closure on the
  worker thread, so the thread stays attached for the session's whole lifetime and `stop()` runs
  on the same attached thread it was created on (no separate re-attach needed for teardown).
- `env.global_from_raw::<JObject>(media_projection_raw)` (real jni 0.22 API, found while
  implementing — not in the original ADR text) takes ownership of the host-supplied raw global
  reference and its `Global`'s `Drop` calls `DeleteGlobalRef` automatically — this is how
  "Rust owns deleting the global reference" (config field doc comment) is actually implemented,
  not a manual raw JNI call.
- `createVirtualDisplay`'s JNI signature string
  (`(Ljava/lang/String;IIIILandroid/view/Surface;Landroid/hardware/display/VirtualDisplay$Callback;Landroid/os/Handler;)Landroid/hardware/display/VirtualDisplay;`)
  and `mediaProjection.stop()`'s (`()V`) are passed via `jni_sig!` macro calls on plain
  traditional JNI signature strings (confirmed this macro accepts raw strings, not only its
  ergonomic DSL, via `jni`'s own doc examples) rather than attempting the DSL form for an
  8-parameter signature — lower risk with zero compile verification available.
- `callback`/`handler` (the last two `createVirtualDisplay` parameters) are passed as JNI
  `null` — this session doesn't need `VirtualDisplay.Callback` state notifications since the
  `ImageReader` already drives frame delivery independently.
- **Real, currently-unclosed verification gap** (already flagged in this ADR's own § CI
  verification plan): none of the JNI method-name/signature strings above have been checked
  against a real `MediaProjection`/`VirtualDisplay` at runtime — a wrong signature string
  produces a runtime `NoSuchMethodError`, not a compile error, and this workspace has no Android
  emulator/device to catch that even after CI's compile step goes green.

## Consequences

### Positive

- Found and documented a genuinely cleaner native architecture (native `AImageReader` →
  `ANativeWindow_toSurface` → one-time `createVirtualDisplay` JNI call) than the naive per-frame
  JNI pixel-marshaling design — a real research win, not just a compile-only exercise.
- Confirmed, via primary Android documentation, a real Android 14+ single-use consent gotcha that
  must inform this crate's own API-honesty documentation (`capture_desktop_video_once`'s
  suitability here).
- The host-app contract is now written down explicitly and completely — nothing about this
  domain's Kotlin/Java requirement is left implicit or discovered later during implementation.

### Negative / Trade-offs

- **Zero compile verification as authored**, plus a **JNI-signature-string verification gap CI
  alone cannot close** (§ CI verification plan) — a strictly weaker verification posture than
  ADR-0001/0002 in this same set.
- Real, load-bearing host-app burden: a custom Activity subclass with genuine Kotlin/Java code is
  unavoidable — this domain cannot be made "just call `open()`" the way DXGI/portal are.
  Documenting this honestly (rather than pretending parity with other platforms) is itself a
  cost this ADR accepts deliberately.
- New `jni` dependency, this crate's first — a real new unsafe/FFI surface class
  (`docs/spec/c-ffi.md`'s spirit, though this is JNI not C-FFI proper) this crate has not carried
  before.
- Android 14+'s single-use consent means this domain's `open()` cannot be treated as a cheap,
  silently-repeatable operation the way every other backend's `open()` is — a real UX and API-
  design constraint, not solvable by better Rust code.

## References

- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) — costly-path
  documentation requirement this ADR's § "capture_desktop_video_once degrades here" finding
  invokes directly
- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
- `mediaway-device` [ADR-0021](../../../../docs/adr/0021-workspace-consolidation.md)
- ADR-0001 (this set) — `GpuBufferHandle::AndroidSurface` reuse, `ndk::media::image_reader`
  precedent
- ADR-0002 (this set) — shared minSdk-26 reinforcement, host-app-permission-vs-host-app-consent
  distinction
- [`android-activity` (`rust-mobile`)](https://github.com/rust-mobile/android-activity) —
  `vm_as_ptr`/`activity_as_ptr`/`run_on_java_main_thread`, confirmed no activity-result surface
- [`jni-rs`](https://github.com/jni-rs/jni-rs) (`MIT OR Apache-2.0`) — not yet a dependency
  anywhere in this workspace
- [Media projection — Android Developers](https://developer.android.com/media/grow/media-projection)
- [Android 14 behavior changes — MediaProjection single-use consent](https://developer.android.com/about/versions/14/behavior-changes-14)
- `local/vendor-ref/ndk/ndk/src/media/image_reader.rs` ·
  `local/vendor-ref/ndk/ndk/src/native_window.rs` — real source read this session for the
  `AImageReader`/`ANativeWindow_toSurface` bridge
