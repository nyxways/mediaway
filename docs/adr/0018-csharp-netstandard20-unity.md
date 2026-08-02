# ADR-0018: C# binding Unity support — netstandard2.0 dual-target + separate UPM integration package

- **Status**: Accepted (2026-07-31 — build sequence steps 1-4 done: all four
  `Mediaway.*` packages dual-target `net8.0;netstandard2.0` and build clean
  with 0 warnings/errors on both (`dotnet build`, no `-f` filter); the net8.0
  xUnit regression suites still pass 10/10 after the `TryPollFrame` refactor
  (Container 4, Pipeline 2, Device 4) — `netstandard2.0` itself is
  compile-verified only, not run (no Unity Editor here, see Deferred).
  `System.Memory` (Microsoft's own netstandard2.0 compatibility package, not
  a convenience dependency) was required and added — Span/Memory/
  IMemoryOwner types are not in the netstandard2.0 BCL at all, discovered by
  a real build failure (7 compiler errors) before adding it. `Marshal.
  PtrToStringUTF8` also does not exist pre-net5.0 — `DeviceHotplug` got a
  hand-rolled netstandard2.0 fallback. Step 5, `com.mediaway.unity`, is real
  source (`bindings/csharp/unity/com.mediaway.unity/`) but UNVERIFIED — no
  Unity Editor available to compile or run it.)
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)

## Context

ADR-0017 §2 shipped `net8.0`-only, explicitly deferring `netstandard2.1`
dual-targeting "unless a concrete older-runtime consumer appears." Unity is
that consumer: Unity's Mono/IL2CPP runtimes cannot load a `net8.0` managed
assembly, and `[LibraryImport]` (source-generated P/Invoke) depends on a
`System.Runtime.InteropServices.LibraryImportAttribute` type that only ships
in the .NET 7+ BCL — it does not exist in `netstandard2.0`.

Facts gathered before deciding:

- Modern Unity (2021.2+) accepts precompiled managed assemblies targeting
  `netstandard2.0` or `netstandard2.1` (selected via Player Settings API
  Compatibility Level); it does not consume `net8.0` assemblies directly.
- Unity does not restore NuGet packages itself, but **NuGetForUnity** — a
  widely used community package — lets a Unity project consume ordinary NuGet
  packages, including ones with native (`runtimes/<rid>/native/*.dll`)
  assets, without Mediaway building a bespoke distribution channel.
- `record`/`init` require the `System.Runtime.CompilerServices.IsExternalInit`
  marker type (ships in .NET 5+ BCL, absent from `netstandard2.0`); `required`
  members additionally need `RequiredMemberAttribute` and
  `CompilerFeatureRequiredAttribute` (.NET 7+ BCL). None of the three are
  actual runtime behavior — they are empty marker types the compiler checks
  for by full name, safe to hand-write once per assembly.
- `netstandard2.0`'s implicit default `LangVersion` is C# 7.3 — records,
  `init`, `required`, and pattern features used throughout the existing
  `Mediaway.*` source need an explicit `LangVersion` override regardless of
  the polyfill question above.
- `IAsyncEnumerable<T>` (used by `IVideoCapture.ReadFramesAsync`/
  `IAudioCapture.ReadFramesAsync`, ADR-0017 §3) and `System.Threading.Channels`
  (the backpressure mechanism behind them) both ship in the `net8.0` BCL but
  require `Microsoft.Bcl.AsyncInterfaces` / `System.Threading.Channels`
  NuGet packages on `netstandard2.0`. The user asked to avoid adding real
  NuGet dependencies here if a clean zero-dependency alternative exists.
- Unity's per-frame model is conventionally a synchronous `Update()`/
  `FixedUpdate()` poll loop, not `await foreach` — a synchronous poll-based
  frame API is arguably a *better* fit for Unity consumers than
  `IAsyncEnumerable`, not just a workaround for missing BCL types.
- Unity-specific integration (converting a `VideoFrame`'s NV12/I420/BGRA8
  pixel buffer into a `Texture2D`/`RenderTexture`; feeding an `AudioFrame`
  into an `AudioClip`/`AudioSource`) needs `UnityEngine` types the portable
  `Mediaway.*` assemblies must never reference — this has to live in code
  Unity itself compiles, not in a `dotnet build`-produced assembly.
- No Unity Editor is installed in this environment. Unity-referencing source
  written under this ADR cannot be compiled or run here — see Consequences.

## Decision

> `Mediaway.Common`/`Container`/`Pipeline`/`Device` dual-target
> `net8.0;netstandard2.0`, distributed as ordinary NuGet packages (consumed by
> Unity via NuGetForUnity — no bespoke UPM package for the core bindings).
> Unity-only glue code (texture/audio conversion) ships separately as a real
> UPM package that depends on the netstandard2.0 build.

### 1. Dual-target the four `Mediaway.*` packages

- `<TargetFrameworks>net8.0;netstandard2.0</TargetFrameworks>` on all four
  `.csproj`. `win-x64`/`linux-x64` native asset scope is unchanged from
  ADR-0017 §2 — this ADR only adds a second **managed** target, not new
  platforms.
