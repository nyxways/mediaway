// Auto video encode -> fragmented MP4 — quick-start example.
//
// The `Mediaway.Common`/`Mediaway.Pipeline` packages this targets are real —
// see bindings/csharp/src/ and docs/adr/0017-csharp-binding-package-layout.md.
// This shows the shipped ergonomics: `IDisposable` classes around native
// handles, exceptions instead of raw error codes, PascalCase members.
//
// Revision note: Finish() returns an IMemoryOwner<byte> (not byte[]) so the
// native fMP4 buffer is released deterministically on Dispose — see
// Mediaway.Common.Interop.NativeOwnedMemoryManager's own comment on why that
// buffer is never freed via a finalizer.
//
// Mirrors examples/encode_to_mp4.rs: pick the best available OS/GPU H.264
// encoder (Zero-Copy GPU path preferred, CPU-upload fallback), feed it 90
// frames (3 s at 30 fps) of a synthetic grey NV12 buffer, and write the
// resulting fragmented MP4 to disk.
//
// Run:
//     dotnet run

using System.Buffers;
using Mediaway.Common;
using Mediaway.Pipeline;

const int Width = 640;
const int Height = 480;
const int Fps = 30;
const int Seconds = 3;

// Defaults for H264 at this resolution/framerate, then override bitrate.
var config = VideoEncodeConfig.CreateDefault(VideoCodec.H264, Width, Height, new Rational(1, Fps)) with
{
    BitrateBps = 2_000_000,
};

AutoVideoEncoder encoder;
try
{
    // "Try the best available backend, tell me if none exists here."
    encoder = AutoVideoEncoder.Open(config);
}
catch (EncoderUnavailableException ex)
{
    Console.WriteLine($"EncodeToMp4: no supported H.264 encoder on this platform ({ex.Message}) — exiting.");
    return;
}

using var session = EncodeSession.Open(encoder);

// Synthetic NV12 source (replace with real frames in your app): grey Y=128,
// UV=128. Layout is Width*Height Y bytes followed by Width*Height/2
// interleaved UV bytes.
var nv12 = new byte[Width * Height + Width * Height / 2];
Array.Fill(nv12, (byte)128);

for (long pts = 0; pts < Fps * Seconds; pts++)
{
    var frame = new VideoFrame
    {
        Pts = pts,
        Duration = 1,
        Width = Width,
        Height = Height,
        PixelFormat = PixelFormat.Nv12,
        Data = nv12,
    };
    session.WriteFrame(frame);
}

using IMemoryOwner<byte> mp4Bytes = session.Finish();
using (var outFile = File.Create("out.mp4"))
{
    outFile.Write(mp4Bytes.Memory.Span); // Write the native buffer directly — no extra copy.
}

Console.WriteLine($"EncodeToMp4: {Fps * Seconds} frames -> out.mp4 ({mp4Bytes.Memory.Length} bytes)");
