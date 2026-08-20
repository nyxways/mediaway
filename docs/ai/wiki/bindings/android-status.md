# Android binding — status

## Status (2026-08-20): design-only, ADR-0025

**Coldest-start binding in the workspace.** No JNI bridge code, no Kotlin/
Java source, no Gradle project exists anywhere in this repo yet — confirmed
via `Glob` on `bindings/` (only `browser/`, `c/`, `cpp/`, `csharp/`,
`native/`, `nodejs/`, `python/` exist). [ADR-0025](../../adr/0025-android-aar-maven-binding-distribution.md)
is the design draft (Status: Proposed); no code has landed.

## What this wraps

Three real, `#[cfg(target_os = "android")]`-gated Rust backends already
exist (see [device/android-capture.md](../device/android-capture.md) and
each crate's own `adr/android/`):

- `mediaway-encoder::android` / `mediaway-decoder::android` — NDK
  `AMediaCodec`, H.264 only, **minSdk 21**.
- `mediaway-device::android` — Camera2 NDK (camera) + AAudio (mic) +
  `MediaProjection` via JNI (screen), **minSdk 26**.

All three: zero compile verification, zero real-device/emulator
verification — `.github/workflows/ci.yml`'s `android` job (compile+lint
only, `arm64-v8a`) is the only proof any of it even builds.

## ADR-0025's key decisions

- **JNI bridge**: a new `jni` Cargo feature + `android` module **inside the
  existing `mediaway-ffi` crate** (not a new `-ffi` crate, not a shim that
  re-enters through the C ABI) — JNI-native functions calling the same
  underlying Rust capability crates the C modules already wrap.
- **Not the same `jni` usage as `mediaway-device::android::screencast`** —
  that one is Rust-calls-Java (sourcing a `MediaProjection` object from a
  host Activity's consent flow); this binding's JNI is Java/Kotlin-calls-
  Rust (native methods), the opposite direction, unrelated purpose.
- **AAR split**: `mediaway-common`/`mediaway-container`/`mediaway-pipeline`
  (minSdk 21) depend on a shared `mediaway-android-native` AAR; `mediaway-
  device` (minSdk 26, including the `MediaProjection` host-Activity consent
  contract) depends on a separate `mediaway-android-native-device` AAR —
  first real use of `mediaway-ffi`'s existing per-capability Cargo feature
  gating beyond "all features on."
- **Maven Central**: Sonatype Central Portal (OSSRH sunset 2025-06-30), no
  confirmed GitHub-Actions-OIDC-equivalent found (unlike npm/NuGet/PyPI) —
  GPG signing + a Sonatype user token are new operational surface. groupId
  (`com.mediaway` vs. GitHub-verified `io.github.nyxways`) left open.
- **ABIs**: `arm64-v8a` + `x86_64` (emulator dev) first; `armeabi-v7a`/`x86`
  deferred, no CI verification either.
- Sequencing mirrors [ADR-0024](../../adr/0024-multi-platform-native-binding-distribution.md)'s
  staged rollout: real Kotlin/JNI source lands before any packaging/publish
  job, matching every other binding's own order.

## Open questions (see ADR-0025 § Deferred)

Maven Central CI auth mechanism, groupId ownership, GPG key custody,
`mediaway-device`'s Kotlin `ScreenCaptureConsent` API shape, Android
emulator CI feasibility (plausibly better than Linux's GPU-less gap for
camera/mic, since the emulator has a real emulated camera/AAudio HAL —
unconfirmed for `MediaProjection` consent-dialog automation).
