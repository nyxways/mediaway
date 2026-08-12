# ADR-0001: NDK `AMediaCodec` via the `ndk` crate, H.264 CPU-upload encode

- **Status**: Accepted (2026-08-12) — see § Decisions confirmed with the user
- **Date**: 2026-08-12
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (module `mediaway-encoder::android`, ADR-0021 `#[cfg]`-gated
  backend — no separate `mediaway-encoder-android` crate)

## Context

This is the **first Android backend in the whole workspace** — no Apple or Android backend
exists anywhere yet. Platform order (`docs/roadmap.md`, crate `docs/roadmap.md`): Windows → Web
→ Linux → other (Apple, Android). Windows (WMF + D3D11/D3D12 Zero-Copy), Web (WebCodecs), and
Linux (VA-API via `cros-libva`, `adr/linux/0001-vaapi-cros-libva-h264-cpu-upload.md`) already
ship. This ADR is the closest analog to Linux ADR-0001: a minimal-scope, single-codec (H.264),
CPU-upload-only first backend for a new platform, and it deliberately mirrors that ADR's scope
discipline and honesty-about-limitations style.

Per `docs/spec/crate-packaging.md`'s platform suffix table, `android` is a reserved module name
(`mediaway-encoder::android`, `cfg(target_os = "android")`) — no new naming decision is needed,
only the binding choice and scope for this first stage.

**Critical environment constraint (read before trusting any code shape below):** this repo's
current dev environment is a **Windows host with no Android NDK installed**. Unlike Linux
ADR-0001 — which got real `cargo check` / `cargo clippy` / `cargo test` via WSL2 Ubuntu with
real `libva-dev` headers, before any hardware was available — **this ADR has had exactly zero
`cargo check` run against it.** Every API name, method signature, and buffer-lifecycle detail
below is grounded in `docs.rs` / GitHub source reads of the `ndk` crate done during this
research pass, not a real local build. See § Consequences for what that implies and § CI
verification for the plan to close that gap **before** any hardware verification is attempted.

## Research: NDK `AMediaCodec` binding options

Three options were evaluated, mirroring the `deps-policy.md` checklist Linux ADR-0001 used for
`cros-libva`:

1. **Hand-written `bindgen`** in this crate's own `build.rs` against NDK's
   `media/NdkMediaCodec.h` / `NdkMediaFormat.h` / `NdkMediaCrypto.h`, mirroring how `cros-libva`
   itself is built internally.
2. **`ndk-sys`** (rust-mobile, the same crate family already used for raw NDK bindings
   workspace-wide once any Android target exists) with its `media` Cargo feature
   (`ffi/media` → `#[link(name = "mediandk")]`, `bindgen`-generated `extern "C"` bindings for
   the exact headers in option 1). **Raw FFI only** — every `unsafe` call site would still be
   written and owned by this crate.
3. **`ndk`** (rust-mobile) with its `media` feature → `ndk::media::media_codec::MediaCodec` +
   `ndk::media::media_format::MediaFormat`, a **safe** Rust wrapper built on top of option 2.

### Correcting an initial assumption

The task that produced this research pass assumed `ndk`/`ndk-sys` "do NOT currently wrap
`AMediaCodec` media APIs, only surface/window/etc." **That assumption is wrong as of `ndk`
0.9.0 / `ndk-sys` 0.6.0** (verified via `docs.rs` + the crates' own `Cargo.toml` on GitHub,
2026-08-12):

- `ndk-sys`'s `Cargo.toml` has a `media` feature (`ffi/media` in `ndk`'s own manifest) that
  links `libmediandk.so` and generates full bindgen bindings for the NDK media headers
  (`AMediaCodec`, `AMediaFormat`, `AMediaCrypto`, `AImage`/`AImageReader`), not just an opaque
  struct.
