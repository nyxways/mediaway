# C#: GPU device factory + real Screen capture + capture-encode bridge

`bindings/csharp` — closes the same gap Node.js closed first (see
[nodejs-gpu-device](nodejs-gpu-device.md), [language-bindings](language-bindings.md)):
before this, no C# caller could construct or discover a GPU device, so
`Mediaway.Device.Desktop.DesktopScreenCapture.Open` was only reachable if the
app brought its own `ID3D11Device*` via raw COM interop — `ScreenRecord.cs`
kept a `NotImplementedException` placeholder, and `CaptureTests.cs`'s
`Screen_Open_...` test hand-rolled a `D3D11CreateDevice` P/Invoke
(`NativeD3D11`) just to exercise the capture at all.

## `Mediaway.Device` additions

- `GpuDevice.ListAdapters()` — enumerates every DXGI adapter (name, VRAM,
  hardware-vs-software), wrapping `mediaway_gpu_adapter_list`.
- `GpuDevice.Create(GpuDeviceOptions)` / `TryCreate` — `GpuAdapterSelect`
  (`Default` or `Index(uint)`), `VideoSupport`/`DebugLayer` flags. Wraps
  `mediaway_gpu_device_create`/`_handle`/`_close`; `GpuDevice.Handle` is the
  `Mediaway.Common.GpuDeviceHandle` value every existing consumer
  (`DesktopScreenCapture.Open`, `VideoEncodeConfig.GpuDevice`) already
  accepted — no shape change needed at those call sites.
- `mediaway_gpu_adapter_info_t.name` is the first owned `char*` **array**
  entry this binding reads (previous owned-string precedent,
  `DeviceHotplug`'s device-changed callback, is a single borrowed string) —
  decoded via `Marshal.PtrToStringUTF8` (net8.0) or a hand-rolled UTF-8 walk
  (netstandard2.0), the same net8/netstandard split `DeviceHotplug` already
  established.

## Capture-to-encode bridge

`EncodeSession.WriteFrameFromCameraCapture(IVideoCapture)` /
`.WriteFrameFromDesktopCapture(IDesktopVideoCapture)` — poll-and-push in one
native call, no intermediate frame type crossing into managed code, no CPU
copy for Screen's GPU frames (`adr/pipeline/0005-capture-encode-bridge-c-abi.md`).

## Cross-assembly SafeHandle sharing

`CameraCaptureSession`/`DesktopVideoCaptureSession` are `internal sealed`
(callers only ever see the public `IVideoCapture`/`IDesktopVideoCapture`
interfaces), and their `CameraCaptureHandle`/`DesktopCaptureHandle`
`SafeHandle`s are `internal` too — but `Mediaway.Pipeline` (a separate
assembly) needs the raw handle to call the two bridge P/Invokes above. Unlike
Node.js's symbol-keyed-method solution (`@mediaway/device`'s `NATIVE_HANDLE`),
C#'s own visibility system already has the right tool:
`[assembly: InternalsVisibleTo("Mediaway.Pipeline")]` added to
`Mediaway.Device.Camera`/`Mediaway.Device.Desktop`'s `Interop/AssemblyInfo.cs`,
plus a new internal `Handle` accessor property on each session class, plus new
`Mediaway.Pipeline.csproj` → `Mediaway.Device.Camera`/`Mediaway.Device.Desktop`
`ProjectReference`s (previously Pipeline referenced only `Mediaway.Common`).
`EncodeSession`'s bridge methods accept the public interface type and pattern-match
(`is not CameraCaptureSession session`) down to the internal concrete type to
reach `.Handle` — a caller can only pass a session it actually got from
`Camera.Open`/`DesktopScreenCapture.Open`, so an `ArgumentException` (not a
crash) is the failure mode for anything else. `SafeHandle` marshals
transparently as a P/Invoke parameter type regardless of which assembly
declared the subclass, for both classic `DllImport` and source-generated
`LibraryImport`/`DisableRuntimeMarshalling` — no special-casing needed beyond
the `InternalsVisibleTo` grant itself.

## Verified

`GpuDevice_ListAdapters_ReturnsRealAdapters` and
`Screen_Open_WithRealGpuDevice_CapturesRealGpuFrames`
(`Mediaway.Device.Tests/CaptureTests.cs`, replacing the old hand-rolled
`NativeD3D11` device creation — the cursor-nudge P/Invoke helpers survive,
renamed `NativeCursor`) both hardware-verified: real RTX 4090 + iGPU + WARP
adapters enumerated, real 1920x1080 GPU-backed screen frames captured.
`ScreenCaptureEncodeBridgeTests` (`Mediaway.Pipeline.Tests`, new, and — unlike
`Mediaway.Device.Tests` — actually runs in the release RC gate) exercises the
bridge itself, soft-skipping (mirroring `screen_capture_encode_bridge_smoke.rs`
exactly, stage by stage) on `GpuDevice.Create`/capture-open failure or on the
same known dev-machine limitation Node.js/C hit
(`MediawayPipelineStatus.Unsupported` once GPU-input frames actually start
flowing through the WMF/DX11 backend), so a CI runner without a working
GPU/display doesn't fail the RC gate. `ScreenRecord.cs` now builds a real
device via `GpuDevice.Create` and streams through `WriteFrameFromDesktopCapture`
instead of a manual poll/`WriteGpuFrame`/`ReleaseFrame` loop; both it and the
new test set `VideoEncodeConfig.PixelFormat = PixelFormat.Bgra8` (DXGI Desktop
Duplication delivers BGRA8, not `CreateDefault`'s NV12 — the same fix the Rust
smoke test and Node.js's `screen-record.ts` needed) and broaden their catch to
`MediawayPipelineException` with `Status` `NoBackend` **or** `Unsupported`.

## A pre-existing bug found while verifying

`ScreenRecord.cs`'s `DrainAudioAsync` used a bare `AudioFrame` under both
`using Mediaway.Device.Audio;` and `using Mediaway.Pipeline;` — ambiguous
(`Mediaway.Pipeline.AudioFrame` already existed) the moment both packages are
referenced together, which they only became once `Mediaway.Pipeline` gained
the `Mediaway.Device.Camera`/`Mediaway.Device.Desktop` `ProjectReference`s
above. Predates this work (both usings were already present); fixed by fully
qualifying `Mediaway.Device.Audio.AudioFrame` at the one call site.

## DDA `AccessDenied` is genuinely flaky on this dev machine

`Screen_Open_...`/`ScreenCaptureEncodeBridgeTests` passed repeatedly during
this work, then hit `MediawayDeviceStatus.AccessDenied` (a real native
rejection, not a managed-side bug) on both suites back to back — DXGI Desktop
Duplication's own documented behavior around locked/switched sessions.
Confirms the soft-skip design above is load-bearing, not CI-only defensiveness.
