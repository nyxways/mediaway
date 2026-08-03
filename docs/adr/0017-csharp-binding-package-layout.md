# ADR-0017: C# binding package layout, safety design, and build sequence

- **Status**: Accepted. **Device-package layout superseded by
  [ADR-0019](0019-csharp-device-package-split-and-hotplug-callback.md)** (2026-08-01):
  `Mediaway.Device` split into `Mediaway.Device` (base) +
  `Mediaway.Device.Camera`/`.Desktop`/`.Audio`/`.Hotplug`, mirroring
  `mediaway-device-ffi`'s own domain-feature split; push-mode hotplug is now
  wired to the real native callback instead of staying deferred. The
  Container/Pipeline package layout and general safety-design decisions
  below are unaffected and still current. (2026-07-31 — build sequence steps 1-4 implemented:
  `Mediaway.Common`, `Mediaway.Container`, `Mediaway.Pipeline`,
  `Mediaway.Device` (Camera/Microphone/DeviceHotplug poll mode; Screen
  capture deferred) all real under `bindings/csharp/src/`, each verified
  with an xUnit suite that runs against the real native DLL —
  `Mediaway.Container.Tests` round-trips a real mux→demux,
  `Mediaway.Pipeline.Tests` drives a real H.264 hardware encode through
  WMF/DX11 and flushes a real fragmented MP4, `Mediaway.Device.Tests` opens
  this machine's real USB camera + microphone and reads real frames. The
  aspirational `bindings/csharp/examples/MuxRoundtrip.cs`/`EncodeToMp4.cs`/
  `CameraRecord.cs` were updated in place to match the shipped API
  (`IMemoryOwner<byte>` instead of `byte[]`/bare `ReadOnlyMemory<byte>` for
  owned native buffers; `Mediaway.Device.VideoFrame` and
  `Mediaway.Pipeline.VideoFrame` are distinct disposable-vs-plain types, not
  one shared record) and recompiled+run end-to-end against the real
  packages and real hardware — `CameraRecord.cs` produced a real 1920x1080
  `out_camera.mp4`. `ScreenRecord.cs` stays aspirational. Step 5 (NuGet
  packaging) not yet started.)
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)

## Context

`docs/spec/c-ffi.md` places C# in Tier B (P/Invoke over the C ABI). The C ABI
itself already exists and is hardware-verified: `mediaway-common-ffi`
(`rlib`-only, no C symbols), `mediaway-container-ffi`, `mediaway-pipeline-ffi`,
`mediaway-device-ffi` (ADR-0001/0002/0003 in that crate). `bindings/csharp/examples/*.cs`
are aspirational-only today (per `bindings/README.md`) — no real
`bindings/csharp/src/` package exists yet.

