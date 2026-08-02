# com.mediaway.unity

Unity Package Manager package providing Unity-specific glue over the
`Mediaway.Device`/`Mediaway.Pipeline` C# bindings — `Texture2D`/`RenderTexture`
conversion for captured video frames, `AudioClip`/`AudioSource` streaming for
captured audio frames. See
[`docs/adr/0018-csharp-netstandard20-unity.md`](../../../../docs/adr/0018-csharp-netstandard20-unity.md)
for why this lives in a separate package instead of inside `Mediaway.Device`
itself.

**Status: real, hand-written source — unverified.** No Unity Editor is
available in the environment this was written in, so nothing under
`Runtime/` has been compiled or run against actual Unity APIs. Treat this the
same way `bindings/csharp/examples/ScreenRecord.cs` is treated elsewhere in
this repo: real code written against the documented API surface, not yet
exercised. First real verification is a follow-up task once a Unity Editor
is available (tracked in ADR-0018's Deferred section).

## Setup

1. This package does **not** bundle the Mediaway bindings themselves. Install
   [NuGetForUnity](https://github.com/GlitchEnzo/NuGetForUnity) in the
   consuming Unity project, then restore `Mediaway.Common`, `Mediaway.Device`,
   `System.Memory` (a transitive dependency of `Mediaway.Common` on
   netstandard2.0 — see ADR-0018 §"Hand-rolled polyfills"), and (if encoding)
   `Mediaway.Pipeline` — their `netstandard2.0` build targets Unity's
   Mono/IL2CPP runtimes.
2. Add this package via Unity's Package Manager (`Add package from disk...`,
   pointing at this folder's `package.json`, or a Git URL once this repo is
   public).
3. Only `win-x64` native assets are hardware-verified today (ADR-0017 §2) —
   Editor/Standalone builds on Windows are the only currently-supported
   Unity target.

## What's here

- `Runtime/MediawayTextureConverter.cs` — `VideoFrame` → `Texture2D`.
  `Bgra8`/`Rgba8` upload directly (matching `TextureFormat.BGRA32`/`RGBA32`,
  no conversion). `Nv12`/`I420` go through a **CPU/software** YUV→RGBA32
  conversion — no GPU shader path ships yet, so this is a real, documented
  cost (per the workspace's costly-path-honesty rule, ADR-0006), not a
  Zero-Copy path. A shader-based conversion is a reasonable follow-up once
  this ships and is measured in a real Unity project.
- `Runtime/MediawayStreamingAudioSource.cs` — a `MonoBehaviour` that polls an
  `IAudioCapture` via `TryPollFrame` (the low-level primitive ADR-0018 added
  to `Mediaway.Device`, available on netstandard2.0) and feeds interleaved
  float PCM into a streaming `AudioClip`/`AudioSource`.
- `Samples~/CameraToTexture/` — a minimal `MonoBehaviour` sample wiring
  `Mediaway.Device.Camera` into `MediawayTextureConverter` inside `Update()`.
  Unity-convention `~` suffix keeps it out of the default package import;
  users pull it in via the Package Manager's Samples tab.

## Why `TryPollFrame`, not `ReadFramesAsync`

`Mediaway.Device`'s `IVideoCapture`/`IAudioCapture` expose an async,
`IAsyncEnumerable`-based `ReadFramesAsync` on `net8.0` — but that is
`net8.0`-only (ADR-0018 deliberately did not add
`Microsoft.Bcl.AsyncInterfaces`/`System.Threading.Channels` to the
`netstandard2.0` build). The synchronous `TryPollFrame` primitive is on both
target frameworks, and is also a more natural fit for Unity's own
`Update()`-loop model than `await foreach` would be — this package always
uses `TryPollFrame`.