- The **safe** `ndk` crate goes further: it ships `ndk::media::media_codec` and
  `ndk::media::media_format` modules with a real `MediaCodec` wrapper —
  `from_encoder_type`/`from_decoder_type`/`from_codec_name`, `configure`, `start`/`stop`/
  `flush`, `dequeue_input_buffer`/`input_buffer`/`queue_input_buffer`,
  `dequeue_output_buffer`/`output_buffer`/`release_output_buffer`, `output_format`,
  `create_input_surface` (API 26+, the future Zero-Copy path), `set_parameters` — none of these
  methods are `unsafe fn`. This is functionally the same shape of "existing safe wrapper over a
  fiddly native session API" that made `cros-libva` the right call for Linux, and the same
  reasoning that made the Windows backend depend on the official `windows` crate instead of
  hand-rolling COM bindings (`adr/windows/0002-windows-crate.md`).

A separate, smaller `mediacodec` (crates.io, maintainer `bayo-code`, MIT) crate also exists as a
higher-level convenience wrapper, but it is a single-maintainer crate with a much shorter track
record than `rust-mobile/ndk` — see § Alternatives Considered.

### `ndk` review (`docs/conventions/deps-policy.md` checklist)

| Question | Answer |
|----------|--------|
| Need | Real: `AMediaCodec`'s state machine (configure → start → per-buffer index dequeue/queue/release → stop) is a large, ordering-sensitive native session API — not a 20–50 line local win, same class of need as VA-API's `Display`/`Config`/`Context`/`Picture`. |
| License | `MIT OR Apache-2.0` — already on `deny.toml`'s allow-list; matches this workspace's own dual license exactly. |
| Transitive | `ndk-sys` (same org, `MIT OR Apache-2.0`); optional `jni`/`jni-sys` only behind `ndk`'s own `test` feature, **not enabled here**. No GPL/LGPL/copyleft surprise. |
| Maintenance | `rust-mobile` org — also maintains `android-activity`, and is the crate `winit`/`wgpu` themselves depend on for Android windowing. Actively released (`ndk` 0.9.0); this dependency is also the same one a future `mediaway::wgpu` Android surface path would need, so it is not a single-purpose add. |
| API stability | `ndk` is pre-1.0 (`0.9.x`) — no full semver guarantee yet, same risk class as `cros-libva 0.0.x`, though `ndk` has a longer history and more downstream consumers. MSRV `1.66`, well under this workspace's `rust-version = "1.91"` pin. |
| Alternatives | See § Alternatives Considered — hand-rolled `bindgen` and raw `ndk-sys` both reinvent buffer-index lifecycle and session-state safety `ndk::media::media_codec` already provides; JNI `android.media.MediaCodec` ties the backend to a live `JNIEnv`/Activity context instead of staying native/JVM-independent. |
| Cost | Build-time coupling to the Android NDK's `libmediandk.so` headers/clang sysroot via `ndk-sys`'s `bindgen` build script — but **any** Android Rust target already requires the NDK toolchain regardless of binding choice, so this is not new cost this ADR introduces. |
| Unsafe surface | All FFI `unsafe` lives in `ndk-sys`/`ndk`; this crate's own code stays on `ndk::media::media_codec`'s safe API. Zero `unsafe` blocks planned in `mediaway-encoder::android` for this stage's CPU-only path (no `NativeWindow`/`AHardwareBuffer` handling needed until the deferred Zero-Copy stage). |

## Decision

> Depend on **`ndk` 0.9** (workspace-pinned through minor) with `features = ["media"]`, as a
> **`[target.'cfg(target_os = "android")'.dependencies]`** entry — mirroring how this crate
> already gates `cros-libva` under `cfg(target_os = "linux")` and `windows` under `cfg(windows)`
> — so `cargo check --workspace` on a non-Android host never invokes `ndk-sys`'s NDK-header
> `bindgen` build script.

- New module `mediaway-encoder::android` (`src/android/`), following the existing
  `src/linux/`, `src/windows/` shape: a thin `AndroidVideoEncoder` (public,
  `#[cfg(target_os = "android")] inner: Option<amediacodec::AmediaCodecVideoEncoder>` /
  `#[cfg(not(target_os = "android"))] _priv: ()`) implementing `VideoEncoder`, wrapping an
  inner `amediacodec` submodule that does the real work — matching `LinuxVideoEncoder` /
  `vaapi::VaapiVideoEncoder`'s split exactly.
