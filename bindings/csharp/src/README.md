# Mediaway for .NET

Native media capture, encoding, and container mux/demux for .NET, backed by
Mediaway's C ABI (`mediaway_ffi.dll`, bundled in the packages — no separate
native install).

The native side is 100% Rust — no `libav*`/GPL codec dependencies, memory-safe
by construction where the OS/GPU APIs allow it. This package is a thin,
idiomatic `SafeHandle`-based wrapper over that Rust core's C ABI, not a
managed reimplementation.

Windows x64 is the fully hardware-verified platform (device/pipeline capture and
encode). Linux x64 is container-verified — `Mediaway.Container.Tests` passes
against a real `libmediaway_ffi.so` (`NativeMethods.cs`'s extensionless
`LibraryName = "mediaway_ffi"` already resolves per-platform via .NET's own
`DllImport` convention; only the test project's native-library staging needed a
Linux fallback). Device/pipeline capability on Linux is untested here. Pre-1.0:
APIs may change.

## Packages

| Package | What it does |
| --- | --- |
| `Mediaway.Common` | Shared types (`Rational`, stream/packet info) used by all packages |
| `Mediaway.Container` | Mux/demux for all 8 `mediaway-container` formats: MP4/WebM (`Muxer`/`Demuxer`), Ogg/ADTS/FLV/MPEG-TS/MP3 (dedicated classes), WAV (mux-only + `WavContainer.Parse`) |
| `Mediaway.Device` | Camera, microphone, and screen capture |
| `Mediaway.Device.Camera` | Camera capture (`Mediaway.Device.Camera`) |
| `Mediaway.Device.Audio` | Microphone capture (`Mediaway.Device.Audio`) |
| `Mediaway.Device.Desktop` | Screen capture (`Mediaway.Device.Desktop`) |
| `Mediaway.Device.Hotplug` | Device add/remove events (`Mediaway.Device.Hotplug`) |
| `Mediaway.Pipeline` | End-to-end capture → encode → mux pipeline |

## Install

```bash
dotnet add package Mediaway.Container    # mux/demux
dotnet add package Mediaway.Device       # capture
dotnet add package Mediaway.Pipeline     # capture → encode → mux
```

## Quick example — fMP4 mux

```csharp
using Mediaway.Container;

var muxer = new Muxer();
int v = muxer.AddVideoTrack(new VideoTrackInfo
{
    Codec = "h264",
    Width = 640,
    Height = 480,
    TimeBase = new Rational(1, 30),
});
byte[] init = muxer.Begin(); // ftyp + moov

for (int i = 0; i < 90; i++)
{
    muxer.Push(new Packet
    {
        TrackIndex = v,
        Data = encodedFrame(i),  // your H.264 bytes
        Pts = i,                 // ticks of the track timeBase
        Duration = 1,
        Key = i % 30 == 0,
    });
}
muxer.Flush();
byte[] tail = muxer.PollBytes();
muxer.Close();
// init + tail -> write to a file / stream
```

The muxer is sans-io: it never touches files — you own every byte of I/O.

## License

MIT OR Apache-2.0. Source: [github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).
