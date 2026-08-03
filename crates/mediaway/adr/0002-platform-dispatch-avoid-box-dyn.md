# ADR-0002: Platform dispatch returns concrete per-platform types, not `Box<dyn Trait>`

- **Status**: Proposed
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway`

## Context

[`src/platform.rs`](../src/platform.rs) is the crate's single `#[cfg(target_os = …)]`/
`#[cfg(windows)]` module (its own doc comment says so, line 1-2). Every public entry
point — `AutoEncoder::open`, `AutoDecoder::open`, `ScreenCapture::open`,
`Microphone::open` — is a zero-sized marker type's associated function that returns
`Box<dyn VideoEncoder>` / `Box<dyn VideoDecoder>` / `Box<dyn VideoCapture>` /
`Box<dyn AudioCapture>`, built by `#[cfg]`-branching between OS-specific backends and
`Box::new`-ing whichever one it picked.

This is worth re-examining against this project's own [ZCA policy](../../../docs/spec/zero-cost-abstractions.md)
(ADR-0009: "Avoid `Box<dyn Trait>` on hot / sans-io paths unless … facade plugin feature
or FFI erasure, documented"; "Prefer `enum` of known backends over `dyn Backend` **when
the set is closed at compile time**") for one specific reason: **the "choice" `platform.rs`
makes is not a runtime choice at all.** Each `#[cfg]` arm compiles into exactly one binary;
in any single compiled artifact, exactly one branch of `AutoEncoder::open` (etc.) exists —
there is never more than one candidate type alive to choose between at runtime. This is
different from a genuine multi-backend runtime selection.

### The genuine precedent for runtime selection already lives one layer down

`mediaway-encoder-windows`'s own `AutoVideoEncoder`
([`src/auto.rs`](../../mediaway-encoder-windows/src/auto.rs)) *does* face a real
closed-set runtime choice: on one Windows binary, WMF / software (rav1e-based) / NVENC /
QuickSync are all compiled in, and which one actually opens depends on live hardware/driver
probing. It represents that choice as a private enum, not `Box<dyn>`:

```rust
// crates/mediaway-encoder-windows/src/auto.rs (lines 20-26)
enum EncoderImpl {
    Wmf(WindowsVideoEncoder),
    Sw(mediaway_sw::av1::Av1Encoder),
    Nvenc(mediaway_encoder_nvenc::NvencVideoEncoder),
    QuickSync(mediaway_encoder_quicksync::QuickSyncVideoEncoder),
}
```

with the doc comment explicitly citing this project's ZCA rule ("Enum (not `Box<dyn>`)
for zero-cost abstraction per AGENTS.md § Zero-cost abstractions"), and
`impl VideoEncoder for AutoVideoEncoder` forwarding each trait method via `match
&self.inner { … }`. `mediaway-encoder`'s own founding ADR already names the alternative
this rejected: `crates/mediaway-encoder/adr/0001-encoder-traits.md`'s alternatives table
lists *"`Box<dyn VideoEncoder>` as the only API | Fights ZCA; hides concrete WMF session
types"* — i.e. this codebase has already decided, at the layer below `platform.rs`, that
`Box<dyn VideoEncoder>` should not be the *only* way to get an encoder.

`mediaway_common::GpuDeviceHandle`/`GpuBufferHandle`
([`src/gpu.rs`](../../mediaway-common/src/gpu.rs)) is a related but different precedent:
a `#[non_exhaustive]` tagged **data** enum (`DirectX11(NativeHandle) | DirectX12(…) |
Vulkan(…) | …`) with no trait-forwarding behavior at all — it is a value type describing
which platform a handle belongs to, not a dispatch mechanism standing in for a trait
object. `EncoderImpl` is the closer precedent for *this* ADR because it forwards trait
methods across backends the way `platform.rs`'s `Box<dyn Trait>` does.

### Why `platform.rs` boxes anyway

`AutoEncoder::open` gets a concrete, already-unboxed `AutoVideoEncoder` on Windows and a
concrete `mediaway_encoder_linux::LinuxVideoEncoder` on Linux (the latter has no `auto`
module — its VA-API backend implements only `VideoInputPreference::CpuUploadOk`, so there
is no backend choice to make there either, per the module's own comment at
`platform.rs` lines 46-48). It then does `Box::new(enc)` on **both** branches purely to
give the function one uniform return type across an OS split that is already fully
resolved at compile time by `#[cfg]`. Nothing about the OS split itself requires type
erasure — `#[cfg]`-gated type aliases can express "exactly one concrete type per compiled
target" without a `Box` or a vtable:

```rust
#[cfg(windows)]
pub type PlatformVideoEncoder = mediaway_encoder_windows::auto::AutoVideoEncoder;
#[cfg(target_os = "linux")]
pub type PlatformVideoEncoder = mediaway_encoder_linux::LinuxVideoEncoder;
```

`EncodeSession<E: VideoEncoder>` ([`src/session.rs`](../src/session.rs)) is already
generic — its own doc comment (lines 18-21) already says it "works with a concrete
unboxed encoder (e.g. Windows `AutoVideoEncoder`) or `Box<dyn VideoEncoder>` … without
imposing a `Box` where the caller doesn't already have one." `EncodeSession` needs no
change for this ADR: today it is instantiated as `EncodeSession<Box<dyn VideoEncoder>>`
only because that is what `platform::AutoEncoder::open` hands it; if `open` returns
`PlatformVideoEncoder` instead, callers get `EncodeSession<PlatformVideoEncoder>` for
free.

One real asymmetry exists and does not go away with this change: `AutoVideoEncoder::open`
(Windows) takes the high-level `AutoVideoEncodeConfig` directly, while
`LinuxVideoEncoder::open` takes the low-level `VideoEncoderConfig`. `AutoEncoder::open`'s
own body already bridges this today via `config.to_low_level(VideoInputPreference::CpuUploadOk,
config.gpu_device)` on the Linux arm — that adaptation function is unaffected; only its
return type changes.

`AutoDecoder::open`, `ScreenCapture::open`, and `Microphone::open` have the same shape,
one rung simpler: each OS has exactly one concrete backend type
(`WindowsVideoDecoder`/`LinuxVideoDecoder`, `WindowsScreenCapture`/`LinuxScreenCapture`,
`WindowsWasapiCapture` with no Linux backend today), with no intra-OS backend-selection
layer at all.

`encoder_support`, `decoder_support`, `device_support`, and `request_device_permission`
are **out of scope** — none of them return `Box<dyn Trait>` today (`Vec<EncoderCapability>`,
`DecodeSupport`, `Support`, `Result<PermissionState, _>`); this ADR does not touch them.

## Decision

> Replace `platform.rs`'s `Box<dyn Trait>` return types with `#[cfg]`-gated concrete type
> aliases (`PlatformVideoEncoder`, `PlatformVideoDecoder`, `PlatformVideoCapture`,
> `PlatformAudioCapture`), each naming the single concrete backend type live in that
> compiled target. `AutoEncoder::open` / `AutoDecoder::open` / `ScreenCapture::open` /
> `Microphone::open` return `Result<Platform*, *Error>` instead of
> `Result<Box<dyn Trait>, *Error>`.

- **Scope**: `AutoEncoder::open`, `AutoDecoder::open`, `ScreenCapture::open`,
  `Microphone::open` only. `encoder_support`/`decoder_support`/`device_support`/
  `request_device_permission` are unaffected (see above).
- The unsupported-platform arm (`#[cfg(not(any(windows, target_os = "linux")))]`) has no
  concrete backend type to alias to and can only ever return `Err`. The implementation
  should give that arm's `Ok` type `core::convert::Infallible` (it can never construct a
  value) rather than reusing `Box<dyn Trait>` there alone — an honest "this branch cannot
  succeed" signal, and it costs nothing since that arm never allocates regardless. This
  likely means `AutoEncoder::open`'s signature itself becomes `#[cfg]`-gated (two or three
  separate `fn` definitions instead of one signature shared by three `#[cfg]` bodies), not
  just the type alias — a small increase in `#[cfg]` surface in a module whose own doc
  comment already flags it as the crate's one platform-`#[cfg]` boundary.