- Encoder session: `ndk::media::media_codec::MediaCodec::from_encoder_type("video/avc")`
  (`None` is a real, honest failure — not every AOSP device is guaranteed a given codec, though
  the Android CDD requires at least one AVC encoder — mapped to `EncodeError::Backend`).
  `MediaFormat::new()` + `set_str(KEY_MIME, "video/avc")` + `set_i32` for width/height/
  bitrate/frame-rate/`KEY_I_FRAME_INTERVAL`, then
  `codec.configure(&format, None /* no Surface ⇒ CPU buffer mode */, MediaCodecDirection::Encoder)`,
  `codec.start()`.
- Per-frame CPU path (`VideoInputPreference::CpuUploadOk` only, this stage):
  `dequeue_input_buffer(timeout)` → `input_buffer(index).buffer_mut()` (copy caller's YUV420
  bytes in — a genuine CPU→codec copy, named `upload_cpu_yuv420` to match the Windows/Linux
  `upload_cpu_nv12` cost-disclosure convention) → `queue_input_buffer(...)`. Output:
  `dequeue_output_buffer(timeout)` → `output_buffer(index)` (copy compressed bytes into a
  `Packet`) → `release_output_buffer(..., render: false)`.
- **Own lightweight session-state guard.** Unlike `cros-libva`'s `Picture<S, T>` compile-time
  typestate, `ndk::media_codec::MediaCodec`'s `configure`/`start`/`stop` are plain `&self`
  methods with no type-level ordering enforcement — that safety is not free from the dependency
  here. This crate's own `AmediaCodecVideoEncoder` therefore tracks an internal
  `enum SessionState { Configured, Started, Stopped }` and rejects out-of-order calls with
  `EncodeError::Closed`/`Backend` rather than trusting caller discipline, keeping the same
  "invalid ordering is a caught error, not UB" property Linux gets from the library instead.
- Exact "force every frame to be a sync/key frame" mechanics (Linux ADR-0001 achieves this
  deterministically via raw per-frame SPS/PPS/slice buffers) are **not** pinned to the same
  byte-exact guarantee here: `AMediaCodec` is a higher-level codec black box, and the closest
  lever is `KEY_I_FRAME_INTERVAL` (set low/zero) plus, if needed, a per-frame
  `PARAMETER_KEY_REQUEST_SYNC_FRAME_PERIOD` via `set_parameters`. This is recorded as an
  **open, not-yet-compile-verified detail** (see § Consequences) rather than asserted as fact,
  per `docs/spec/caveats-and-clarity.md` — do not assume parity with Linux's guarantee until
  confirmed against a real build.
- No JNI, no `JNIEnv`, no Activity/JVM context dependency — purely the native `AMediaCodec`
  path, usable from a native/Rust entry point (e.g. `android-activity`-style apps) without a
  Java runtime attached.

## ZCA / typestate shape

Mirrors `LinuxVideoEncoder`'s wrapper shape exactly — no `Box<dyn _>` introduced:

| Linux (VA-API / `cros-libva`) | Android (`AMediaCodec` / `ndk`) | Difference |
|---|---|---|
| `Config` + `Context` (capability vs. bound session) | Single `MediaCodec` handle, configured then started | `AMediaCodec` collapses capability query and session into one object |
| `Picture<S, T>` **typestate** (`PictureNew → Begin → Render → End → Sync`) enforced by the dependency itself | Local `SessionState` enum (`Configured`/`Started`/`Stopped`) enforced **by this crate**, not the dependency | `ndk::media_codec` does not typestate-enforce configure/start/stop ordering — this crate adds that layer itself |
| `Image::create_from` + `vaPutImage` on `Drop` (CPU upload) | `input_buffer(index).buffer_mut()` (borrowed `&mut [MaybeUninit<u8>]`) + explicit `queue_input_buffer` (no upload-on-drop) | Buffer index is dequeued/queued explicitly per frame; `InputBuffer<'_>` borrows the codec rather than owning an uploadable image object |
| `EncCodedBuffer` + `MappedCodedBuffer` segment iteration (output) | `dequeue_output_buffer` → `output_buffer(index)` slice → `release_output_buffer` (output) | Same "dequeue index, read, release index" shape on both input and output sides — `AMediaCodec`'s buffer-index model is symmetric where VA-API's is not |
| `GpuBufferHandle::Vulkan` / DMA-BUF import — **deferred** | `GpuBufferHandle::AndroidSurface` (`AHardwareBuffer*`) / `create_input_surface` + `ANativeWindow` — **deferred**, see § Scope | Both defer Zero-Copy input this stage |

