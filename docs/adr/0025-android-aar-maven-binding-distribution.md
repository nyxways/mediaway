# ADR-0025: Android AAR / Maven Central binding distribution

- **Status**: Proposed
- **Date**: 2026-08-20
- **Deciders**: @dev-nyxie (+ agent)

## Context

[ADR-0024](0024-multi-platform-native-binding-distribution.md) extended native
binding distribution to Linux/macOS across npm, NuGet, PyPI, and CPack, and
explicitly scoped Android **out**: "Android consumers reach Mediaway through
the NDK `.so` directly via Gradle/AAR — **not** through npm/NuGet/PyPI/CPack…
A dedicated future ADR should design the AAR/Maven shape when a concrete
Android-binding consumer exists." This is that ADR.

**This is a genuinely colder start than every other binding.** Every other
language (C, C++, C#, Python, Node.js) already has real, hardware-or-CI-
verified source under `bindings/<lang>/` per
[`docs/ai/wiki/bindings/status.md`](../ai/wiki/bindings/status.md). Confirmed
via `Glob` before writing this ADR: `bindings/` currently has `browser/`,
`c/`, `cpp/`, `csharp/`, `native/`, `nodejs/`, `python/` — **no `android/`
directory, no JNI bridge code, no Kotlin/Java source, no Gradle project
anywhere in this repository.** This ADR designs from nothing, unlike ADR-0024
(which only needed to extend an already-working four-ecosystem release
pipeline to two more platforms) and unlike the C#/Python/Node ADRs (which
designed a wrapper layer over an already-shipping C ABI).

### What already exists on the Rust side (ground truth for this ADR)

Three Android backends exist today, all `#[cfg(target_os = "android")]`-gated
modules inside existing crates (ADR-0021 — no separate `-android` crates):

| Crate module | Backend | minSdk | Source |
|---|---|---|---|
| `mediaway-encoder::android` | NDK `AMediaCodec`, H.264 CPU-upload encode | **21** | `crates/mediaway-encoder/adr/android/0001-ndk-amediacodec-h264-cpu-upload.md` |
| `mediaway-decoder::android` | NDK `AMediaCodec`, H.264 CPU-output decode | **21** | `crates/mediaway-decoder/adr/android/0001-ndk-amediacodec-h264-cpu-out.md` |
| `mediaway-device::android` | Camera2 NDK (camera) + AAudio (mic) + `MediaProjection` (screen, via JNI) | **26** | `crates/mediaway-device/adr/android/0001`–`0003-*.md` |

Two facts from those ADRs are load-bearing for this one:

- **The minSdk split is real and already decided, not something this ADR
  invents.** `mediaway-encoder`/`mediaway-decoder` need only API 21
  (`AMediaCodec`'s own introduction level). `mediaway-device` needs API 26
  because `ndk`'s `audio` Cargo feature hard-requires `api-level-26`
  (confirmed from `ndk/Cargo.toml`: `audio = ["ffi/audio", "api-level-26"]`)
  and the screen-capture bridge's `ANativeWindow_toSurface` needs the same
  floor. `.github/workflows/ci.yml`'s `android` job already lints the two
  crate families at two different `-p <api-level>` values for exactly this
  reason.
- **`mediaway-device::android::screencast` already depends on `jni = "0.22.4"`
  (`jni-rs`, `MIT OR Apache-2.0`)** — but for a narrow, *outbound* purpose:
  Rust code calling **into** a host app's already-obtained `MediaProjection`
  Java object (`createVirtualDisplay`, `mediaProjection.stop()`). That ADR's
  own § Research is explicit that this exists because
  `MediaProjection`'s consent flow has **no NDK equivalent** — Android's
  platform design only delivers `onActivityResult` to a JVM `Activity`
  subclass, so a host app's Kotlin/Java Activity must run the consent dialog
  and hand the resulting `MediaProjection` object to Rust. **This is not a
  general JNI bridge and must not be conflated with the JNI bridge this ADR
  designs below** (Java/Kotlin calling **into** the Rust `.so` as native
  methods — the opposite call direction, for an unrelated purpose: giving
  *any* JVM host language access to the library, not sourcing one specific
  consent object). The `mediaway-decoder::android` ADR separately rejected
  JNI `android.media.MediaCodec` **for the codec backend itself**, to keep
  that backend usable from a headless native entry point with no JVM
  attached — also unrelated to this ADR's binding-layer JNI bridge, which
  runs at a different layer entirely (it does not touch codec internals).
- **`MediaProjection` screen capture is the one capability that cannot be
  made "just call `open()`" no matter how this ADR packages things.** A real
  Activity subclass with genuine Kotlin code, launching the system consent
  dialog and forwarding the result into a JNI-exported native method, is
  unavoidable — this ADR's binding layer must expose that contract
  explicitly to AAR consumers (a Kotlin `ScreenCaptureConsent` helper
  mirroring the Activity-result flow), not hide it behind a same-shape
  `open()` call the way camera/mic get.

### CI status today

`.github/workflows/ci.yml`'s `android` job (`ubuntu-latest`,
`nttld/setup-ndk` pinned to NDK r27c, `cargo-ndk`) compiles + lints
`mediaway-encoder`/`mediaway-decoder` at `-p 21` and `mediaway-device` at
`-p 26`, `arm64-v8a` only, **compile-only — no emulator, no device.** This is
the *only* verification any Android code path has ever had in this
workspace, matching the "authored with zero runtime verification" honesty
posture every Android ADR above states explicitly. There is no `.aar`
build/publish job anywhere; this ADR's own proposal has had **zero**
`./gradlew build` run against it, for the same reason the Rust Android ADRs
had zero `cargo check` run against them at authoring time: this dev
environment (Windows host) has no local emulator/device and this research
pass did not attempt a real Gradle build.

### Binding-quality bar this ADR must meet

