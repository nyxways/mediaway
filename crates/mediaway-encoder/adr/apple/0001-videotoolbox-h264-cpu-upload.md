# ADR-0001: `VideoToolbox` `VTCompressionSession` via `objc2`, H.264 CPU-upload encode

- **Status**: Accepted (2026-08-12) — see § Decisions confirmed with the user
- **Date**: 2026-08-12
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (module `mediaway-encoder::apple`, ADR-0021 `#[cfg]`-gated
  backend — no separate `mediaway-encoder-apple` crate)

## Context

This is the **last "Other" platform backend** (`docs/roadmap.md` Stage 4), alongside the
just-implemented Android backend. Platform order: Windows → Web → Linux → other (Apple,
Android). Per `docs/spec/crate-packaging.md`'s platform suffix table, `apple` is a single
reserved module name covering **both** macOS and iOS — "split further only with an ADR". This
ADR evaluates whether that holds at Stage 1 (see § Single-module justification below) instead of
asserting it.

**Critical environment constraint, and how this research pass differs from Android's:** this
repo's dev environment (Windows host) cannot compile-check Apple code — Apple SDK
headers/frameworks cannot be legally cross-compiled outside macOS/Xcode, a *harder* wall than
Android's "just missing the NDK" gap. Per explicit user direction, this ADR is grounded entirely
in a locally cloned copy of [`objc2`](https://github.com/madsmtm/objc2) (`local/vendor-ref/objc2/`,
`generated` submodule initialized) — the de facto standard modern Rust Apple framework bindings
(used by `winit`/`bevy`) — not web-fetched summaries. Every API name, signature, constant, and
feature-flag claim below is a direct read of that checkout's `generated/VideoToolbox/`,
`generated/CoreVideo/`, `generated/CoreMedia/`, and the four `framework-crates/objc2-*/Cargo.toml`
files, done during this pass. Where the local source does not settle a question, this ADR says so
explicitly (§ Open questions) rather than filling the gap from general/web knowledge, per the
task's explicit constraint.

## Research: binding options

Only one realistic option exists at the "safe-ish Rust over the native session API" tier this
crate's other backends use (`ndk` for Android, `cros-libva` for Linux, `windows` for Windows):

1. **Hand-written `bindgen`/raw FFI** directly against the VideoToolbox/CoreMedia/CoreVideo C
   headers in this crate's own `build.rs`.
2. **`objc2-video-toolbox` + `objc2-core-video` + `objc2-core-media` + `objc2-core-foundation`**
   (the `objc2` project's `header-translator`-generated per-framework crates) — confirmed real via
   local read, not raw `bindgen` output but structured `unsafe fn` wrappers with typed
   `CFRetained<T>` smart pointers, `bitflags`-backed option/info-flag types, and doc-derived
   `# Safety` sections already written by the generator.

### A finding that simplifies the dependency graph vs. a naive plan

VideoToolbox, CoreMedia, and CoreVideo are **plain C APIs**, not Objective-C class hierarchies —
confirmed by reading the generated files: `VTCompressionSession`/`CVPixelBuffer`/`CMSampleBuffer`
are `#[repr(C)]` opaque Core Foundation types (`cf_type!`/`ConcreteType` + `CFRetained<T>`
ref-counting), and every function (`VTCompressionSessionCreate`, `CVPixelBufferCreateWithBytes`,
`CMSampleBufferGetDataBuffer`, …) is a plain `extern "C-unwind"` C call — **not** an
`objc2::msg_send!` trampoline. The `"objc2"` Cargo feature on each of these framework crates
exists only to add *optional* Cocoa/`NSObject` bridging (needed for AVFoundation-style Obj-C
classes, not for this surface). This crate's Stage-1 scope therefore needs **none** of
`objc2`/`objc2-foundation`/`block2` as dependencies — a smaller unsafe/dependency surface than a
naive "add the whole objc2 ecosystem" plan would produce, and notably smaller than Android's
`ndk` (which is one integrated safe wrapper crate; here we compose four thin C-API crates
ourselves).

### `objc2-*` review (`docs/conventions/deps-policy.md` checklist)

| Question | Answer |
|----------|--------|
| Need | Real: `VTCompressionSession`'s C API (session create → `VTSessionSetProperty` config → `VTCompressionSessionEncodeFrame` → async callback → `CompleteFrames`/`Invalidate`) is the only supported HW H.264 encode path on Apple platforms — not a 20–50 line local win. |
| License | `Zlib OR Apache-2.0 OR MIT` (confirmed: `local/vendor-ref/objc2/Cargo.toml` `[workspace.package] license`, applies uniformly to all four framework crates read). All three terms are already on `deny.toml`'s allow-list (`MIT`, `Apache-2.0`, `Zlib` rows) — no new allow-list entry needed. |
| Transitive | `objc2-core-foundation` has no required deps beyond `std`/`alloc` at default-features-off; `bitflags`/`block2`/`libc` are all `optional`. No GPL/LGPL/copyleft surprise in anything read. |
| Maintenance | `madsmtm/objc2` — the foundational crate `winit`, `bevy`, and most of the Rust-on-Apple ecosystem depend on for framework bindings; actively released, `header-translator`-regenerated against current Apple SDKs. |
| API stability | Workspace-shared version in this checkout: **0.3.2** (`local/vendor-ref/objc2/Cargo.toml` `[workspace.package] version`) — pre-1.0, same semver-risk class as `ndk 0.9`/`cros-libva 0.0.x`. MSRV `1.71` per that same file, well under this workspace's `1.91` pin. Confirm the exact currently-published crates.io version at implementation time (do not assume `0.3.2` is still current then); pin via workspace minor as `"0.3"`. |
| Alternatives | See § Alternatives Considered. |
| Cost | No build-time codegen/bindgen step on the crate's own side — `objc2-*` ship pre-generated bindings, unlike `ndk-sys`'s NDK-header `bindgen` build script. Build-time cost is only "must be on an Apple SDK toolchain", unavoidable for any Apple target regardless of binding choice. |
| Unsafe surface | Every `objc2-*` call is `unsafe fn` (raw C API, no safe wrapper layer) — **unlike Android's `ndk::media::media_codec` (safe) or Linux's `cros-libva` (safe-ish typestate)**, this crate's own code will contain real, non-trivial `unsafe` blocks with `// SAFETY:` comments, the same discipline this crate already uses in `src/windows/` (cited: `d3d12_share.rs`, `d3d12_video_encode.rs`). `#![allow(unsafe_code)]` (already this crate's root default) applies to the new `apple` module — it cannot mirror Android/Linux's `#![forbid(unsafe_code)]` module-level stance. |