- `net8.0` build: unchanged from ADR-0017 — `[LibraryImport]` +
  `[assembly: DisableRuntimeMarshalling]`, `IAsyncEnumerable<T>` +
  `System.Threading.Channels` for continuous frame delivery.
- `netstandard2.0` build: classic `[DllImport(LibraryName, ExactSpelling =
  true)] internal static extern` P/Invoke (no `DisableRuntimeMarshalling` —
  the attribute type does not exist pre-.NET 7; runtime marshalling handles
  the already-blittable (`byte`-not-`bool`) structs identically either way).
  Each `NativeMethods.cs` splits into two files
  (`NativeMethods.LibraryImport.cs` / `NativeMethods.DllImport.cs`), each
  wrapped in `#if NET8_0_OR_GREATER` / `#else`, so exactly one compiles per
  target — never both declaring the same member.
- `<LangVersion>` pinned explicitly (not left to the TFM-inferred default) so
  `netstandard2.0` compiles the same C# 9+ syntax (`record`, `init`,
  `required`, pattern matching) the `net8.0` build already uses.
- `System.Memory` (`Mediaway.Common`, netstandard2.0-only `PackageReference`,
  flows transitively to the other three) is required, not optional:
  `Span<T>`/`Memory<T>`/`IMemoryOwner<T>`/`MemoryManager<T>` — the buffer-
  ownership design ADR-0017 §3 is built on — are not part of the
  netstandard2.0 BCL at all (confirmed by a real build failure: 7 compiler
  errors before adding it). This is Microsoft's own compatibility package
  for that specific BCL gap, unlike the `Channels`/`AsyncInterfaces`
  packages this ADR deliberately avoids elsewhere (§3 below) — there was no
  design alternative that kept the existing owned-buffer API shape without
  it.
- `Marshal.PtrToStringUTF8` (used by `DeviceHotplug.PollEvent`) also does not
  exist before net5.0/netstandard2.1 — `netstandard2.0` gets a small
  hand-rolled UTF-8-pointer-to-string fallback instead of a dependency for
  one call site.

### 2. Hand-rolled polyfills instead of a NuGet package

- One shared source file
  (`Mediaway.Common/Interop/NetStandardPolyfills.cs`) declares
  `System.Runtime.CompilerServices.IsExternalInit`,
  `System.Runtime.CompilerServices.RequiredMemberAttribute`, and
  `System.Diagnostics.CodeAnalysis.CompilerFeatureRequiredAttribute` as
  `internal` types, compiled only when `'$(TargetFramework)' ==
  'netstandard2.0'`.
- Every other project links the same file in via MSBuild `<Compile
  Include>`, same TFM condition — `internal` visibility means each assembly
  needs its own copy of the *type*, not a shared reference, so linking (not
  a project/package reference) is required.
- Rejected the `PolySharp` NuGet package for this — a dependency is not
  worth adding for three marker types with no runtime behavior we can write
  ourselves in under 20 lines.

### 3. No `Microsoft.Bcl.AsyncInterfaces` / `System.Threading.Channels` dependency — `TryPollFrame` becomes the shared low-level primitive

- `IVideoCapture`/`IAudioCapture` gain `bool TryPollFrame(out VideoFrame?
  frame)` / `bool TryPollFrame(out AudioFrame? frame)` — a synchronous,
  non-blocking, zero-dependency wrap over the native `poll_frame`/
  `release_frame` pair. Available on **both** target frameworks.
- `ReadFramesAsync()` (`IAsyncEnumerable<T>` + `Channels`-based, per
  ADR-0017 §3) stays **`net8.0`-only**, implemented as a thin convenience
  loop over `TryPollFrame` (unchanged internal pump/channel implementation,
  just re-expressed on top of the new primitive instead of calling the
  native poll directly).
- This is not only a dependency-avoidance move: `docs/spec/api-layers.md`
  requires low-level entry points to stay first-class, with convenience
  layers composing them rather than being the only way in — `TryPollFrame`
  is that low-level entry point, and it also happens to fit Unity's
  `Update()`-loop consumption model better than an async-enumerable would.

### 4. Unity integration ships as a separate UPM package, not inside `Mediaway.*`

- New path: `bindings/csharp/unity/com.mediaway.unity/` — a standard Unity
  Package Manager package (`package.json`, `Runtime/` asmdef referencing
  `UnityEngine` + the NuGetForUnity-restored `Mediaway.Device`/
  `Mediaway.Pipeline` netstandard2.0 assemblies).
- Scope: pixel-format conversion from `VideoFrame`/`Mediaway.Device.VideoFrame`
  into `Texture2D`/`RenderTexture` (`Texture2D.LoadRawTextureData` /
  `SetPixelData` path), and `AudioFrame` → `AudioClip`/`AudioSource`
  streaming glue, built on the `TryPollFrame` primitive from §3 inside a
  `MonoBehaviour`-friendly `Update()`-shaped wrapper.
- Kept **out of** `Mediaway.Common`/`Device`/`Pipeline` — those must stay
  buildable with a plain `dotnet build` and must never reference
  `UnityEngine`. Mirrors the same boundary `docs/spec/crate-packaging.md`
  draws between a facade and a platform-specific adapter, just at the C#
  binding layer instead of the Rust crate layer.
