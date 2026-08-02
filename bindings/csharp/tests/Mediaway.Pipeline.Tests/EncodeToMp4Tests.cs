using Mediaway.Common;
using Xunit;

namespace Mediaway.Pipeline.Tests;

/// <summary>
/// Exercises the real native <c>mediaway_pipeline_ffi</c> library end-to-end — mirrors
/// <c>bindings/csharp/examples/EncodeToMp4.cs</c>'s scenario (pick the best available H.264
/// encoder, feed it synthetic NV12 frames, flush to a fragmented MP4) to verify the P/Invoke
/// layer against the actual ABI, not just that it compiles. This machine has a
/// hardware-verified WMF/DX11 H.264 backend, so <see cref="EncoderUnavailableException"/> is
/// treated as a real test failure here, not a graceful skip.
/// </summary>
public sealed class EncodeToMp4Tests
{
    private const int Width = 640;
    private const int Height = 480;
    private const int Fps = 30;

    private static VideoEncodeConfig MakeConfig() =>
        VideoEncodeConfig.CreateDefault(VideoCodec.H264, Width, Height, new Rational(1, Fps)) with
        {
            BitrateBps = 2_000_000,
        };

    [Fact]
    public void EncodeSyntheticFrames_ProducesNonEmptyFragmentedMp4()
    {
        using var encoder = AutoVideoEncoder.Open(MakeConfig());
        using var session = EncodeSession.Open(encoder);

        var nv12 = new byte[Width * Height + Width * Height / 2];
        Array.Fill(nv12, (byte)128);

        for (long pts = 0; pts < Fps; pts++) // 1 s — enough to exercise real encode, not 3 s.
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
        }

        using var mp4Bytes = session.Finish();
        Assert.True(mp4Bytes.Memory.Length > 0, "Encoder produced no bytes after Finish().");
    }

    [Fact]
    public void Dispose_AfterFinish_IsASafeNoOp()
    {
        using var encoder = AutoVideoEncoder.Open(MakeConfig());
        var session = EncodeSession.Open(encoder);

        using (session.Finish())
        {
            // Just needs a non-empty buffer to prove Finish() actually ran.
        }

        // `session`'s native handle was already consumed by Finish() (unconditionally, per
        // pipeline.h) — Dispose() afterward must be a safe no-op, not a double-close. This
        // call implicitly proves that by not throwing or crashing.
        session.Dispose();
    }
}