`AndroidVideoEncoder` wraps a concrete `amediacodec::AmediaCodecVideoEncoder` behind `Option`
(closed-after-move sentinel), identical to `WindowsVideoEncoder`/`LinuxVideoEncoder`. No new
heap allocation pattern beyond what `Packet`/`Bytes` already use for compressed output.

## `GpuBufferHandle::AndroidSurface` — already exists

`mediaway-common::GpuBufferHandle::AndroidSurface { buffer: NativeHandle }`
(`crates/mediaway-common/src/gpu.rs`) is **already defined** in the enum — it predates any
Android backend (declared alongside `Metal`/`Vulkan`/`WebGpu` per the wiki's design table,
`docs/ai/wiki/zero-copy/handles.md`). No new `mediaway-common` change is needed to *declare*
the variant; it is simply unused by any real backend until this crate's deferred Zero-Copy
stage wires `AHardwareBuffer*` through it (matching `ndk::media_codec::MediaCodec::
create_input_surface` + `ANativeWindow`, and `ndk::media::image_reader` for the CPU-visible
`AImage`/`AImageReader` side if needed).

## Scope (this stage)

**In:**

- H.264 (`video/avc`) encode only, CPU YUV420 upload only
  (`VideoInputPreference::CpuUploadOk`), no `Surface`/`NativeWindow` at `configure` time.
- One encoder session per `AndroidVideoEncoder::open`; honest `EncodeError::Backend` when
  `from_encoder_type` returns `None` or `configure`/`start` fails, `EncodeError::Unsupported`
  for any other codec/input-path request.
- `mediaway-encoder::android` module only — **not** wired into `mediaway-encoder::auto` /
  `capability` this stage, matching Linux's own current state (`capability.rs`'s doc comment
  only names `mediaway_encoder_windows::auto::support` as wired today). `AndroidVideoEncoder::
  open` is a standalone low-level constructor, same shape as `LinuxVideoEncoder::open`.

**Out (deferred, tracked in `docs/roadmap.md` / crate `docs/roadmap.md`):**

- Zero-Copy `AHardwareBuffer`/`ANativeWindow` input (`VideoInputPreference::ZeroCopyGpu`,
  `GpuBufferHandle::AndroidSurface`) — returns `EncodeError::Unsupported`.
- HEVC / AV1 / VP9 encode (`AMediaCodec` supports them per-device; this crate does not yet).
- CBR/VBR rate control, explicit GOP/P-frame structure beyond `KEY_I_FRAME_INTERVAL`.
- Wiring into `mediaway-encoder::auto` / `capability::support` (`Backend::Os` Android
  resolution).
- Vulkan Video encode on Android (some devices expose it) — left for its own future ADR, same
  as Linux's Vulkan Video alternative is still open.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Hand-written `bindgen` FFI in this crate against NDK media headers directly | Reinvents buffer-index lifecycle and session safety `ndk::media::media_codec` already provides safely; larger unsafe surface owned directly by this crate instead of an upstream-maintained wrapper — same reasoning Linux ADR-0001 used against hand-rolled VA-API bindings. |