- Low-level reachability is preserved, not reduced: callers who want type erasure still
  get it for free via reference coercion (`&mut concrete_type` → `&mut dyn VideoEncoder`)
  or by boxing at their own call site; callers who don't want erasure are no longer forced
  into it. See `api-layers.md` rule 3 ("Zero-Copy stays reachable") and rule 2 ("No
  opaque-only path") — this change moves `platform.rs` from the latter to neither, since
  neither a boxed nor an erased path was ever the *only* path at the trait/session layer
  below it.
- This is an implementation-shape decision, not a behavior change: `open`'s error
  semantics (`Err(NoBackend)` on unsupported platforms, propagated backend errors
  otherwise) are unchanged.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Status quo — `Box<dyn Trait>` unconditionally | Pays one heap allocation + one vtable indirection per `open` call on every currently-supported platform for a "choice" that is already fully resolved at compile time by `#[cfg]` — not a genuine multi-candidate runtime dispatch. Contradicts this project's own stated ZCA rule ("prefer enum of known backends over `dyn Backend` when the set is closed at compile time") one level more strongly, since here the set isn't just closed, it's a singleton per build. |
| Wrap in a real enum (`PlatformVideoEncoder::Windows(AutoVideoEncoder) \| Linux(LinuxVideoEncoder)`), mirroring `EncoderImpl`/`GpuDeviceHandle` | `EncoderImpl` earns its enum because **all four Windows encoder backends are compiled into the same binary simultaneously** and a live hardware probe decides between them at runtime — a genuine multi-candidate choice. Here, `#[cfg]` means only one variant could *ever* be constructed in a given compiled binary; a match with one always-reachable arm needs a discriminant tag and hand-written trait-forwarding boilerplate for exactly one case, which is strictly more code and no less type erasure at the ABI level than what a bare alias gives for free (the concrete type already implements the trait directly — no forwarding needed). Enum is the right tool when multiple backends are *simultaneously live in one binary*; a type alias is the right tool when a `#[cfg]` split already reduces that binary's candidate set to exactly one. |
| Associated type on a `PlatformDispatch` trait instead of an inherent type alias + fn | No caller anywhere in this codebase needs `AutoEncoder`/`AutoDecoder`/`ScreenCapture`/`Microphone` to be generic over a shared trait — they are marker types used only for their one `open` associated function. Adds an abstraction with zero callers ("no abstractions for one-off code"). |
| Keep `Box<dyn Trait>` only on the not-yet-supported-platform fallback arm, alias elsewhere (partially adopted here) | Not rejected — this is the fallback-arm approach folded into the Decision above, refined to `Infallible` instead of `Box<dyn Trait>` specifically because the fallback arm never constructs a value at all, so even a "just box it" fallback would be paying nothing at runtime but still be a slightly less honest signature than `Infallible`. |

## Consequences

### Positive

- Removes an unconditional heap allocation + vtable indirection from every
  `AutoEncoder::open`/`AutoDecoder::open`/`ScreenCapture::open`/`Microphone::open` call on
  every currently-supported platform, for a branch that was already resolved at compile
  time — direct alignment with [ADR-0009](../../../docs/spec/zero-cost-abstractions.md)
  and this codebase's own precedent (`EncoderImpl`; `mediaway-encoder` ADR-0001's explicit
  rejection of "`Box<dyn VideoEncoder>` as the only API").