The user decided to hold off making the repo public until real, per-language
bindings exist, built to a fully idiomatic standard for each language — not a
thin P/Invoke passthrough with syntax sugar. C# is first in the agreed
sequence (C# → Python → Node → rest), acting as the reference quality bar the
other languages are held to.

> **Superseded 2026-08-03**: the binding sequence completed — C, C++, C#,
> Python, Node.js are verified and npm/NuGet/PyPI-publishable, the browser
> package shipped (ADR-0020), and the repository is public. The quality bar
> this paragraph set still stands; the "hold off" condition is resolved.

This decision spans all three existing `-ffi` crates (not just one), and sets
precedent the Python/Node bindings will also follow — so it is a workspace-wide
ADR (`docs/adr/`), not a crate-local one.

Facts gathered before deciding:

- A symbol-collision check (`#[no_mangle] extern "C" fn` across all three
  `-ffi` crates) found no name collisions: `mediaway_muxer_*`/`mediaway_demuxer_*`
  vs. `mediaway_auto_encoder_*`/`mediaway_encode_session_*` vs.
  `mediaway_video_capture_*`/`mediaway_audio_capture_*`/`mediaway_device_hotplug_*`
  are deliberately distinct per ADR-0015. `mediaway-common-ffi` exports zero C
  symbols, so it never appears in a P/Invoke `DllImport`/`LibraryImport`
  target — its value types are mirrored directly in C#.
- The known `device.h` + `pipeline.h` C co-include hazard (duplicate
  `mediaway_rational_t`/`mediaway_pixel_format_t` struct-tag redefinition,
  `mediaway-device-ffi/docs/roadmap.md` Stage 4) is C-preprocessor-only. It does
  not affect C#: P/Invoke resolves per-named-native-module
  (`LoadLibrary`/`dlopen`), with no textual struct-redefinition step. The real
  remaining risk is semantic, not a link error: `PixelFormat` is still
  independently defined in both `pipeline.h` and `device.h` (only
  `Rational`/`CodecKind` were unified into `mediaway-common-ffi` per
  ADR-0015) — the two C# mirrors must be kept in sync by hand until that gap
  is closed on the Rust side.
- The `mediaway-device-ffi` ADR-0003 GPU-handle work unblocks Screen capture
  for this binding: `ScreenRecord.cs`'s target API is no longer blocked on an
  unrepresentable `GpuDeviceHandle`.

## Decision

> We ship one NuGet package per capability, mirroring the Rust crate split, with
> a raw P/Invoke layer wrapped by a public safe, idiomatic layer.

### 1. Package layout

- `Mediaway.Common` — shared value types (`Rational`, `CodecKind`,
  `PixelFormat`, `GpuDeviceHandle`/`GpuBufferHandle` mirrors) and the
  `MediawayException` hierarchy. No native asset of its own (no `-ffi` crate
  backs it — `mediaway-common-ffi` is `rlib`-only).
- `Mediaway.Container` — `Muxer`/`Demuxer` over `mediaway-container-ffi`.
- `Mediaway.Pipeline` — `AutoVideoEncoder`/`EncodeSession` over
  `mediaway-pipeline-ffi`.
- `Mediaway.Device` — `Camera`/`Microphone`/`ScreenCapture`/`DeviceHotplug`
  over `mediaway-device-ffi`.
- No fat umbrella package in v1. A `Mediaway.All` meta-package (dependency-only,
  no code) may follow later if consumers ask for one.

### 2. Target framework and platform

- `net8.0` only. Enables source-generated `[LibraryImport]` (no reflection-based
  marshaling, AOT/trim-friendly). No `netstandard2.1` dual-target unless a
  concrete older-runtime consumer appears.
  **Amended by [ADR-0018](0018-csharp-netstandard20-unity.md):** Unity turned
  out to be that consumer — all four packages now dual-target
  `net8.0;netstandard2.0`. Read ADR-0018 alongside this section.
- `win-x64` ships first for all four packages — the only platform with
  hardware-verified backends today (WMF encode, DXGI screen, WASAPI audio).
- `linux-x64` ships for `Mediaway.Container` alone from day one: `iso-bmff`
  mux/demux is sans-io and has no platform backend to verify.

### 3. Safety design

- `internal NativeMethods` — raw `[LibraryImport]` P/Invoke declarations,
  1:1 with the C headers (`crates/mediaway-*-ffi/include/mediaway/*.h`).
  Never public.
- `SafeHandle` subclass per opaque native handle (`MuxerSessionHandle`,
  `EncodeSessionHandle`, `VideoCaptureHandle`, `AudioCaptureHandle`,
  `DeviceHotplugHandle`, ...). `SafeHandle`'s critical finalizer guarantees the
  matching native `_close` call runs even if a caller forgets `Dispose`/`using`.
- Status-enum → typed exception hierarchy, one exception type per capability
  domain, all deriving `MediawayException` (`Mediaway.Common`):
  `MediawayContainerException`, `MediawayPipelineException`,
  `MediawayDeviceException`. Two capability-specific leaf types the example
  files already assume: `CaptureUnavailableException` and
  `EncoderUnavailableException`. No raw `mediaway_*_status_t` value is ever
  exposed in the public API.
- Every device-opening surface exposes both a throwing `Open()` and a
  non-throwing `TryOpen(...)` for the same operation (mirrors
  `DateTime.Parse`/`TryParse` and `IServiceProvider.GetService`/
  `GetRequiredService`) — the binding does not decide once, for everyone,
  which absences are fatal; each call site picks.
- Owned native output buffers (e.g. `mediaway_encode_session_finish`,
  `mediaway_muxer_session_poll_bytes`) are wrapped in a `MemoryManager<byte>`
  so the native `mediaway_buffer_free` runs on `Dispose`, instead of an eager
  `Marshal.Copy` into a managed `byte[]` — this is the honesty-rule-relevant
  (ADR-0006) Zero-Copy point: a copy here would silently defeat the whole
  project's Zero-Copy premise at the binding boundary. Borrowed inputs (e.g. a
  packet payload pushed into the muxer) use `fixed`/pinning, never a copy.
- Continuous frame delivery (`IVideoCapture`/`IAudioCapture`) is
  `IAsyncEnumerable<T>`, consumed with `await foreach`; the native
  `poll_frame`/`release_frame` cycle runs underneath on a loop the binding
  owns. Sparse device-hotplug notifications are a plain C# event
  (`DeviceHotplug.DeviceChanged`) — the opposite delivery shape for the
  opposite delivery pattern.
- Frame payloads are `ReadOnlyMemory<byte>`, not `byte[]`, for GC-frame data
  (`Span<byte>` cannot cross the `await` boundaries `IAsyncEnumerable`
  introduces). GPU-resident frames (`VideoFrameStorageKind.Gpu` from
  ADR-0003) get their own leased-handle type with explicit `Dispose()`
  semantics — not designed in this ADR; tracked as Deferred below.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| One fat `Mediaway` package covering all capabilities | Forces every consumer to pull in Device/Pipeline/Container native assets even if they only need one; does not mirror the Rust crate split the rest of the workspace already commits to (ADR-0003 crate packaging). |