Per the project's established, repeatedly-confirmed convention (C#'s
`SafeHandle`-based design, ADR-0017; carried through Python/Node/C++): **every
binding must be genuinely idiomatic, not a thin passthrough with syntax
sugar.** For Kotlin that means: `AutoCloseable`/`use {}` resource safety (the
JVM/Android analog of `SafeHandle`'s deterministic-plus-finalizer cleanup),
sealed classes for `CodecKind`/status enums instead of raw `Int` constants,
coroutines/`Flow` for continuous frame delivery (the Kotlin analog of C#'s
`IAsyncEnumerable<T>`), and typed exceptions instead of raw native status
codes leaking into public API — the same shape ADR-0017 § Safety design
already committed to, adapted to Kotlin idiom rather than re-derived.
`docs/spec/c-ffi.md`'s own Tier B table already lists **"Kotlin / Java | JNI
over the C ABI | Aligns with Android platform track"** as the aspirational
plan — this ADR is the first pass at making that concrete, and (see
§ Decision 1) finds a real reason to refine "over the C ABI" into "JNI
functions living in the same crate as the C ABI, calling the same underlying
Rust capability crates directly" rather than literally routing through the C
calling convention.

## Decision

> Ship a **Gradle multi-module Android library project** under
> `bindings/android/` (a future implementation PR, not this ADR) publishing
> per-capability AARs to **Maven Central via the Sonatype Central Portal**,
> backed by a **new `jni` Cargo feature on the existing `mediaway-ffi`
> crate** — not a new `-ffi` crate, and not a thin JNI shim that re-enters
> through the C ABI.

### 1. JNI bridge layer: a sibling module inside `mediaway-ffi`, not a shim over the C ABI

Two shapes were weighed (per the task's framing):

- **(a) JNI-glue-calls-C-ABI**: a thin native shim (C or Rust) whose exported
  `Java_com_mediaway_..._nativeXxx` functions turn around and call the
  existing `mediaway_ffi.h` C functions (`mediaway_muxer_session_open`,
  status-code out-params, opaque `*mut c_void` handles).
- **(b) JNI-native sibling**: `extern "system" fn Java_com_mediaway_..._nativeXxx`
  functions written directly in Rust (via `jni-rs`), living in a new `android`
  module inside `mediaway-ffi` itself, calling the **same underlying Rust
  capability crates** (`mediaway-container`, `mediaway-device`, `mediaway`,
  `mediaway-encoder`, `mediaway-decoder`, `mediaway-sw`) that the existing
  `common`/`container`/`device`/`pipeline` C modules already wrap — a
  JNI-shaped peer of those modules, not a layer on top of them.

**Decision: (b).** The C ABI's opaque-handle-plus-status-code shape
(`docs/spec/c-ffi.md` § Design rules: "Opaque handles + error codes; no panic
across FFI") exists specifically for C callers with no exceptions, no
generics, and no rich marshaling — every one of those constraints is *absent*
for a JNI caller, which can throw real Java exceptions, receive real Kotlin
sealed-class values, and use `jni-rs`'s own safe wrappers for strings/arrays/
objects directly. Routing JNI calls through the C ABI first would add a
second boundary crossing (JNI → C ABI → Rust internals) that buys nothing —
the C status codes would just get re-decoded into exceptions on the JNI side,
duplicating logic the `android` module can do once, directly, against the
same Rust types. This also avoids **conflating** the two unrelated existing
`jni` uses this ADR's own research found: `mediaway-device::android`'s
outbound `MediaProjection` JNI calls (Rust → Java, one narrow purpose,
already shipped) stay exactly as they are; this ADR's binding-layer JNI is
inbound (Java/Kotlin → Rust), a different direction serving a different
purpose, and does not touch that module's internals.

**Not a new `-ffi` crate.** ADR-0004/ADR-0021 already establish "add a module
+ feature to `mediaway-ffi`, not a second `-ffi` crate" for new
*capabilities* — a JNI calling convention is not a new capability, it is a
new **consumer** of the same capabilities the C modules already expose, so
the same "no crate proliferation" reasoning applies: a new **`jni` Cargo
feature**, gating a new `android` module
(`crates/mediaway-ffi/src/android/{common,container,device,pipeline}.rs`,
mirroring the C modules' own split), `#[cfg(all(target_os = "android"))]`
throughout so `cargo check --workspace` on every other platform never sees
JNI code. `jni-rs` version: **pin to `0.22.4`**, the exact version
`mediaway-device::android::screencast` already depends on — that ADR's own
§ New dependency table flagged a real API-shape difference between `jni
0.21`'s `JNIEnv<'a>` and `jni 0.22`'s lifetime-token `Env<'local>` generation;
using the same version already in the dependency graph avoids adding a
second, differently-shaped `jni-rs` generation to reason about, and avoids
Cargo resolving two `jni`/`jni-sys` major versions into one build.

Handle representation: JNI's native-pointer idiom is `jlong` (not `*mut
c_void`) — the `android` module `Box::into_raw`s the **same underlying Rust
session/handle types** the C modules already box (`Muxer`, `Demuxer`,
`EncodeSession`, `CameraCapture`, …), cast through `jlong`, stored in a
Kotlin class's private `private var nativeHandle: Long`. No duplicate
session-state logic between the C and JNI modules — both are thin call-
throughs over the same Rust structs.

### 2. AAR structure: capability AARs over a shared, feature-sliced native AAR

Mirrors this workspace's per-capability convention (`Mediaway.Container`/
`Mediaway.Device`/`Mediaway.Pipeline` in C#; `@mediaway/container`/`device`/
`encoder`/`decoder` in npm) with a genuine Android-specific improvement over
both: Gradle/AAR lets a **native-only AAR be a shared transitive dependency**
of multiple capability AARs without each one embedding its own copy of the
same `.so` (unlike NuGet's flat per-package `runtimes/<rid>/native/` folders,
where each of the four `Mediaway.*` packages independently stages the same
DLL). Android's build system also dedupes `jniLibs` by ABI at final APK
assembly regardless, so this is not solving a real bloat problem so much as
avoiding a maintenance one (N identical binary copies staying in sync across
N package builds).

**Two native artifacts, not one**, to keep the minSdk split real instead of
forcing every consumer to the higher floor (see § 4):

| Artifact | `mediaway-ffi` Cargo features | minSdk it can honestly declare |
|---|---|---|
| `mediaway-android-native` | `jni`, `mux`, `demux`, `pipeline` (container + auto-encode/decode only — **not** `camera`/`desktop`/`audio`/`hotplow`) | **21** |
| `mediaway-android-native-device` | `jni`, `mux`, `demux`, `pipeline`, `camera`, `desktop`, `audio`, `hotplug` (everything) | **26** |

This is the **first real consumer of `mediaway-ffi`'s existing per-capability
Cargo feature gating for anything other than "all on"** — every other
binding (Windows/Linux/macOS, all four ecosystems in ADR-0024) ships the
`default` (all-features) build unconditionally. `docs/spec/c-ffi.md`'s
feature table already documents this gating exists; this ADR is simply the
first packaging layer to actually build two different feature combinations
instead of always defaulting to "every feature on" — real, useful exercise
of infrastructure that already existed unused, not new complexity invented
for Android specifically.

Kotlin/capability AAR layer, each depending on the narrower native artifact
that satisfies its own minSdk:

| AAR (`com.mediaway:<artifact>`) | Wraps | Depends on native artifact | minSdk |
|---|---|---|---|
| `mediaway-common` | Shared value types (`Rational`, `CodecKind` sealed class, `MediawayException` hierarchy) | none (no native asset — mirrors `Mediaway.Common`'s own "no native asset of its own" precedent) | 21 |
| `mediaway-container` | `Muxer`/`Demuxer` | `mediaway-android-native` | 21 |
| `mediaway-pipeline` | Auto video/audio encode, Opus decode | `mediaway-android-native` | 21 |
| `mediaway-device` | Camera/Microphone/`ScreenCapture` (+ the `MediaProjection` host-Activity consent contract, exposed as a Kotlin `ScreenCaptureConsent` helper) | `mediaway-android-native-device` | 26 |

No fat umbrella package in v1, matching ADR-0017's own "no `Mediaway.All` in
v1" call.

**ABIs shipped**: `arm64-v8a` (matches the existing CI job's only compiled/
linted target — the only ABI with *any* verification today) and `x86_64`
(cheap to cross-compile via `cargo-ndk`, and the one ABI that makes the
Android **emulator** usable for local app-developer testing — real, concrete
value even though production devices are effectively all `arm64-v8a` today).
**`armeabi-v7a`/`x86` deferred** — no CI verification exists for either, 32-
bit ARM device share keeps shrinking, and `mediaway-device::android`'s own
minSdk-26 floor already excludes the API-21-era hardware where `armeabi-v7a`
would matter most. Same "no speculative wider floor" reasoning ADR-0024 used
for the Linux glibc / macOS deployment-target floors — revisit only against
a real consumer request.

### 3. Maven Central: Sonatype Central Portal, groupId decision deferred, GPG is new operational surface

**Central Portal (`central.sonatype.com`), not OSSRH.** Verified via web
search during this ADR's research: OSSRH's HTTP-PUT staging-repo endpoint was
sunset **June 30, 2025** in favor of the Central Portal's Publisher API —
asserting an OSSRH-shaped design here would be a real, dated mistake this ADR
must not make.

**No confirmed OIDC/Trusted-Publishing-equivalent for GitHub Actions.**
`release.yml`'s own comments describe npm, NuGet, and PyPI as needing **"NO
long-lived tokens"** via each registry's GitHub-Actions-OIDC trusted-
publisher mechanism. This research pass found **no equivalent, confirmed
mechanism for the Sonatype Central Portal** — the Portal's own login supports
OIDC for the *human dashboard*, but the Publisher API itself authenticates
via a **user token** (a username/password-shaped credential pair generated
from the portal UI), used as HTTP Basic auth from CI — a real, structural
asymmetry from this repo's stated preference. This is flagged as **not
found**, not as **confirmed absent**: re-check Sonatype's current publishing
docs immediately before any implementation PR, since this space (like PyPI's
own Trusted Publishing rollout) moves quickly and a GitHub-Actions-native
flow may exist or land before implementation.

**GPG signing is a hard Maven Central requirement** (every jar, sources jar,
javadoc jar, and POM must be signed) — unlike npm/PyPI, which require no
artifact signing at all. This is real new operational surface this project
has not had to build before: a GPG keypair generated and stored as GitHub
Actions secrets (`GPG_PRIVATE_KEY`, `GPG_PASSPHRASE`), key-rotation policy,
and public-key distribution (a keyserver upload) — none of which
`tools/scripts/release-secrets.ts` currently documents or provisions for any
existing publish target. This must be provisioned by the user as a real,
human precondition before any implementation PR can merge a working publish
job — it cannot be a code-only change the way extending `release.yml` with a
new Linux/macOS `native-assets-*` job was in ADR-0024.

**groupId: `com.mediaway` is the natural, precedent-consistent choice**
(matches `@mediaway` on npm, `Mediaway.*` on NuGet, `mediaway` on PyPI/
crates.io) **but is not decided here** — Maven Central requires proving
namespace ownership, either via a DNS TXT record on the `mediaway.com`
domain (if the project owns/controls it — unconfirmed by this research pass)
or via the GitHub-verified-namespace fallback (`io.github.nyxways`, proving
only GitHub repository ownership, materially easier but a less-branded
groupId). **Flagged as an open decision for the user**, not assumed either
way.

### 4. minSdk: split by AAR, not by runtime check inside one AAR

Already decided implicitly by § 2's two-native-artifact split: `mediaway-
common`/`mediaway-container`/`mediaway-pipeline` declare **minSdk 21**;
`mediaway-device` declares **minSdk 26**. An app that only needs container/
pipeline can target API 21 exactly as `mediaway-encoder::android`/`mediaway-
decoder::android` already allow at the Rust level; an app that also wants
camera/mic/screen capture must raise its own `minSdk` to 26 to add the
`mediaway-device` dependency — the same floor `mediaway-device::android`
already committed to at the Rust ADR level (ADR-0002/0003 in that set).

Rejected: one AAR at minSdk 21 with runtime `Build.VERSION.SDK_INT` guards
throwing at call time for camera/mic/screen below 26. That would force the
**native `.so` itself** to be built once at the lower API level yet still
reference AAudio/`ANativeWindow_toSurface` symbols only resolvable at 26+ —
a real risk this ADR does not want to carry into production without a real
device to verify Android's dynamic-linker behavior for lazily-bound symbols
referenced-but-uncalled below their introduction API level. Two separately-
floored native artifacts avoids that question entirely by never compiling
device code into the same `.so` a 21-floor consumer loads.

### 5. CI/release verification reality

**Zero real-device/emulator verification, as authored, matching every
Android ADR in this workspace.** `ci.yml`'s `android` job stays the only
compile-time proof this ADR's design is even plausible, and only after it is
extended with a `jni` feature build step (`cargo ndk -t arm64-v8a -t x86_64
check -p mediaway-ffi --features jni,...`) — not yet done, a future
implementation-PR task, not this ADR.

**No `.aar` build/publish job exists today** — a real Gradle build/lint/
`./gradlew assembleRelease` CI job, and a separate Maven Central publish job
(distinct from `native-assets-*`/npm/nuget/pypi/native in `release.yml`),
are both entirely new CI surface this ADR does not build, only names as
necessary future work.

**An Android emulator CI job is more realistic than it might first appear**,
and worth flagging as a real, distinguishing opportunity rather than
deferred by default: `reactivecircus/android-emulator-runner` runs real
system images (API 26+ available) on GitHub-hosted `ubuntu-latest`/
`macos-latest` runners with hardware acceleration, and — unlike Linux's own
`ubuntu-latest` GPU-less gap for VA-API hardware verification — the Android
emulator ships a **usable emulated camera, virtual microphone, and AAudio
HAL**, meaning an emulator job could plausibly verify real camera/mic capture
behavior that the equivalent bare Linux CI runner genuinely cannot for
VA-API. `MediaProjection`'s consent-dialog automation (`uiautomator`/`adb
shell input`) is a known but nontrivial CI pattern whose feasibility this
research pass did not verify — flagged honestly as unconfirmed, not
asserted. **Recommendation: treat this as real future work (an RC-gate-style
job mirroring ADR-0024's `bindings-tests-*` jobs), not part of the first
implementation PR** — get compile-verified Kotlin/JNI source landed first,
the same staged order every other binding followed.

### 6. Sequencing

Not a single PR. Mirrors ADR-0024's staged-rollout philosophy and every
prior binding's own "real source lands before packaging ADR" order:

1. **This ADR** (design review, no code, no Gradle files, no
   `bindings/android/` directory — confirmed untouched).
2. **Future PR 1 — JNI bridge + Kotlin source**: `mediaway-ffi`'s new `jni`
   Cargo feature and `android` module (container/pipeline first, mirroring
   this ADR's minSdk-21 slice; device JNI functions can land in the same or
   a following PR once camera/mic/screen wrapping is designed in Kotlin
   detail). `bindings/android/` Gradle multi-module project with real
   Kotlin source for at least `mediaway-common`/`mediaway-container`.
   `ci.yml`'s `android` job extended with a `jni`-feature build step.
   **Not published anywhere yet** — matches how C++/C#/Python/Node each had
   real, compiling source before any publish job existed.
3. **Future PR 2 — AAR packaging + Maven Central publish job**: requires the
   groupId decision (§ 3) and the GPG/Sonatype-token secrets provisioned by
   the user first — a real precondition, not a code-only PR.
4. **Future, separate consideration**: Android emulator CI job (§ 5),
   `mediaway-device`'s camera/mic/screen JNI wrapping if not already done in
   PR 1, `armeabi-v7a`/`x86` if a concrete consumer appears.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| JNI glue calls the existing C ABI (route through `mediaway_ffi.h`) | Adds a pointless second boundary crossing (JNI → C status codes/opaque pointers → Rust) that buys nothing a JNI caller needs — `jni-rs` already gives richer, safer marshaling than the C ABI's own C-caller-shaped design. See § 1. |
| A new `mediaway-android-ffi` crate, separate from `mediaway-ffi` | Contradicts ADR-0004/ADR-0021's "add a module + feature to `mediaway-ffi`, not a new `-ffi` crate" rule — a JNI calling convention is a new *consumer*, not a new *capability*; the existing crate's module/feature shape already fits. |
| One AAR, one native `.so` (full feature set), runtime `SDK_INT` guards for device-only calls | Forces every consumer to API 26 even for container-only use; carries unverified risk around lazily-bound AAudio/`ANativeWindow_toSurface` symbols in a `.so` loaded on sub-26 devices with no real device available to check. Two feature-sliced native artifacts avoids the question by construction. |
| One fat native AAR embedded redundantly inside each capability AAR (mirror NuGet's flat per-package native-asset shape exactly) | Works, but throws away a real Android/Gradle-specific improvement (a shared native-only AAR as a transitive dependency) for no benefit — NuGet's redundancy is a constraint of that ecosystem, not something to imitate where Gradle doesn't have the same constraint. |
| Ship `armeabi-v7a`/`x86` alongside `arm64-v8a`/`x86_64` from day one | No CI verification for either, shrinking real-device relevance for 32-bit ARM, and `mediaway-device::android`'s own minSdk-26 floor already excludes the hardware generation `armeabi-v7a` would matter most for. Same "no speculative wider floor" call ADR-0024 already made elsewhere. |
| Assume OSSRH's classic staging-repo flow | Factually wrong — sunset June 30, 2025. Central Portal is the only live path. |

## Consequences

### Positive

- Reuses `mediaway-ffi`'s existing per-capability Cargo feature gating for
  the first time for anything other than "all features on" — a real,
  low-risk exercise of infrastructure that already existed, not new
  complexity invented for this platform.
- The JNI-native-sibling design (§ 1) avoids a pointless double FFI hop and
  keeps the C ABI's own design (opaque handles, status codes) unpolluted by
  JNI-specific concerns — each calling convention gets the marshaling shape
  that actually fits it.
- The shared native-AAR-as-transitive-dependency shape is a genuine, if
  small, improvement over the NuGet precedent this ADR otherwise mirrors —
  Gradle's dependency model avoids N redundant copies of the same `.so`
  NuGet's flat package shape accepts.
- minSdk stays split and honest (21 vs. 26) at the AAR level, matching the
  Rust crates' own already-decided, independently-scoped floors instead of
  inventing a third, blended floor or silently forcing every consumer to 26.
- Explicitly reconciles, rather than silently glossing over, the two
  existing unrelated `jni` usages in this workspace (`mediaway-device::
  android::screencast`'s outbound `MediaProjection` calls vs. this ADR's
  inbound binding-layer JNI) — a real, previously-possible source of
  confusion closed by this ADR's own § Context.

### Negative / Trade-offs

- **Coldest-start binding in the workspace**: zero JNI bridge code, zero
  Kotlin/Java source, zero Gradle tooling exists anywhere today — every
  other binding at least had a working C ABI to wrap. This ADR is
  design-only; a real implementation PR (or several) is required before any
  of this is real, unlike ADR-0024's narrower "extend an already-working
  pipeline" scope.
- **No confirmed token-less publish path** for Maven Central, unlike npm/
  NuGet/PyPI's existing OIDC Trusted Publishing — a real new secrets/ops
  burden (GPG keypair generation, storage, rotation; Sonatype user token)
  this project has not had to build before, and a real human precondition
  before any publish job can go live.
- **groupId ownership is unresolved** — `com.mediaway` requires proving DNS
  ownership of a domain this research pass could not confirm is controlled
  by the project; the GitHub-verified-namespace fallback (`io.github.
  nyxways`) is easier but less branded. Left as an explicit open decision.
- **Two native artifacts instead of one** doubles the native-build surface
  this ADR's own future CI job must produce (two `cargo ndk` invocations per
  ABI instead of one) — a real, if modest, CI cost versus every other
  platform's single-feature-set build.
- Zero compile verification of this ADR's own JNI-bridge shape as authored —
  every method-naming convention (`Java_com_mediaway_..._nativeXxx`),
  `jlong` handle-boxing detail, and `jni-rs` 0.22 API call above is a design
  proposal, not something run through `rustc`/`javac`/Gradle this session.
- Screen capture's Activity-subclass consent contract (§ Context) means
  `mediaway-device`'s Kotlin API cannot be made uniform with `mediaway-
  container`/`mediaway-pipeline`'s "just call open()" shape — a real,
  documented asymmetry consumers must accept, not a gap this ADR's
  packaging choices can hide.

## Deferred / Open questions

- **Maven Central authentication mechanism for CI** — re-confirm whether any
  GitHub-Actions-native, token-less flow exists for the Sonatype Central
  Portal before implementation; this ADR's research found none confirmed,
  not a confirmed absence.
- **groupId decision** (`com.mediaway` via DNS-verified namespace vs.
  `io.github.nyxways` via GitHub-verified namespace) — needs a real user
  decision plus, if `com.mediaway` is chosen, DNS TXT-record provisioning on
  a domain the project must actually control.
- **GPG key generation, storage, and rotation policy** — net-new operational
  surface; needs a real decision (who holds the key, how it's rotated, where
  the public key is published) before any publish job can be written.
- **`mediaway-device`'s Kotlin `ScreenCaptureConsent` API shape** — this ADR
  names the requirement (a host-Activity consent contract mirroring the
  Rust-side `AndroidScreenCaptureConfig`) but does not design the exact
  Kotlin surface; a future implementation PR's job, informed by however
  `mediaway-device::android::screencast`'s own JNI signature strings are
  eventually verified against a real device.
- **Android emulator CI job** — plausibly more valuable here than the
  equivalent Linux GPU-less gap allows for VA-API (§ 5), but feasibility
  (especially `MediaProjection` consent-dialog automation) is unconfirmed;
  real future work, not committed to in the first implementation PR.
- **`armeabi-v7a`/`x86`** — no concrete consumer yet; revisit only against a
  real request, same reasoning ADR-0024 applied to its own deferred
  platforms.
- **Slim/full feature-set naming and any further slicing of
  `mediaway-ffi`'s Cargo features beyond the two artifacts this ADR
  proposes** — this ADR picks the minimal split that preserves the existing
  21/26 minSdk boundary; finer slicing (e.g. `camera`-only without `desktop`/
  `audio`) is not designed here and would need its own justification.

## References

- related ADR: [ADR-0003](0003-crate-packaging.md) (sans-io / backend / facade
  split), [ADR-0004](0004-c-ffi.md) (C-FFI surface, Tier B Kotlin/Java entry),
  [ADR-0017](0017-csharp-binding-package-layout.md) (idiomatic-binding
  quality bar, per-capability package layout precedent),
  [ADR-0021](0021-workspace-consolidation.md) (single `mediaway-ffi` crate,
  `#[cfg]`-gated backend modules — "no new `-ffi` crate" rule this ADR
  follows), [ADR-0024](0024-multi-platform-native-binding-distribution.md)
  (the sibling ADR this one was deferred from; staged-rollout philosophy this
  ADR mirrors)
- spec: [`docs/spec/c-ffi.md`](../spec/c-ffi.md) (Tier B Kotlin/Java entry,
  feature-gated module design), [`docs/spec/crate-packaging.md`](../spec/crate-packaging.md)
- crate: `crates/mediaway-ffi/Cargo.toml` (existing per-capability feature
  gating this ADR is the first to exercise beyond "all on")
- crate ADRs (Android backends this ADR wraps, read in full before writing
  this one): `crates/mediaway-encoder/adr/android/0001-ndk-amediacodec-h264-cpu-upload.md`,
  `crates/mediaway-decoder/adr/android/0001-ndk-amediacodec-h264-cpu-out.md`,
  `crates/mediaway-device/adr/android/0001-camera2-ndk-native-camera-capture.md`,
  `crates/mediaway-device/adr/android/0002-aaudio-microphone-capture.md`,
  `crates/mediaway-device/adr/android/0003-mediaprojection-jni-screen-capture.md`
- workflow: `.github/workflows/ci.yml` (`android` job — the only existing
  Android verification), `.github/workflows/release.yml` (existing npm/
  NuGet/PyPI OIDC Trusted Publishing patterns this ADR could not find a
  Maven Central equivalent of)
- wiki: [`docs/ai/wiki/bindings/status.md`](../ai/wiki/bindings/status.md)
- external: [Sonatype Central Portal](https://central.sonatype.com/) (OSSRH
  sunset June 30, 2025) · [`jni-rs`](https://github.com/jni-rs/jni-rs)
  (`MIT OR Apache-2.0`, already `0.22.4` in this workspace via
  `mediaway-device::android::screencast`)

ADRs are written in **English**.