- `EncodeSession<E: VideoEncoder>` needs **no code change** — it becomes
  `EncodeSession<PlatformVideoEncoder>` automatically wherever
  `platform::AutoEncoder::open`'s result flows into `EncodeSession::open`, staying
  monomorphic/inlinable through `examples/pipeline/screen_record.rs`,
  `examples/pipeline/encode_to_mp4.rs`, `examples/pipeline/trim_and_splice.rs`, and
  `crates/mediaway/tests/screen_mic_av_smoke.rs` without those callers writing
  anything differently.
- Pushes the one place type erasure is *actually* required — a C ABI opaque handle, which
  by construction needs one uniform pointer layout across platforms — down into
  `mediaway-ffi`, matching [`c-ffi.md`](../../../docs/spec/c-ffi.md) ("C ABI
  lives only in `mediaway-*-ffi`"). See Negative below for the one line that needs an
  explicit cast there.
- Confirmed via `examples/pipeline/screen_record.rs` (`record_loop(cap: &mut dyn
  VideoCapture, mic: &mut dyn AudioCapture, venc: &mut dyn VideoEncoder, …)`, lines
  158-160): passing `&mut screen`/`&mut mic`/`&mut venc` into a function parameter typed
  `&mut dyn Trait` is an ordinary unsized coercion that works identically whether the
  local binding is `Box<dyn Trait>` or a concrete `PlatformVideoCapture` — this pattern
  needs **zero source changes**.

### Negative / Trade-offs

- **Breaking API change** (pre-1.0, allowed per
  [`docs/spec/status.md`](../../../docs/spec/status.md), but real): the return type of all
  four `open` functions changes from a platform-independent `Box<dyn Trait>` to a
  platform-dependent (`#[cfg]`-gated) concrete alias. Code that names the return type
  explicitly (e.g. a struct field or a function signature declared as `Box<dyn
  VideoEncoder>` that relies on `open`'s result already being boxed, rather than
  `impl VideoEncoder` / `&mut dyn VideoEncoder` / boxing itself) must be updated to box
  explicitly. This is a **Rust-API-only** change — the trait method call surface itself
  (`push_frame`, `poll_packet`, …) is identical either way, so most call sites only need
  recompilation, not edits.
- One real call site needs an actual code change:
  [`crates/mediaway-ffi/src/encoder.rs`](../../mediaway-ffi/src/encoder.rs)
  defines `pub type AutoEncoderHandle = Box<dyn VideoEncoder>;` and, at
  `mediaway_auto_encoder_open` (line 62), calls `AutoEncoder::open(&rust_config)` and
  relies on **type inference** to give `encoder: Box<dyn VideoEncoder>` so that
  `let handle: Box<AutoEncoderHandle> = Box::new(encoder);` (line 67) type-checks as
  `Box<Box<dyn VideoEncoder>>`. After this ADR, `encoder` is a concrete
  `PlatformVideoEncoder`; that line needs an explicit
  `Box::new(Box::new(encoder) as Box<dyn VideoEncoder>)` (or equivalent) to keep
  compiling. `crates/mediaway-ffi/src/session.rs`'s `EncodeSessionHandle {
  inner: EncodeSession<AutoEncoderHandle> }` is unaffected — `AutoEncoderHandle` itself
  keeps its `Box<dyn VideoEncoder>` definition; only how it gets constructed in
  `encoder.rs` changes. The public C ABI shape (`mediaway_auto_encoder_t*`, function
  signatures) does not change.
- No other real call site needs an edit, but every one should be re-verified at
  implementation time: `examples/pipeline/trim_and_splice.rs`,
  `examples/pipeline/screen_record.rs`, `examples/pipeline/encode_to_mp4.rs`,
  `examples/encode/encode_h264.rs`, `examples/decode/decode_h264.rs`,
  `examples/device/capture_screen.rs`, `examples/device/capture_microphone.rs`, and
  `crates/mediaway/tests/screen_mic_av_smoke.rs` all call `platform::*::open`
  through `let` bindings with inferred types (no explicit `Box<dyn Trait>` annotation
  found at any of these sites) — expected to keep compiling unchanged, verify at
  implementation time.
- Documentation/rustdoc drifts out of date the moment this lands and must be updated in
  the same change, not after:
  - `platform.rs`'s own module doc (line 4: "All public functions return `Box<dyn
    Trait>` so callers stay platform-agnostic") becomes false.
  - `session.rs`'s `EncodeSession` doc (lines 18-21) naming
    `crate::platform::AutoEncoder::open` as a source of `Box<dyn VideoEncoder>`.
  - `README.md` (root) lines 97-100 ("`platform::ScreenCapture` / `platform::Microphone`
    / `platform::AutoEncoder` are typed against **facade traits** (`&mut dyn
    VideoCapture` / …)") and the surrounding code samples (`platform::AutoEncoder::open`
    around lines 80, 106-107, 110, 134-135) describe the pre-change return shape.
  - [`docs/adr/0014-pipeline-convenience-crate.md`](../../../docs/adr/0014-pipeline-convenience-crate.md)
    (the workspace ADR that created this crate and `EncodeSession`'s dual-mode generic
    shape) documents `platform::AutoEncoder::open(&config)` returning `Box<dyn
    VideoEncoder>` as a historical decision record — left as-is (ADRs are not rewritten
    after the fact), but this ADR should be cross-referenced from it if that workspace
    ADR gets a "superseded in part" note.
  - This crate's own [ADR-0001](0001-frame-filter-hook.md) (`FrameFilter` chain, already
    Accepted/implemented) justifies `Box<dyn FrameFilter>`'s per-frame vtable cost partly
    by pointing at `Box<dyn VideoEncoder>` in platform dispatch as an already-accepted
    precedent ("the same class of dynamic-dispatch cost this crate already accepts for
    `Box<dyn VideoEncoder>` in platform dispatch"). If this ADR is implemented, that
    sentence in ADR-0001 becomes stale — `FrameFilter`'s own heterogeneous-chain
    reasoning still stands independently, but the cross-reference should be revisited.
- `bindings/cpp/examples/*.cpp` (aspirational, does not compile today) call
  `AutoEncoder::open`/`ScreenCapture::open`/`Microphone::open` against a not-yet-existing
  C++ wrapper over the C ABI — unaffected, since the C ABI's own shape does not change
  (see the `mediaway-ffi` note above).
- The unsupported-platform (`not(any(windows, linux))`) arm has no concrete backend type
  to name; resolving it cleanly (e.g. `Infallible`) likely means `AutoEncoder::open`'s
  signature becomes `#[cfg]`-gated itself, not just the type alias behind it — a small
  increase in `#[cfg]` surface versus today's one shared signature.

## References

- [`src/platform.rs`](../src/platform.rs) — all four `open` functions, module doc line 4
- [`src/session.rs`](../src/session.rs) — `EncodeSession<E: VideoEncoder>`, lines 18-21
- [`crates/mediaway-encoder-windows/src/auto.rs`](../../mediaway-encoder-windows/src/auto.rs) —
  `EncoderImpl` enum precedent (lines 20-26), `AutoVideoEncoder` trait-forwarding `impl`
- [`crates/mediaway-common/src/gpu.rs`](../../mediaway-common/src/gpu.rs) —
  `GpuDeviceHandle`/`GpuBufferHandle` tagged **data** enum precedent (no trait
  forwarding — see Context for the distinction from `EncoderImpl`)
- [`crates/mediaway-encoder/adr/0001-encoder-traits.md`](../../mediaway-encoder/adr/0001-encoder-traits.md) —
  alternatives table already rejects "`Box<dyn VideoEncoder>` as the only API"
- [`crates/mediaway-ffi/src/encoder.rs`](../../mediaway-ffi/src/encoder.rs) —
  `AutoEncoderHandle = Box<dyn VideoEncoder>`, the one real call site needing a code change
- [`crates/mediaway-ffi/src/session.rs`](../../mediaway-ffi/src/session.rs) —
  `EncodeSessionHandle`, unaffected structurally
- [`docs/spec/zero-cost-abstractions.md`](../../../docs/spec/zero-cost-abstractions.md) —
  ADR-0009, `Box` rules, "enum of known backends when the set is closed at compile time"
- [`docs/spec/api-layers.md`](../../../docs/spec/api-layers.md) — low-level stays
  reachable, no opaque-only path
- [`docs/spec/c-ffi.md`](../../../docs/spec/c-ffi.md) — ADR-0004, C ABI erasure belongs in
  `mediaway-*-ffi` only
- [`docs/adr/0014-pipeline-convenience-crate.md`](../../../docs/adr/0014-pipeline-convenience-crate.md) —
  workspace ADR that created this crate and `EncodeSession`'s current dual-mode shape
- [ADR-0001](0001-frame-filter-hook.md) — this crate's prior ADR; its `Box<dyn
  VideoEncoder>`-in-platform-dispatch cross-reference would need revisiting if this ADR
  is implemented

ADRs are written in **English**.