| `ndk-sys` raw FFI directly (skip the safe `ndk` layer) | Same unsafe-ownership problem as above; `ndk` is a thin, actively maintained safe layer over exactly these bindings with no meaningful downside to using it instead. |
| JNI `android.media.MediaCodec` via `jni` / `jni-android-sys` | Requires a live `JNIEnv` / JVM attach and ties the backend to an Activity/JVM context; heavier unsafe surface (JNI method-ID lookups) for equivalent functionality. NDK's native `AMediaCodec` is the JVM-independent path and the one Google documents for native/game-engine-style apps. |
| `mediacodec` (crates.io, `bayo-code`, MIT) convenience crate | Single-maintainer, materially shorter track record than `rust-mobile/ndk` (the foundational crate behind `android-activity`/`winit`/`wgpu`'s own Android support); would add a second, less-established dependency for functionality `ndk` already covers directly. |
| Vulkan Video encode on Android | Out of scope for "first Android backend, mirror Linux's minimal CPU path"; device/driver Vulkan Video maturity on Android is unverified in this workspace (mirrors the already-known AV1-on-Vulkan driver-maturity gap elsewhere), and would need its own ADR. |
| Depend on system `ffmpeg`/`libavcodec` MediaCodec wrapper | Forbidden — FFmpeg stays a test/dev oracle only (ADR-0002), never a product dependency. |

## ⚠️ CI verification plan — weaker starting point than every prior backend

**This ADR, as authored, has had zero `cargo check`, `cargo clippy`, or `cargo test` run
against it — not even a compile check** — because this repository's current dev environment
(Windows host, no Android NDK installed) cannot target `android` at all, unlike Linux ADR-0001
which got real WSL2 `cargo check`/`clippy`/`test` with real `libva-dev` headers before any
hardware existed. This is a strictly weaker starting point and must be closed **before** any
implementation PR is trusted, let alone before hardware verification is attempted.

**Recommended CI job** (new job in `.github/workflows/ci.yml`, gated the same way the existing
`wasm` job is — `needs: [affected]`, only runs when `mediaway-encoder` or `mediaway-common` is
in the affected set):

1. `runs-on: ubuntu-latest` (NDK cross-compilation works the same on any CI host OS; Linux
   runners are cheapest and match this workspace's existing non-Windows job).
2. `dtolnay/rust-toolchain@stable` + `rustup target add aarch64-linux-android` (primary
   real-device target; `armeabi-v7a`/`x86_64` left as a follow-up decision, see § Open
   questions).
3. **`nttld/setup-ndk@v1`** with a pinned `ndk-version` (a specific NDK release, not `latest`,
   for reproducibility — exact version to be chosen with the user, see § Open questions) to
   install the NDK and export `ANDROID_NDK_HOME`.
4. **`cargo-ndk`** (`bbqsrc/cargo-ndk`, prebuilt via `taiki-e/install-action` if it publishes a
   binary, else `cargo install cargo-ndk`) to resolve the NDK's per-ABI clang/linker paths —
   recommended over hand-wiring `CC_aarch64-linux-android` / `CARGO_TARGET_..._LINKER` env vars
   directly, since manual NDK linker wiring is a well-known source of breakage across NDK
   releases (confirmed via `rust-lang/cargo#7611` and multiple community write-ups during this
   research pass).
5. `cargo ndk -t arm64-v8a check -p mediaway-encoder --all-features` (compile-check), then
   `cargo ndk -t arm64-v8a clippy -p mediaway-encoder --all-features -- -D warnings`
   (lint-check), matching this workspace's existing clippy gate shape.
6. **No test execution** — no Android emulator/device runner in this CI job. This mirrors the
   existing `wasm` job in `ci.yml`, which is already an accepted **compile-only** verification
   pattern in this workspace for a target the dev environment cannot execute
   (`cargo build --target wasm32-unknown-unknown`, no wasm runtime test either) — so this
   proposal is not a novel risk shape for the workspace, just a second instance of one.

Only once this job exists and is green does "compile-verified" become true for this backend —
matching the bar Linux ADR-0001 had **before** its own zero-hardware caveat, not the bar it had
after.

## Decisions confirmed with the user (2026-08-12)

1. **CI job**: added in the same PR as the implementation (not a follow-up) — see § CI
   verification plan, now implemented in `.github/workflows/ci.yml`'s `android` job.
2. **`minSdkVersion` floor: API 21** (Android 5.0, the release `AMediaCodec` itself was
   introduced in) — maximizes device compatibility for this CPU-upload-only stage. The
   deferred Zero-Copy `create_input_surface` stage will need to gate on API 26+ separately
   (`ndk`'s `api-level-26` feature), a forward-compatible bump, not a blocker now. Reflected in
   CI as `cargo ndk -t arm64-v8a -p 21 …`.
3. **CI ABI scope: `arm64-v8a` only** — matches real modern Android devices and this ADR's
   "small, real first backend" discipline (same reasoning Linux ADR-0001 used to stay
   H.264-only). `armeabi-v7a`/`x86_64` are a follow-up if/when needed (e.g. emulator testing).
4. **`mediaway-encoder::auto`/`capability` wiring: deferred**, matching Linux's own current
   unwired state — `AndroidVideoEncoder::open` stays a standalone low-level constructor this
   stage (already stated in § Scope, confirmed rather than revisited).
5. **NDK version pin: r27c** — a real, released LTS-track NDK version, chosen for
   reproducibility per § CI verification plan; bump deliberately (not to `latest`) when a
   concrete reason exists (e.g. a newer required API-level feature).

## Consequences

### Positive

- Small, real unsafe surface (none written directly in this crate; all FFI unsafety lives in
  `ndk`/`ndk-sys`), matching the Linux/Windows precedent.
- `ndk` is a dependency this workspace will likely need again for any future Android windowing
  / `mediaway::wgpu` Android surface work — not a single-purpose add.
- `GpuBufferHandle::AndroidSurface` already existing in `mediaway-common` means the deferred
  Zero-Copy stage has no blocking type-design work left, only wiring.
- CI verification model (`nttld/setup-ndk` + `cargo-ndk`, compile-only) has a direct existing
  precedent in this workspace's `wasm` CI job.

### Negative / Trade-offs

- **Zero compile verification as authored** (see § CI verification plan) — every method name,
  `MediaFormat` key constant, and buffer-lifecycle claim above is a research-pass read of
  `docs.rs`/GitHub source, not a real local build. Treat all of it as unverified until the CI
  job goes green.
- `ndk = "0.9"` is pre-1.0 — same semver-risk class as `cros-libva 0.0.x`, though with a longer
  track record and more downstream consumers.
- The "force every frame to be a sync frame" mechanism is an approximation
  (`KEY_I_FRAME_INTERVAL` / possible per-frame `set_parameters`), not the same deterministic
  guarantee Linux ADR-0001 achieves via raw VA-API buffers — open until confirmed against a real
  build (see § Open questions in the accompanying response, not duplicated here).
- No real Android hardware or emulator has been touched at all — this is a strictly earlier
  milestone than Linux's "compile-verified, zero hardware" state, not the same one.
- Build-time hard dependency on the Android NDK toolchain for any `target_os = "android"` build
  of this crate — acceptable per this crate's own platform scope (never required for
  Windows/Web/Linux/other builds, per the `cfg(target_os = "android")` gate), and unavoidable
  for *any* Android Rust target regardless of binding choice.

## References

- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
- [`docs/spec/crate-packaging.md`](../../../../docs/spec/crate-packaging.md) — `android` platform
  suffix (already reserved)
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) — honesty
  requirement this ADR follows for its own unverified-detail admissions
- `mediaway-encoder` [ADR-0021](../../../../docs/adr/0021-workspace-consolidation.md) — `#[cfg]`-gated
  backend modules, no separate platform crate
- `adr/linux/0001-vaapi-cros-libva-h264-cpu-upload.md` — the direct scope/honesty precedent this
  ADR mirrors
- `adr/amf/0001-amf-deferred-no-hardware.md` — precedent for recording unverified/deferred
  research honestly without overstating capability
- `adr/windows/0002-windows-crate.md` — precedent for depending on an official/community binding
  over hand-rolled FFI
- [`ndk` on crates.io](https://crates.io/crates/ndk) ·
  [GitHub (`rust-mobile/ndk`)](https://github.com/rust-mobile/ndk) (`MIT OR Apache-2.0`)
- [`ndk::media::media_codec` docs.rs](https://docs.rs/ndk/latest/ndk/media/media_codec/index.html)
- [`nttld/setup-ndk`](https://github.com/nttld/setup-ndk) ·
  [`cargo-ndk` (`bbqsrc/cargo-ndk`)](https://github.com/bbqsrc/cargo-ndk)
- `docs/roadmap.md` § platform order (Windows → Web → Linux → other) · crate
  `docs/roadmap.md` § Stage 4 — Other