| `netstandard2.1` dual-target | No concrete consumer needs it yet; doubles the marshaling code path (source-generated `LibraryImport` needs `net7.0`+) for a hypothetical requirement. Revisit if a real consumer asks. |
| Expose raw `mediaway_*_status_t` / C structs directly, thin wrapper only | Explicitly rejected per the project's binding-quality bar — must be fully idiomatic, not a passthrough with syntax sugar. |
| `Marshal.Copy` owned buffers into `byte[]` eagerly | Simpler, but silently defeats Zero-Copy at the binding boundary and violates the "document/preserve costly paths" rule (ADR-0006). `MemoryManager<byte>` keeps the native buffer alive until `Dispose` instead. |

## Consequences

### Positive

- Consumers install only the capability they need; native asset size per
  package stays proportional to what it wraps.
- The `SafeHandle` + typed-exception + `MemoryManager<byte>` combination gives
  C# consumers a fully memory-safe, idiomatic surface with no raw pointers or
  status codes in the public API.
- Sets a concrete precedent (package-per-capability, safe-handle ownership,
  exception mapping, owned-vs-borrowed buffer types) that the Python and Node
  bindings can adapt to their own idioms instead of re-deriving from scratch.

### Negative / Trade-offs

- Four packages to version and release instead of one — more NuGet/CI surface
  area than a single package.
- `PixelFormat` must be kept in sync by hand across the `Mediaway.Pipeline` and
  `Mediaway.Device` mirrors until the Rust-side `device.h`/`pipeline.h` header
  unification (deferred in `mediaway-device-ffi/adr/README.md`) lands.
- `net8.0`-only excludes any consumer still on `netstandard2.1`/`net6.0`/`net7.0`
  until a real request justifies a second target.

## Deferred

- GPU-resident frame handle type (`VideoFrameStorageKind.Gpu` consumer API) —
  needs its own design once `Mediaway.Device`'s Screen capture path is
  implemented against `mediaway-device-ffi` ADR-0003. Implementers should read
  that ADR's hazard list (COM refcount discipline, texture read-window vs. the
  driver thread's next `CopyResource`, `ID3D11Multithread::SetMultithreadProtected`)
  before designing the wrapping `SafeHandle`.
- NuGet packaging + CI matrix (multi-RID native asset packaging, signing).
- Whether to update the four `bindings/csharp/examples/*.cs` files in place
  once real source lands next to them, or promote them to a `samples/`
  directory. (`MuxRoundtrip.cs`, `EncodeToMp4.cs`, `CameraRecord.cs` were
  updated in place as each package landed; `ScreenRecord.cs` stays
  aspirational until Screen capture below is implemented.)
- `DeviceHotplug` push-mode callback registration
  (`mediaway_device_hotplug_register_callback`) — `Mediaway.Device` ships
  poll mode (`PollEvent()`) only. Marshalling a long-lived C# delegate as a
  native function pointer safely (keeping it rooted against the GC for the
  registration's lifetime, respecting `device.h`'s "must not block / must
  not call back into this handle / must not unwind across the FFI boundary"
  callback contract) is a distinct, non-trivial design question not yet
  worked through — poll mode covers every binding consumer so far.

## Build sequence

1. [x] `Mediaway.Common` — types + exception hierarchy only, no native asset.
2. [x] `Mediaway.Container` — smallest surface, no hardware dependency;
   validates the whole pattern against `MuxRoundtrip.cs`.
3. [x] `Mediaway.Pipeline` — validates the `Finish()`-consumes-handle
   contract, targets `EncodeToMp4.cs`. Confirmed the "unconditional consume,
   success or failure" hazard `pipeline.h` documents for
   `mediaway_encode_session_open`/`_finish` needs `SafeHandle.SetHandleAsInvalid()`
   right after the call, not inside `ReleaseHandle()` — otherwise a later
   `Dispose()` would double-close an already-freed native pointer.
4. [x] `Mediaway.Device` — Camera/Microphone/DeviceHotplug (poll mode),
   targeting `CameraRecord.cs`. Real-verified: real USB camera geometry +
   frames, real microphone frames, real hotplug open/poll. Screen capture
   (`ScreenRecord.cs`) and hotplug push-mode callback registration remain
   deferred (see Deferred). `Mediaway.Device.VideoFrame`/`AudioFrame` are
   owned, disposable types (Zero-Copy poll output) — distinct from
   `Mediaway.Pipeline.VideoFrame` (a plain, non-disposable borrowed-input
   value type); `CameraRecord.cs`'s record loop explicitly converts between
   them per-frame instead of a single-type `with` clone.
5. NuGet packaging + CI matrix.
6. Decide the fate of the four aspirational example files (see Deferred).
7. [ADR-0018](0018-csharp-netstandard20-unity.md): `netstandard2.0`
   dual-target for Unity + separate `com.mediaway.unity` UPM package.

## References

- spec: `docs/spec/c-ffi.md` (Tier B)
- related ADR: `docs/adr/0003-crate-packaging.md`, `docs/adr/0004-c-ffi.md`,
  `docs/adr/0006-caveats-and-clarity.md`, `docs/adr/0015-common-ffi-unification.md`
- related ADR: `crates/mediaway-device-ffi/adr/0003-gpu-handle-c-abi.md`
  (GPU handle hazards this binding's Screen-capture wrapper must respect)
- examples: `bindings/csharp/examples/*.cs` (aspirational target API shape)

ADRs are written in **English**.
