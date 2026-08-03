using Mediaway.Common;
using Xunit;

namespace Mediaway.Container.Tests;

/// <summary>
/// Exercises the real native <c>mediaway_ffi</c> library end-to-end — mirrors
/// <c>bindings/csharp/examples/MuxRoundtrip.cs</c>'s scenario (mux one H.264 video + one AAC
/// audio track, demux the result back) to verify the P/Invoke layer against the actual ABI,
/// not just that it compiles.
/// </summary>
public sealed class MuxRoundtripTests
{
    [Fact]
    public void MuxThenDemux_RecoversAllPacketsAndStreamMetadata()
    {
        var frameRate = new Rational(1, 30);
        var audioTimeBase = new Rational(1, 48_000);
        const int frameCount = 90; // 3 s at 30 fps

        using var muxer = new Muxer();

        uint videoTrackId = muxer.AddTrack(new VideoTrackInfo
        {
            Id = 0,
            Codec = CodecKind.H264,
            TimeBase = frameRate,
            Width = 1920,
            Height = 1080,
        });

        uint audioTrackId = muxer.AddTrack(new AudioTrackInfo
        {
            Id = 1,
            Codec = CodecKind.Aac,
            TimeBase = audioTimeBase,
            SampleRate = 48_000,
            Channels = 2,
        });

        using MuxerSession session = muxer.Begin();

        byte[] fakeNalUnit = { 0x00, 0x00, 0x00, 0x01 };
        byte[] fakeAdtsFrame = { 0xFF, 0xF1 };

        for (int i = 0; i < frameCount; i++)
        {
            using var videoPacket = new Packet
            {
                StreamId = videoTrackId,
                Pts = i,
                Dts = i,
                Duration = 1,
                IsKeyframe = i % 30 == 0,
                IsDiscard = false,
                Payload = fakeNalUnit,
            };
            session.PushPacket(videoPacket);

            using var audioPacket = new Packet
            {
                StreamId = audioTrackId,
                Pts = i * 1_600L,
                Dts = i * 1_600L,
                Duration = 1_600,
                IsKeyframe = true,
                IsDiscard = false,
                Payload = fakeAdtsFrame,
            };
            session.PushPacket(audioPacket);
        }

        session.Flush();

        using var muxed = session.PollBytes();
        Assert.True(muxed.Memory.Length > 0, "Muxer produced no bytes after Flush().");

        using var demuxer = new Demuxer();
        demuxer.PushBytes(muxed.Memory.Span);

        var streams = demuxer.Streams;
        Assert.Equal(2, streams.Count);
        try
        {
            var videoStream = Assert.Single(streams, s => s.Id == videoTrackId);
            Assert.Equal(CodecKind.H264, videoStream.Codec);
            Assert.Equal(new StreamGeometry(1920, 1080), videoStream.Geometry);

            var audioStream = Assert.Single(streams, s => s.Id == audioTrackId);
            Assert.Equal(CodecKind.Aac, audioStream.Codec);
            Assert.Null(audioStream.Geometry);
        }
        finally
        {
            foreach (var stream in streams)
            {
                stream.Dispose();
            }
        }

        int videoPacketCount = 0;
        int audioPacketCount = 0;
        while (demuxer.PollPacket() is { } packet)
        {
            using (packet)
            {
                if (packet.StreamId == videoTrackId)
                {
                    videoPacketCount++;
                }
                else
                {
                    audioPacketCount++;
                }
            }
        }

        Assert.Equal(frameCount, videoPacketCount);
        Assert.Equal(frameCount, audioPacketCount);
    }

    [Fact]
    public void AddTrack_AfterBegin_ThrowsObjectDisposedException()
    {
        using var muxer = new Muxer();
        using var session = muxer.Begin();

        Assert.Throws<ObjectDisposedException>(() => muxer.AddTrack(new VideoTrackInfo
        {
            Id = 0,
            Codec = CodecKind.H264,
            TimeBase = new Rational(1, 30),
            Width = 640,
            Height = 480,
        }));
    }

    [Fact]
    public void PushPacket_ForUnregisteredTrack_ThrowsMediawayContainerException()
    {
        using var muxer = new Muxer();
        muxer.AddTrack(new VideoTrackInfo
        {
            Id = 0,
            Codec = CodecKind.H264,
            TimeBase = new Rational(1, 30),
            Width = 640,
            Height = 480,
        });
        using var session = muxer.Begin();

        using var packet = new Packet
        {
            StreamId = 99, // Never registered.
            Pts = 0,
            Dts = 0,
            Duration = 1,
            IsKeyframe = true,
            IsDiscard = false,
            Payload = new byte[] { 0x00 },
        };

        var ex = Assert.Throws<MediawayContainerException>(() => session.PushPacket(packet));
        Assert.Equal(MediawayContainerStatus.InvalidPacket, ex.Status);
    }

    [Fact]
    public void PollBytes_BeforeAnyPacket_ReturnsEmptyNoOpOwner()
    {
        using var muxer = new Muxer();
        muxer.AddTrack(new VideoTrackInfo
        {
            Id = 0,
            Codec = CodecKind.H264,
            TimeBase = new Rational(1, 30),
            Width = 640,
            Height = 480,
        });
        using var session = muxer.Begin();

        using var polled = session.PollBytes();
        Assert.Equal(0, polled.Memory.Length);
    }
}
