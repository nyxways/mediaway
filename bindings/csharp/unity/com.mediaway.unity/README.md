# com.mediaway.unity

Unity Package Manager package providing Unity-specific glue over the
`Mediaway.Device`/`Mediaway.Pipeline` C# bindings — `Texture2D`/`RenderTexture`
conversion for captured video frames, `AudioClip`/`AudioSource` streaming for
captured audio frames. See
[`docs/adr/0018-csharp-netstandard20-unity.md`](../../../../docs/adr/0018-csharp-netstandard20-unity.md)
for why this lives in a separate package instead of inside `Mediaway.Device`
itself.

**Status: real, hand-written source — not yet verified in a Unity Editor.**

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
3. Only `win-x64` native assets are hardware-verified today — Editor/Standalone
   builds on Windows are the only currently-supported Unity target.

## What's here

- `Runtime/MediawayTextureConverter.cs` — `VideoFrame` → `Texture2D`.
  `Bgra8`/`Rgba8` upload directly (matching `TextureFormat.BGRA32`/`RGBA32`,
  no conversion). `Nv12`/`I420` go through a CPU YUV→RGBA32 conversion (no
  GPU shader path yet).
- `Runtime/MediawayStreamingAudioSource.cs` — a `MonoBehaviour` that polls an
  `IAudioCapture` via `TryPollFrame` and feeds interleaved float PCM into a
  streaming `AudioClip`/`AudioSource`.
- `Samples~/CameraToTexture/` — a minimal `MonoBehaviour` sample wiring
  `Mediaway.Device.Camera` into `MediawayTextureConverter` inside `Update()`.
  Unity-convention `~` suffix keeps it out of the default package import;
  users pull it in via the Package Manager's Samples tab.

## Why `TryPollFrame`, not `ReadFramesAsync`

`Mediaway.Device`'s async `ReadFramesAsync` API is `net8.0`-only, so this
package uses the synchronous `TryPollFrame` primitive instead — it works on
`netstandard2.0` and fits Unity's `Update()`-loop model naturally.