## Decision

> Depend on **`objc2-video-toolbox`, `objc2-core-video`, `objc2-core-media`,
> `objc2-core-foundation`, version `"0.3"`** (workspace-pinned through minor), all
> `default-features = false` with only the specific per-framework sub-features this crate's
> Stage-1 surface needs, as a **`[target.'cfg(any(target_os = "macos", target_os = "ios"))'.dependencies]`**
> entry — Cargo target-cfg requires `any(...)` here because `target_os = "macos"` and
> `target_os = "ios"` are **distinct** cfg values (unlike Android's single `target_os = "android"`
> gate already used for `ndk`), so a bare `cfg(target_os = "macos")` would silently exclude iOS
> from ever building this dependency.

Confirmed-real feature names to enable (read directly from each crate's own `Cargo.toml`
optional-dependency feature lists, not invented):

- `objc2-video-toolbox`: `"VTCompressionSession"`, `"VTCompressionProperties"`, `"VTErrors"`,
  `"VTSession"`, `"objc2-core-media"`, `"objc2-core-video"` (the latter two are this crate's own
  feature flags that transitively pull `objc2-core-media`/`objc2-core-video` with a **fixed**
  sub-feature set VideoToolbox itself declares: `CMBase`/`CMFormatDescription`/`CMSampleBuffer`/
  `CMTaggedBufferGroup`/`CMTime`/`CMTimeRange` and `CVBuffer`/`CVImageBuffer`/`CVPixelBuffer`/
  `CVPixelBufferPool` respectively).
- `objc2-core-media` (direct dependency, **beyond** what `objc2-video-toolbox` auto-enables):
  add `"CMBlockBuffer"` explicitly — VideoToolbox's own feature list does **not** include it, but
  this crate needs `CMBlockBuffer::copy_data_bytes`/`data_pointer`/`data_length` to read the
  encoded payload out of `CMSampleBufferGetDataBuffer`'s result. Cargo feature unification across
  the shared dependency makes this additive, not a fork.
- `objc2-core-video`: no additive features beyond what `objc2-video-toolbox` pulls in — `CVBuffer`/
  `CVImageBuffer`/`CVPixelBuffer`/`CVPixelBufferPool` already cover `CVPixelBuffer::with_planar_bytes`.
- `objc2-core-foundation`: `"CFString"`, `"CFDictionary"`, `"CFArray"`, `"CFNumber"` (property
  keys/values, encoder-specification dictionaries). Whether a distinct `"CFBoolean"` feature name
  exists, or boolean `CFType` values are reachable without a separate gate, is **not settled by
  local grounding** — flagged in § Open questions rather than guessed.

New module `mediaway-encoder::apple` (`src/apple/`), following the `src/android/`,
`src/linux/`, `src/windows/` shape: a thin `AppleVideoEncoder` (public,
`#[cfg(any(target_os = "macos", target_os = "ios"))] inner: Option<videotoolbox::VideoToolboxVideoEncoder>`
/ non-Apple stub) implementing `VideoEncoder`, wrapping an inner `videotoolbox` submodule.

### Session lifecycle

- `VTCompressionSession::new(...)` (`VTCompressionSessionCreate`) with `codec_type =
  kCMVideoCodecType_H264`, `encoder_specification = None` (let VideoToolbox pick), and an
  `output_callback`/`output_callback_ref_con` pair (see § Callback design) — confirmed real
  signature in `generated/VideoToolbox/VTCompressionSession.rs`.