- **Unverified**: no Unity Editor is available in this environment, so this
  package's source has not been compiled or run against real Unity APIs.
  Treated the same way `ScreenRecord.cs`/Screen capture is treated elsewhere
  in this ADR family — real, hand-written source, explicitly labeled
  unverified until exercised inside an actual Unity project.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `Microsoft.Bcl.AsyncInterfaces` + `System.Threading.Channels` polyfill packages on `netstandard2.0` | Keeps one unified `IAsyncEnumerable`-based API shape across both TFMs, but adds two real NuGet dependencies for a delivery style (`await foreach`) that is a worse fit for Unity's synchronous `Update()` loop anyway. `TryPollFrame` is both dependency-free and a better fit. |
| Bespoke UPM package carrying the *core* bindings (native asset placement, managed DLL) | NuGetForUnity already solves NuGet-package-in-Unity consumption for a wide range of existing packages; building a second, Mediaway-specific distribution channel for the same artifact NuGet already produces is redundant. |
| `PolySharp` NuGet package for the `record`/`init`/`required` polyfills | Three marker types with zero runtime behavior do not justify a dependency; hand-written and shared via file-link instead. |
| Fold Unity texture/audio glue into `Mediaway.Device`/`Mediaway.Pipeline` behind a Unity-detection `#if` | Would force a `UnityEngine` reference (or reflection hack) into packages that must build and run outside Unity too; breaks the "portable core, platform adapter separate" boundary this ADR otherwise draws. |

## Consequences

### Positive

- Unity consumers get the same memory-safe `SafeHandle` / typed-exception /
  owned-buffer design as every other C# consumer — no separate, lesser
  Unity-only API surface for the core bindings.
- Zero new real dependencies added to any `Mediaway.*` package for either
  target framework.
- `TryPollFrame` becomes the documented low-level primitive for both TFMs,
  which is a strict improvement to the `net8.0`-only shape ADR-0017 shipped,
  not just a Unity accommodation.

### Negative / Trade-offs

- Every P/Invoke surface, and the polyfill file, must now be maintained in
  two forms (`LibraryImport`/`DllImport`) — doubles the mechanical
  maintenance surface for future native ABI changes.
- `com.mediaway.unity` is real, unverified source — first real Unity Editor
  test pass is Deferred (see below), and a bug there would not be caught
  until then.
- Explicit `LangVersion` pin decouples the compiled language surface from
  the TFM-implied default — future contributors must remember both targets
  compile the same language version, not each TFM's own default.

## Deferred

- Actually compiling/running `com.mediaway.unity` inside a real Unity
  Editor project — not possible in this environment. First real
  verification is a follow-up task once a Unity Editor is available.
- `netstandard2.0` build verified via `dotnet build -f netstandard2.0` only
  (compiles cleanly) — not exercised under Mono/IL2CPP at runtime here, only
  inside a real Unity project (see above).
- `linux-x64`/other-RID native asset scope for Unity (e.g. Unity Editor on
  macOS/Linux, or IL2CPP mobile targets) — out of scope; `win-x64` is the
  only hardware-verified backend today per ADR-0017 §2, unchanged here.

## Build sequence

1. [x] Dual-target all four `.csproj`; add `LangVersion`; link the
   netstandard2.0-only polyfill file; add `System.Memory` to
   `Mediaway.Common` (netstandard2.0-only).
2. [x] Split each `NativeMethods.cs` into `LibraryImport`/`DllImport` halves;
   guard `DisableRuntimeMarshalling` to `net8.0` only.
3. [x] Add `TryPollFrame` to `Mediaway.Device`'s capture sessions; re-express
   `ReadFramesAsync` (net8.0-only) on top of it.
4. [x] Regression: `net8.0` xUnit suites stay green (10/10 — Container 4,
   Pipeline 2, Device 4); `netstandard2.0` builds clean, 0 warnings/errors,
   for all four projects (`dotnet build`, no `-f` filter — both TFMs).
5. [x] `com.mediaway.unity` UPM package: texture conversion
   (`MediawayTextureConverter` — direct upload for Bgra8/Rgba8, CPU-side
   YUV→RGBA32 for Nv12/I420) + audio glue (`MediawayStreamingAudioSource` —
   ring-buffered streaming `AudioClip`) + one sample
   (`Samples~/CameraToTexture`). Real source; UNVERIFIED (see Deferred) — no
   Unity Editor available here to compile or run it.
6. [x] Docs: `bindings/README.md`, wiki `language-bindings.md`.

## References

- spec: `docs/spec/c-ffi.md` (Tier B), `docs/spec/api-layers.md`
  (low-level-stays-first-class — grounds the `TryPollFrame` decision)
- related ADR: `docs/adr/0017-csharp-binding-package-layout.md` (extends §2's
  "no dual-target unless a concrete consumer appears" — Unity is that
  consumer)
- related ADR: `docs/adr/0003-crate-packaging.md` (facade/adapter boundary
  this ADR mirrors for the Unity UPM split)

ADRs are written in **English**.
