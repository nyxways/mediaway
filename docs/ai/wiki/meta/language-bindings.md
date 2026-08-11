# Language bindings

Root [`bindings/`](../../../../bindings/README.md) holds per-language folders
for the languages planned in [`docs/spec/c-ffi.md`](../../../spec/c-ffi.md)
(Tier B via C ABI, Tier C browser via WASM). The C ABI itself is real and
hardware-verified: `mediaway-ffi`, `mediaway-ffi`,
`mediaway-ffi`, `mediaway-ffi`. `bindings/README.md` is the
hub: per-language status legend, capability truth table, and scenario map.

## Status per language

- **C** — real, link+run-verified against the actual ABI
  (`bindings/c/examples/*.c`).
- **C#** — real under `bindings/csharp/src/`: `Mediaway.Common` (shared
  `Rational`/`CodecKind`/`PixelFormat`/`MediawayException`, plus the
  `GpuDeviceHandle`/`GpuBufferHandle`(+`Kind`)/`VideoFrameStorageKind` GPU
  mirrors — moved here from `Mediaway.Device.Desktop` once `Mediaway.Pipeline`
  became a second real consumer), `Mediaway.Container` (`Muxer`/
  `MuxerSession`/`Demuxer`), `Mediaway.Pipeline` (`AutoVideoEncoder`/
  `EncodeSession` — `WriteFrame` CPU-only, `WriteGpuFrame` Zero-Copy/GPU-copy
  via `VideoEncodeConfig.GpuDevice`, mirroring `mediaway-ffi`
  ADR-0002), and the `Mediaway.Device.*` package family (ADR-0019, mirroring
  Rust FFI ADR-0004): `Mediaway.Device` (shared), `Mediaway.Device.Camera`,
  `Mediaway.Device.Audio` (Microphone/Loopback/ProcessLoopback),
  `Mediaway.Device.Desktop` (Screen — Zero-Copy only, no CPU fallback), and
  `Mediaway.Device.Hotplug` (poll **and** real native push-mode callback).
  Each has an xUnit suite that runs against the real native DLL, not a mock —
  `Mediaway.Device.Tests` opens this machine's real USB camera + microphone
  and reads real frames; the recompiled `CameraRecord.cs` example produced a
  real 1920x1080 `out_camera.mp4` end-to-end. `ScreenRecord.cs` still throws
  `NotImplementedException` — no GPU device factory to build an
  `ID3D11Device*` from yet (Node.js has one, `@mediaway/device`'s
  `GpuDevice`; C# parity is open follow-up work). Design:
  [`docs/adr/0017-csharp-binding-package-layout.md`](../../../adr/0017-csharp-binding-package-layout.md),
  [`docs/adr/0019-csharp-device-package-split-and-hotplug-callback.md`](../../../adr/0019-csharp-device-package-split-and-hotplug-callback.md).
  Safety shape: `SafeHandle` per native handle, status→typed-exception
  mapping, owned native buffers via `IMemoryOwner<byte>`
  (`NativeOwnedMemoryManager`/`EmptyMemoryOwner` in
  `Mediaway.Common.Interop`) — **no finalizer** on the memory manager
  (CA2015: freeing under a live `Span<byte>` would be a use-after-free, not
  just a leak), so callers must `Dispose`/`using` every owned frame/buffer
  they receive. Handle-consuming calls (`mediaway_encode_session_open`/
  `_finish`, Device's frame frees) get `SafeHandle.SetHandleAsInvalid()`
  right after, preventing a later `Dispose` from double-closing. Device's and
  Pipeline's `VideoFrame` types are deliberately distinct (owned/disposable
  poll output vs. plain borrowed-input record).
  - **Unity**: all 4 packages dual-target `net8.0;netstandard2.0` (Unity
    consumes the latter via NuGetForUnity — no bespoke UPM package for the
    core bindings themselves). `netstandard2.0` uses classic `DllImport`
    (not source-generated `LibraryImport`, which needs net7.0+ BCL types);
    `System.Memory` is a required — not optional — netstandard2.0-only
    dependency, since `Span`/`Memory`/`IMemoryOwner` aren't in that BCL at
    all. `IVideoCapture`/`IAudioCapture` gained a synchronous
    `TryPollFrame` primitive on **both** TFMs; `ReadFramesAsync`
    (`IAsyncEnumerable`+`Channels`) stays net8.0-only — a deliberate
    zero-new-dependency choice, and a better fit for Unity's `Update()`-loop
    model. Unity-specific glue (`VideoFrame` → `Texture2D`, `AudioFrame` →
    streaming `AudioClip`) ships separately as
    `bindings/csharp/unity/com.mediaway.unity/` — a real, hand-written UPM
    package that is **unverified** (no Unity Editor available to compile or
    run it). Design:
    [`docs/adr/0018-csharp-netstandard20-unity.md`](../../../adr/0018-csharp-netstandard20-unity.md).
- **C++** — real under `bindings/cpp/`: RAII wrapper over the C ABI
  (`mediaway.hpp`), 7 examples compile `-Wall -Werror` and run for real.
- **Python** — real under `bindings/python/mediaway/` (`ctypes`), 7 examples
  run; encode output byte-identical to C/C++/Node.
- **Node.js** — real under `bindings/nodejs/packages/@mediaway/*` (koffi
  FFI); napi-rs is the eventual official path. Five packages now:
  `@mediaway/ffi`, `@mediaway/container`, `@mediaway/decoder`,
  `@mediaway/encoder`, `@mediaway/device`. GPU device factory + real Screen
  capture + capture-to-encode bridge landed here first (ahead of C#/Python/
  C++) — see [nodejs-gpu-device](nodejs-gpu-device.md).
- **Browser** — ✅ verified (ADR-0020): `@mediaway/browser` npm package — wasm
  mux/demux (`iso-bmff-wasm` promoted to real `Muxer`/`Demuxer` classes) +
  WebCodecs encode-to-MP4 (`EncodeSession`; avcC/ASC pulled from the first
  output's metadata, the browser analog of push → stream_info → mux). Capture is
  native Web APIs (getUserMedia/getDisplayMedia), canvas-bridged into the encode
  session. E2E: `tools/e2e-web/browser-package.spec.ts` (Chromium + real Edge).
- **Zig / Go / Swift / Kotlin** — not in the supported set; folders removed
  2026-08. Revisit per `docs/spec/c-ffi.md` Tier B when a consumer appears.

## RC validation

`bindings/` paths map to `NONE` in the CI affected-set, so binding checks
happen at release time: the `bindings-tests` job in `release.yml` runs each
binding's round-trip suite against the release-built DLL and gates every
publish job (also on `dry_run` = RC validation).

## Rules the examples/bindings follow

- English comments only.
- Map existing Rust surfaces only — no invented capabilities.
- Real bindings hide the raw C ABI entirely behind language-native types/
  idioms (exceptions, not status codes; RAII/`IDisposable`, not manual
  free calls) — a thin `@cImport`-style passthrough does not meet the bar.
- Aspirational `examples/*` are not part of the Cargo workspace or any CI.

See also: [device](../device/index.md) for the capture traits future
bindings wrap, [crate-packaging](crate-packaging.md) for `-ffi` naming.