- Config via `VTSessionSetProperty(session, key, value)` (confirmed generic setter in
  `VTSession.rs`) with confirmed-real keys from `VTCompressionProperties.rs`:
  `kVTCompressionPropertyKey_MaxKeyFrameInterval = 1` (every frame a forced keyframe — the doc
  comment's own language, "requiring a keyframe every X frames", reads as a **harder** contract
  than Android's device-dependent `KEY_I_FRAME_INTERVAL = 0` best-effort lever, though this is
  still an inference from the doc comment, not hardware-verified this stage),
  `kVTCompressionPropertyKey_AllowFrameReordering = false` (no B-frames, matches "every frame
  effectively independent" scope), `kVTCompressionPropertyKey_AverageBitRate`,
  `kVTCompressionPropertyKey_ExpectedFrameRate`, `kVTCompressionPropertyKey_RealTime = true`,
  `kVTCompressionPropertyKey_ProfileLevel` (Baseline-class profile, matching Linux/Android's
  Baseline-first-stage choice — exact constant name deferred to implementation).
- Per-frame: `VTCompressionSession::encode_frame(image_buffer, pts, duration, ...,
  source_frame_refcon, ...)` (`VTCompressionSessionEncodeFrame`, confirmed signature).
- Flush: `VTCompressionSession::complete_frames(kCMTimeIndefinite)`
  (`VTCompressionSessionCompleteFrames`, confirmed: "all pending frames will be emitted before
  the function returns" per its own doc comment — a genuinely stronger synchronous drain
  guarantee than Android's opportunistic best-effort drain loop).
- Teardown: `VTCompressionSession::invalidate()` then drop the `CFRetained<VTCompressionSession>`.

## CPU-upload input strategy — `CVPixelBufferCreateWithPlanarBytes`, not `CreateWithBytes`

**A real deviation from a naive plan, found only by reading the actual function signatures:**
`CVPixelBufferCreateWithBytes` (`CVPixelBuffer::with_bytes`) takes a **single** `base_address` +
`bytes_per_row` — correct for packed formats (BGRA), but `PixelFormat::Nv12` /
`kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange`/`...FullRange` (both confirmed real constants
in `generated/CoreVideo/CVPixelBuffer.rs`) is **semi-planar**: one 8-bit luma plane + one
interleaved-chroma plane, each with its own stride. Only `CVPixelBufferCreateWithPlanarBytes`
(`CVPixelBuffer::with_planar_bytes`, confirmed signature: `number_of_planes`,
`plane_base_address`/`plane_width`/`plane_height`/`plane_bytes_per_row` **arrays**) can express
that layout. `CreateWithBytes` would silently misinterpret NV12 bytes as a packed single-plane
format if used here — a wrong-function bug a naive "grab the first `CreateWith*` result" pass
could easily have shipped uncaught until real hardware output looked corrupted.

Decision: **`upload_cpu_nv12`** (matching this crate's existing WMF `upload_cpu_nv12` cost-name,
per the caveats catalog — no Android-style renaming needed since Apple's pixel format is
literally NV12, not Android's YUV420-semi-planar naming ambiguity):

1. Copy the caller's `VideoFrame::storage: VideoFrameStorage::Cpu { data }` bytes into a
   heap-owned `Box<[u8]>` — the one real, named `memcpy` this path performs.
2. Call `CVPixelBufferCreateWithPlanarBytes` with `plane_base_address` pointing at that box's Y
   and UV plane offsets, `release_callback` = a `Box::from_raw`-based reclaim, `release_ref_con` =
   `Box::into_raw(owned_box) as *mut c_void` — the box's lifetime is handed to the `CVPixelBuffer`
   itself; VideoToolbox releases it exactly once, whenever the pixel buffer's own retain count
   hits zero (confirmed real callback shape: `CVPixelBufferReleasePlanarBytesCallback`).
3. **No second copy into a VT-owned pool.** The alternative — `VTCompressionSessionGetPixelBufferPool`
   + memcpy into a pool buffer — was rejected: `VTCompressionSessionCreate`'s own doc comment
   (`generated/VideoToolbox/VTCompressionSession.rs`) states "Using pixel buffers not allocated
   by the Video Toolbox **may increase the chance that it will be necessary to copy image data**"
   — i.e. even the pool path is not a guaranteed avoidance of an internal copy, so the extra
   pool-memcpy this crate would add has no proven payoff, and the direct-wrap path keeps exactly
   one named, documented copy (matching the "document costly paths" rule) instead of a possible
   two.

## Callback / output-collection design

`VTCompressionOutputCallback` (confirmed type:
`unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, OSStatus, VTEncodeInfoFlags, *mut CMSampleBuffer)`)
fires **asynchronously on a VideoToolbox-internal thread**, decoupled from the
`push_frame`/`encode_frame` call that produced it — a stronger asymmetry than Android's
opportunistic-drain `dequeue_output_buffer` (which is at least caller-pollable on the caller's own
thread).

Shape: `Arc<Mutex<VecDeque<Packet>>>` shared between the session struct and the callback:

- At `open()`: build `shared: Arc<Mutex<VecDeque<Packet>>>`; keep one clone in
  `VideoToolboxVideoEncoder { shared, session, .. }` (used by `poll_packet`); pass
  `Arc::into_raw(shared.clone())` as `output_callback_ref_con` to `VTCompressionSessionCreate` —
  this deliberately holds one "extra" strong count that only this struct's `Drop` reclaims.
- Inside the `extern "C-unwind"` callback: reconstruct a **borrow**, not an owning value —
  `let shared = unsafe { &*(ref_con.cast::<Mutex<VecDeque<Packet>>>()) };` (never
  `Arc::from_raw` inside the callback itself, which would wrongly decrement/free on every
  invocation) — extract the `CMSampleBuffer`'s payload (§ below) and `push_back` a `Packet`.
- In `Drop`: call `session.invalidate()`, **then** `drop(unsafe { Arc::from_raw(ref_con_ptr) })`
  to release the extra strong count and let the allocation free once both the struct's own clone
  and this reclaimed one are gone.

**Open question (not settled by local source, flagged rather than assumed):** this design
depends on `VTCompressionSessionInvalidate` guaranteeing no further callback invocation once it
returns. The doc comment read in `VTCompressionSession.rs` only says invalidate "ensures a
deterministic, orderly teardown" — it does not explicitly state "no callback fires after this
returns". Calling `complete_frames(kCMTimeIndefinite)` before `invalidate()` (already this
backend's `flush()` behavior) should drain all in-flight callbacks first per that function's own
"all pending frames will be emitted before the function returns" doc guarantee, which narrows the
race to "did the caller call `flush()` before drop" — but this exact ordering contract should be
confirmed against Apple's official long-form developer documentation before Accepted status (not
via WebFetch per this task's constraint — the user or a future pass with real Apple docs access
should confirm). See § Open questions.

## Extradata (SPS/PPS) capture — **in scope this stage**, unlike Android's deferral

Explicit scope call, not left ambiguous: **capture it**. Unlike Android's `AMediaCodec`, where
`csd-0`/`csd-1` arrive as a separate `BUFFER_FLAG_CODEC_CONFIG` output-buffer event requiring
extra dequeue-loop branching (deferred there for that reason), VideoToolbox's parameter sets are
reachable **synchronously off the same `CMSampleBuffer`** as the first successfully encoded
frame: `sample_buffer.format_description()` → `CMFormatDescription::h264_parameter_set_at_index(0
/* SPS */)` and `(1 /* PPS */)` (confirmed real signature in `generated/CoreMedia/CMFormatDescription.rs`).
No extra polling branch, no separate event type — cheap enough per `docs/spec/caveats-and-clarity.md`'s
"document/avoid silent slow defaults" spirit to just do it. `StreamInfo::Video::extra_data`'s
established convention in this crate is **avcC** (confirmed: `src/windows/wmf/video.rs` converts
WMF's Annex-B `MF_MT_MPEG_SEQUENCE_HEADER` via `iso_bmff::bitstream::avc::to_avcc`). Apple's
`h264_parameter_set_at_index` returns raw NAL payload bytes with no start code and no container
framing, so this backend will synthesize a temporary Annex-B buffer (`00 00 00 01` + SPS bytes +
`00 00 00 01` + PPS bytes) on the first callback invocation and reuse the **existing**
`iso_bmff::bitstream::avc::to_avcc` helper unchanged — reuses an already-workspace dependency
instead of writing a new avcC builder, per `deps-policy.md`'s "can existing code cover it" check.

## ZCA / typestate shape

| Android (`AMediaCodec` / `ndk`) | Apple (`VTCompressionSession` / `objc2-*`) | Difference |
|---|---|---|
| Single `MediaCodec` handle, `SessionState` enum owned by this crate | Single `CFRetained<VTCompressionSession>`, no extra state enum needed — `VTCompressionSessionEncodeFrame` after `invalidate()` simply returns `kVTInvalidSessionErr` (a real, catchable `OSStatus`, confirmed constant in `VTErrors.rs`) | VideoToolbox's own error surface already distinguishes "invalid session" as a typed error; a `flushed: bool`-style local guard is still useful to short-circuit before the FFI call, but not load-bearing for safety the way Android's was |
| Buffer-index dequeue/queue (input), opportunistic `dequeue_output_buffer` drain (output) | Direct `encode_frame` call (input, synchronous submit); output arrives via callback into `Arc<Mutex<VecDeque<Packet>>>`, `poll_packet` just pops the front | Output collection is push-based (VT pushes into our queue) vs. Android's pull-based opportunistic drain |
| `GpuBufferHandle::AndroidSurface` — deferred | `GpuBufferHandle::Metal` (`CVPixelBuffer`/`IOSurface` token) — **already exists**, deferred the same way (§ below) | Same "type exists, wiring deferred" shape both platforms share |

No `Box<dyn _>` introduced. `AppleVideoEncoder` wraps a concrete
`videotoolbox::VideoToolboxVideoEncoder` behind `Option` (closed-after-move sentinel), identical
to every other platform wrapper in this crate.

## `GpuBufferHandle::Metal` — already exists

`mediaway-common::GpuBufferHandle::Metal { buffer: NativeHandle }` (`CVPixelBuffer`/`IOSurface`
token) and `GpuDeviceHandle::Metal(NativeHandle)` (`MTLDevice`) are **already defined**
(`crates/mediaway-common/src/gpu.rs`, confirmed via read, predates any Apple backend — same
situation Android found for `GpuBufferHandle::AndroidSurface`). No `mediaway-common` change
needed to declare the variant; wiring `VTCompressionSessionGetPixelBufferPool`-sourced (or
externally supplied) `CVPixelBuffer`s through it is deferred Zero-Copy work.

## Single-module (`apple`) justification, not split into `macos`/`ios`

Grounded, not asserted: every function/type this ADR cites (`VTCompressionSession`,
`CVPixelBuffer`/`CVPixelBufferPool`, `CMSampleBuffer`/`CMBlockBuffer`/`CMFormatDescription`) is
declared in the generated files with **no** `#[cfg(target_os = "macos")]` / `"ios"` split at the
item level, and `objc2-video-toolbox`'s own `Cargo.toml` lists `aarch64-apple-darwin` **and**
`aarch64-apple-ios`/`-tvos`/`-visionos`/`-ios-macabi` together under one `docs.rs` target list —
VideoToolbox has been a unified cross-Apple-platform framework since it shipped on both OSes.
The **one** platform-conditional split found in the dependency graph
(`objc2-core-video/Cargo.toml`'s `[target.'cfg(target_os = "macos")'.dependencies]
objc2-open-gl`) is for legacy `CVOpenGLBuffer*` interop this crate does not use. **Confidently**
single `apple` module for this stage's API surface — the caveat is *runtime device/session
availability* (e.g. background-app HW-encoder eligibility on iOS), a behavioral difference, not
an API-shape one, deferred to hardware verification.

## Scope (this stage)

**In:**

- H.264 encode only, CPU NV12 upload only (`VideoInputPreference::CpuUploadOk`), no
  `CVPixelBufferPool`/Zero-Copy input.
- SPS/PPS `extra_data` capture (avcC), via `iso_bmff::bitstream::avc::to_avcc` reuse (§ above) —
  **in scope**, deviating from Android's deferral for the reasons given.
- `mediaway-encoder::apple` module only — **not** wired into `auto`/`capability`, matching
  Android/Linux's current unwired state.

**Out (deferred):**

- Zero-Copy `CVPixelBuffer`/`IOSurface` input (`GpuBufferHandle::Metal`) — returns
  `EncodeError::Unsupported`.
- HEVC/AV1/ProRes encode (VideoToolbox supports them; this crate does not yet).
- CBR/VBR rate control beyond `kVTCompressionPropertyKey_AverageBitRate`, multi-pass encode.
- `mediaway-encoder::auto`/`capability` wiring (`Backend::Os` Apple resolution).
- Real hardware/simulator test execution beyond CI compile+clippy (see § CI verification plan).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Hand-written `bindgen`/raw FFI against VideoToolbox/CoreMedia/CoreVideo headers | Reinvents `CFRetained` ref-counting, `OSStatus`/`CVReturn` error typing, and struct layouts `objc2-*` already provides correctly (`header-translator`-verified against real Apple SDKs) — larger unsafe surface owned directly by this crate, same reasoning every prior backend's ADR used against hand-rolled FFI. |
| `core-foundation`/`core-video-sys`/`core-media-sys` (older, pre-`objc2` community crates) | Materially less actively maintained than `madsmtm/objc2`, which is the crate the broader Rust-on-Apple ecosystem (`winit`, `bevy`) has consolidated on; would add a second, less-established binding lineage. |
| Full `objc2`/`objc2-foundation`/`block2` dependency (naive "add the whole ecosystem" plan) | Unnecessary — confirmed this crate's Stage-1 surface (C-API-shaped VideoToolbox/CoreMedia/CoreVideo calls) needs none of the Cocoa/Obj-C-class bridging those crates add; smaller dependency graph without them. |
| `VTCompressionSessionEncodeFrameWithOutputHandler` (block-based, not C-function-pointer callback) | Requires `block2` (Obj-C blocks ABI) as an added dependency for no scope benefit at this stage — the plain `VTCompressionOutputCallback` C-function-pointer form already covers this crate's async-output design without it; may be revisited if a later stage wants Rust closures instead of a raw refcon. |

## ⚠️ CI verification plan

Same starting point as Android — **zero compile verification as authored**, for a *stronger*
structural reason (this platform cannot even be cross-compiled outside Apple hardware/tooling,
unlike Android's "just missing the NDK"). Unlike Android, though, GitHub-hosted `macos-*` runners
are **real Apple Silicon hardware** with Xcode preinstalled — this gives Stage-1 CI a
meaningfully stronger available bar than Android's cross-compile-only story, if the user wants it.

**Proposed jobs** (new jobs in `.github/workflows/ci.yml`, gated like the existing `wasm`/`android`
jobs — `needs: [affected]`, only when `mediaway-encoder`/`mediaway-common` is in the affected set):

1. **`apple-macos`**: `runs-on: macos-14` (a **pinned** runner image, not `macos-latest` — mirrors
   the `ndk-version: r27c` pin philosophy against silent OS/Xcode drift). Native target (no
   cross-compile) — `cargo clippy -p mediaway-encoder --features full --all-targets -- -D
   warnings`. Because this is a *native* Apple-Silicon runner (unlike Android's cross-compiled
   `ubuntu-latest` job), this job could additionally run `cargo test -p mediaway-encoder
   --features full` for real — see § Open questions on whether Stage 1 should include that.
2. **`apple-ios`** (compile-only, cross-compiled from the same or a second `macos-14` runner):
   `rustup target add aarch64-apple-ios`; `cargo clippy --target aarch64-apple-ios -p
   mediaway-encoder --features full --all-targets -- -D warnings`. No simulator boot/run —
   mirrors Android's "no emulator" scope discipline, even though `xcrun simctl` *is* available on
   these runners if a later stage wants it.

Only once these jobs are green does "compile-verified" become true for this backend.

## Open questions for the user (before implementation / Accepted status)

1. **`VTCompressionSessionInvalidate` / callback-cutoff ordering** (§ Callback design) — confirm
   against Apple's official developer documentation (not fetched in this pass per the task's
   local-source-only constraint) whether invalidate() guarantees no further callback invocation
   after it returns, or whether the `Arc`-shared design's safety margin needs strengthening
   further (e.g. requiring `flush()` before `Drop`, enforced at the type level).
2. **`CFBoolean` reachability** — `objc2-core-foundation`'s `Cargo.toml` feature list (read in
   full) does not show a standalone `"CFBoolean"` feature name the way `"CFNumber"`/`"CFString"`
   are listed; whether boolean property values (`kVTCompressionPropertyKey_AllowFrameReordering`,
   `_RealTime`) are reachable under an existing enabled feature or need one not yet identified is
   unresolved from local grounding alone.
3. **Color range**: `kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange` vs. `...FullRange` — both
   are real, confirmed constants; `mediaway-common::PixelFormat::Nv12` carries no range field
   today, so this crate must pick one. Proposal: `VideoRange` (matches typical camera/broadcast
   H.264 convention), but this is a real product decision, not purely a grounding question.
4. **CI scope**: should the `apple-macos` job (real Apple Silicon runner) include an actual
   `cargo test` run in the same PR, given real hardware is available unlike every other backend's
   CI-only-compiles-it story? Or should Stage 1 stay compile+clippy-only for parity with how
   Android/Linux/Windows first landed, with hardware `cargo test` deferred to a later "hardware
   verified" milestone the same way those did?
5. **`kVTCompressionPropertyKey_ProfileLevel` exact constant** — deferred to implementation;
   confirm the Baseline-class constant name against `VTCompressionProperties.rs` in full before
   writing code (only a subset of that file was read for the property **keys** needed here, not
   every profile/level constant it defines).

## Decisions confirmed with the user (2026-08-12)

1. **`CFBoolean` reachability — resolved by local re-grounding, not asked**: `CFBoolean` /
   `kCFBooleanTrue` / `kCFBooleanFalse` live under the existing `"CFNumber"` module/feature
   (`generated/CoreFoundation/CFNumber.rs`, re-exported from `CoreFoundation/mod.rs`) — no
   separate `"CFBoolean"` feature name exists or is needed; `"CFNumber"` (already listed in the
   Decision section) already covers it.
2. **`kVTCompressionPropertyKey_ProfileLevel` exact constant — resolved by local re-grounding**:
   `kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel` (confirmed real in
   `VTCompressionProperties.rs`) — matches Linux/Android's Constrained-Baseline-first choice.
3. **`VTCompressionSessionInvalidate` callback-cutoff ordering — still unconfirmed** (Apple's
   official docs page is JS-rendered and unreachable via this session's `WebFetch`; not resolved
   from local `objc2` source either, since it only re-exposes the C API, not Apple's prose
   guarantees). Mitigated defensively rather than left as a live risk: `Drop` unconditionally
   calls `complete_frames(kCMTimeIndefinite)` **before** `invalidate()`, regardless of whether the
   caller already called `flush()` — narrows the risk window to "does `complete_frames` itself
   fully synchronize the callback thread before returning", which its own doc comment ("all
   pending frames will be emitted before the function returns") already asserts. Still worth
   confirming against real hardware once CI/hardware access exists.
4. **Color range: `FullRange`** — `kCVPixelFormatType_420YpCbCr8BiPlanarFullRange` (0–255), not
   `VideoRange` (16–235).
5. **CI scope: compile + clippy only**, matching the Android/Linux precedent, **not** real
   `cargo test` execution — even though `apple-macos` runs on genuine Apple Silicon hardware
   (unlike Android's cross-compile-only CI), Stage 1 stays parity with how every other backend
   first landed; hardware `cargo test` is deferred to a later "hardware verified" milestone.

## Addendum (2026-08-19): real per-packet `is_keyframe`

Closes the scope cut recorded in the "Implementation notes" section below. Grounded this
session against a direct read of the local `objc2` clone (not re-guessed): `CMSampleBuffer`'s
`sample_attachments_array(create_if_necessary: bool) -> Option<CFRetained<CFArray<CFDictionary
<CFString, CFType>>>>` (`generated/CoreMedia/CMSampleBuffer.rs`), `CFArray<T>::get`/
`CFDictionary<K, V>::get` (both safe, non-`unsafe` methods — `objc2-core-foundation/src/
{array,dictionary}.rs`), and `CFBoolean`'s real `unsafe impl ConcreteType` (confirmed via
`CFBooleanGetTypeID`, `generated/CoreFoundation/CFNumber.rs`) — the raw-pointer
`value_at_index`/`value` FFI this ADR originally worried about turned out unnecessary; the safe
container accessors were sufficient. `is_sync_sample` (new, `videotoolbox/video.rs`) reads the
`kCMSampleAttachmentKey_NotSync` attachment per Apple's documented convention (key absent ⇒
sync/keyframe; present + `true` ⇒ not). `SharedState::gop_size`/`packet_count` (the fields the
old heuristic needed) are removed as dead code, not left behind unused. Same
zero-compile-verification caveat as the rest of this ADR — no macOS/Xcode anywhere in this
workspace's sessions, this increment is unverified even at the compile level.

## Implementation notes (2026-08-12, written alongside the code)

- **Per-packet `is_keyframe` detection is scoped down further than this ADR's research pass
  implied.** Reading `kCMSampleAttachmentKey_NotSync` off `CMSampleBufferGetSampleAttachmentsArray`
  requires indexing a generic `CFArray<CFDictionary<CFString, CFType>>` via raw-pointer
  `value_at_index`/`value` calls — a third layer of unverified generic CoreFoundation container
  FFI on top of everything else in this ADR. Implemented instead: `is_keyframe = gop_size <= 1 ||
  packet_index == 0` (every packet when the session is IDR-only, otherwise only the session's
  first packet, which `VTCompressionSessionCreate` always emits as a keyframe). This is an
  explicit, documented scope cut (`SharedState::gop_size`'s doc comment in `videotoolbox/video.rs`),
  not a silent approximation — real per-packet sync-frame detection is deferred, same discipline
  as Android's `BUFFER_FLAG_CODEC_CONFIG` deferral.
- `VTSessionSetProperty`/`CVPixelBuffer::with_planar_bytes`/etc. all rely on Rust's deref
  coercion chain (`CFRetained<VTCompressionSession> → VTCompressionSession → CFType`, confirmed
  real via `objc2_core_foundation::type_traits::Type::as_CFTypeRef`'s doc comment: "CF types
  deref to CFType") rather than a manual pointer cast — simpler and safer *if* the coercion
  chain compiles as expected, but this is exactly the kind of detail the § CI verification plan
  exists to catch.
- `objc2-core-video`'s `"CVReturn"` feature is enabled explicitly and directly (beyond what
  `objc2-video-toolbox` pulls in transitively) — `CVPixelBufferCreateWithPlanarBytes`'s own
  `#[cfg(...)]` gate requires it and video-toolbox's own transitive feature list does not
  include it (mirrors why `"CMBlockBuffer"` needed the same direct-addition treatment for
  `objc2-core-media`, per the Decision section above).

## Consequences

### Positive

- Grounded entirely in real local source (`local/vendor-ref/objc2/generated/`), catching one
  concrete would-be bug (`CreateWithBytes` vs. `CreateWithPlanarBytes` for NV12) before any code
  was written.
- Smaller dependency graph than a naive plan: no `objc2`/`objc2-foundation`/`block2` needed for
  this C-API-shaped surface.
- Extradata (SPS/PPS) capture achievable in scope this stage (cheaper API shape than Android's),
  reusing the existing `iso_bmff::bitstream::avc::to_avcc` helper instead of new code.
- `GpuBufferHandle::Metal` already existing means the deferred Zero-Copy stage has no blocking
  type-design work left, only wiring — same as Android found for `AndroidSurface`.
- Stronger available CI bar than Android's (real Apple Silicon runners, not cross-compile-only),
  if the user opts into it (§ Open questions).

### Negative / Trade-offs

- **Real, non-trivial `unsafe` surface owned directly by this crate** — unlike Android/Linux,
  which stay `#![forbid(unsafe_code)]` behind fully safe wrapper crates, every `objc2-*` call here
  is `unsafe fn`; this module needs `#![allow(unsafe_code)]` + `// SAFETY:` discipline matching
  `src/windows/`, not the Android/Linux precedent.
- **Zero compile verification as authored**, for a structurally harder reason than Android (no
  legal cross-compile path at all outside Apple tooling) — every signature above is a research-pass
  read, not a real local build.
- Callback-lifetime safety (§ Callback design) rests on an **unconfirmed** assumption about
  `VTCompressionSessionInvalidate`'s ordering guarantee — flagged, not silently assumed, but still
  an open risk until resolved.
- `objc2-*` crates are pre-1.0 (`0.3.x`) — same semver-risk class as this workspace's other
  pre-1.0 platform bindings.
- Two real, undecided product questions (color range, `CFBoolean` feature name) block a clean
  implementation start until resolved.

## Addendum (2026-08-20) — `ProfileLevel` corrected: `ConstrainedBaseline` → `Baseline`

The Decisions section above resolved `kVTCompressionPropertyKey_ProfileLevel` to
`kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel` from local grounding alone (the constant is
real in `VTCompressionProperties.rs`, matching Linux/Android's Constrained-Baseline-first choice)
— but that grounding could only confirm the constant *exists*, not that VideoToolbox's hardware
encoder actually *accepts* it. This crate's first real run on Apple hardware (`bindings-tests-macos`,
`macos-14` GitHub-hosted runner, real Apple Silicon) surfaced a genuine `VTSessionSetProperty`
failure — `kVTParameterErr` (`OSStatus -12902`) — on every H.264 `AutoVideoEncoder::open()` call,
traced via a temporary file-based diagnostic (`eprintln!` alone never reached the CI log; VSTest's
testhost process isolation swallows raw native stderr writes from the C# RC-gate tests).

Root cause, corroborated by a real-world report
([livekit/client-sdk-swift#1002](https://github.com/livekit/client-sdk-swift/issues/1002)):
VideoToolbox's **hardware** H.264 encoder rejects `ConstrainedBaseline` outright — Constrained
Baseline drops B-frames and CABAC entirely, and Apple's hardware encoder does not allocate a
session for that "degraded" profile at all, only for plain `Baseline`/`Main`/`High`/`ConstrainedHigh`.
Software-only VideoToolbox sessions may tolerate it, but this backend never requests one
(`encoderSpecification: None` still resolves to hardware first on real Apple Silicon).

Fix: use `kVTProfileLevel_H264_Baseline_AutoLevel` instead. This keeps the same practical
bitstream shape the "Constrained-Baseline-class" scope always meant — `AllowFrameReordering` is
already `false` (no B-frames) and Baseline profile itself never allows CABAC (CAVLC-only by
spec) — so the only actual difference from true Constrained Baseline is a profile_idc/constraint-flag
detail no consumer of this crate depends on, while gaining real hardware-encoder support. Pending
re-confirmation from the next `bindings-tests-macos` run. HEVC's `kVTProfileLevel_HEVC_Main_AutoLevel`
(ADR-0002) is unaffected — Main is not a constrained profile and was never implicated by this failure.

## References

- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
- [`docs/spec/crate-packaging.md`](../../../../docs/spec/crate-packaging.md) — `apple` platform
  suffix (already reserved, single module for macOS+iOS)
- [`docs/spec/caveats-and-clarity.md`](../../../../docs/spec/caveats-and-clarity.md) — honesty
  requirement this ADR follows, including for its own unresolved-detail admissions
- `mediaway-encoder` [ADR-0021](../../../../docs/adr/0021-workspace-consolidation.md) — `#[cfg]`-gated
  backend modules, no separate platform crate
- `adr/android/0001-ndk-amediacodec-h264-cpu-upload.md` — the direct scope/honesty/structure
  precedent this ADR mirrors
- `adr/windows/0002-windows-crate.md` — precedent for depending on an official/community binding
  over hand-rolled FFI; also the precedent for real `unsafe`+`// SAFETY:` discipline in this crate
  (Android/Linux stayed `forbid(unsafe_code)`, this backend cannot)
- Local grounding source (read directly, not web-fetched): `local/vendor-ref/objc2/generated/VideoToolbox/{VTCompressionSession,VTCompressionProperties,VTErrors,VTSession}.rs`,
  `local/vendor-ref/objc2/generated/CoreVideo/{CVPixelBuffer,mod}.rs`,
  `local/vendor-ref/objc2/generated/CoreMedia/{CMSampleBuffer,CMBlockBuffer,CMFormatDescription,CMTime}.rs`,
  `local/vendor-ref/objc2/framework-crates/objc2-{video-toolbox,core-video,core-media,core-foundation}/Cargo.toml`,
  `local/vendor-ref/objc2/Cargo.toml` (workspace version/license)
- [`objc2` on GitHub](https://github.com/madsmtm/objc2) (`Zlib OR Apache-2.0 OR MIT`)
- `docs/roadmap.md` § platform order (Windows → Web → Linux → other) · crate
  `docs/roadmap.md` § Stage 4 — Other
