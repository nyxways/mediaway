# C# (.NET)

Windows desktop apps (WPF/WinUI) and Unity native plugins call Mediaway through P/Invoke
against the [`mediaway-ffi`](https://github.com/nyxways/mediaway/tree/main/crates/mediaway-ffi)
C ABI. Status: ✅ verified.

Examples live in [`bindings/csharp/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/csharp/examples):

| Example | Shows |
|---------|-------|
| `MuxRoundtrip.cs` | MP4 mux → demux round-trip |
| `EncodeToMp4.cs` | Encode + mux pipeline |
| `CameraRecord.cs` | Camera capture |
| `ScreenRecord.cs` | Screen capture (Zero-Copy) |

Build and run instructions: [`bindings/csharp/README.md`](https://github.com/nyxways/mediaway/blob/main/bindings/csharp/README.md).
