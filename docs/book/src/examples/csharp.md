# C# (.NET)

Windows desktop apps (WPF/WinUI) and Unity native plugins call Mediaway through P/Invoke
against the [`mediaway-ffi`](https://github.com/nyxways/mediaway/tree/main/crates/mediaway-ffi)
C ABI. Status: ✅ verified.

## Install

```bash
dotnet add package Mediaway.Container   # container mux/demux (pulls Mediaway.Common)
dotnet add package Mediaway.Device      # capture/playback; Mediaway.Device.{Camera,Desktop,Audio,Hotplug} subsets
dotnet add package Mediaway.Pipeline    # encode pipeline
```

```csharp
using Mediaway.Common;
using Mediaway.Container;

using var muxer = new Muxer();
muxer.AddTrack(new VideoTrackInfo
{
    Id = 0,
    Codec = CodecKind.H264,
    TimeBase = new Rational(1, 30),
    Width = 1920,
    Height = 1080,
});
using MuxerSession session = muxer.Begin(); // Open -> Live
/* session.PushPacket(...); session.Flush(); drain with PollBytes() */
```

Examples live in [`bindings/csharp/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/csharp/examples):

| Example | Shows |
|---------|-------|
| `MuxRoundtrip.cs` | MP4 mux → demux round-trip |
| `EncodeToMp4.cs` | Encode + mux pipeline |
| `CameraRecord.cs` | Camera capture |
| `ScreenRecord.cs` | Screen capture (Zero-Copy) |

Build and run instructions: [`bindings/csharp/README.md`](https://github.com/nyxways/mediaway/blob/main/bindings/csharp/README.md).
