// Auto video encode -> fragmented MP4, written to disk INCREMENTALLY — streaming example.
//
// EncodeToMp4.cs holds the whole recording in RAM until Finish(). That is fine for a few
// seconds and wrong for a long capture: a one-hour recording is a one-hour buffer.
// EncodeSession.PollBytes() (adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md)
// hands over the fMP4 bytes that are ready NOW, so memory is bounded by how often you poll
// instead of by how long you record.
//
// The contract, in three lines:
//   - PollBytes() returns a Zero-Copy owner over the native buffer; dispose it after writing.
//   - An empty owner means "nothing ready yet" — normal for most polls (fMP4 is written in
//     fragments, 30 frames each by default), not an error.
//   - Finish() returns only the bytes NOT yet polled: append that tail to the same file.
//
// Before opening an encoder you can ask which backends exist at your size, because encoder
// support depends on resolution:
//     EncoderSupport.Query(VideoCodec.H264, 640, 480)   // costly — call once, not per frame
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

var config = VideoEncodeConfig.CreateDefault(VideoCodec.H264, Width, Height, new Rational(1, Fps)) with
{
    BitrateBps = 2_000_000,
};

// Costly probe (opens a throwaway session per backend): decide up front, don't discover it
// by failing to open. Skipped here in favour of the graceful catch below, but this is the call.
foreach (var row in EncoderSupport.Query(config.Codec, Width, Height))
{
    Console.WriteLine($"  {row.Backend,-9} {row.State,-14} {row.PathClass}");
}

AutoVideoEncoder encoder;
try
{
    encoder = AutoVideoEncoder.Open(config);
}
catch (EncoderUnavailableException ex)
{
    Console.WriteLine($"StreamToMp4: no supported H.264 encoder on this platform ({ex.Message}) — exiting.");
    return;
}

using var session = EncodeSession.Open(encoder);
using var outFile = File.Create("out_streamed.mp4");

var nv12 = new byte[Width * Height + Width * Height / 2];
Array.Fill(nv12, (byte)128);

long written = 0;
int chunks = 0;

for (long pts = 0; pts < Fps * Seconds; pts++)
{
    session.WriteFrame(new VideoFrame
    {
        Pts = pts,
        Duration = 1,
        Width = Width,
        Height = Height,
        PixelFormat = PixelFormat.Nv12,
        Data = nv12,
    });

    // Drain after every frame; most polls are empty, a fragment lands every ~30 frames.
    using IMemoryOwner<byte> chunk = session.PollBytes();
    if (chunk.Memory.Length > 0)
    {
        outFile.Write(chunk.Memory.Span); // straight from the native buffer — no extra copy
        written += chunk.Memory.Length;
        chunks++;
    }
}

// The tail: everything the polls did not take (the last, partial fragment).
using IMemoryOwner<byte> tail = session.Finish();
outFile.Write(tail.Memory.Span);
written += tail.Memory.Length;

Console.WriteLine($"StreamToMp4: {Fps * Seconds} frames -> out_streamed.mp4 ({written} bytes; {chunks} streamed chunks + {tail.Memory.Length}-byte tail)");
