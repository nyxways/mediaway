using Mediaway.Common;
using Xunit;

namespace Mediaway.Container.Tests;

/// <summary>
/// Exercises the real native <c>mediaway_ffi</c> library for the 7 container formats wired
/// into this binding alongside plain MP4 (WebM/Ogg/ADTS/FLV/MPEG-TS/MP3/WAV). Every
/// payload/expected value mirrors a verified round trip already checked in the Rust FFI
/// smoke tests (<c>crates/mediaway-ffi/tests/{webm,ogg_adts,flv,ts,mp3,wav}_container_smoke.rs</c>)
/// and the C++ binding's own <c>examples/container/all_formats_smoke.cpp</c> — not invented here.
/// </summary>
public sealed class AllFormatsSmokeTests
{
    [Fact]
    public void WebM_RoundTripsFiveVp8Packets()
    {
        using var muxer = new Muxer(ContainerFormat.WebM);
        // Track id starts at 1, not 0 — WebM/Matroska's TrackNumber element must not be 0
        // (see Muxer(ContainerFormat) doc comment).
        uint trackId = muxer.AddTrack(new VideoTrackInfo
        {
            Id = 1,
            Codec = CodecKind.Vp8,
            TimeBase = new Rational(1, 30),
            Width = 64,
            Height = 64,
        });
        using var session = muxer.Begin();

        byte[] payload = new byte[16];
        Array.Fill(payload, (byte)0xAA);

        var webmBytes = new List<byte>();
        for (long i = 0; i < 5; i++)
        {
            using var packet = new Packet
            {
                StreamId = trackId,
                Pts = i,
                Dts = i,
                Duration = 1,
                IsKeyframe = i == 0,
                IsDiscard = false,
                Payload = payload,
            };
            session.PushPacket(packet);
            using var chunk = session.PollBytes();
            webmBytes.AddRange(chunk.Memory.Span.ToArray());
        }

        session.Flush();
        using var tail = session.PollBytes();
        webmBytes.AddRange(tail.Memory.Span.ToArray());

        Assert.True(webmBytes.Count > 4 && webmBytes[0] == 0x1A && webmBytes[1] == 0x45, "EBML magic present");

        using var demuxer = new Demuxer(ContainerFormat.WebM);
        demuxer.PushBytes(webmBytes.ToArray());
        int count = 0;
        while (demuxer.PollPacket() is { } p)
        {
            using (p)
            {
                count++;
            }
        }

        Assert.Equal(5, count);
    }

    [Fact]
    public void Ogg_RoundTripsOpusHeaderAndPacket()
    {
        var head = new List<byte>();
        head.AddRange("OpusHead"u8.ToArray());
        head.Add(1); // version
        head.Add(2); // channels
        head.Add(0); head.Add(0); // pre-skip
        uint rate = 48000;
        for (int i = 0; i < 4; i++)
        {
            head.Add((byte)(rate >> (8 * i)));
        }

        head.Add(0); head.Add(0); // output gain
        head.Add(0); // channel mapping family

        using var muxer = new OggMuxer(1);
        using (var headPacket = new Packet
        {
            StreamId = 0, Pts = 0, Dts = 0, Duration = 0, IsKeyframe = true, IsDiscard = false,
            Payload = head.ToArray(),
        })
        {
            muxer.PushPacket(headPacket);
        }

        using var oggBytesOwner = muxer.PollBytes();
        var oggBytes = new List<byte>(oggBytesOwner.Memory.Span.ToArray());

        byte[] audio = [1, 2, 3, 4];
        using (var audioPacket = new Packet
        {
            StreamId = 0, Pts = 960, Dts = 960, Duration = 0, IsKeyframe = true, IsDiscard = false,
            Payload = audio,
        })
        {
            muxer.PushPacket(audioPacket);
        }

        using var chunk = muxer.PollBytes();
        oggBytes.AddRange(chunk.Memory.Span.ToArray());
        muxer.Flush();

        Assert.True(oggBytes.Count > 4 && oggBytes[0] == (byte)'O' && oggBytes[1] == (byte)'g', "capture pattern present");

        using var demuxer = new OggDemuxer();
        demuxer.PushBytes(oggBytes.ToArray());
        using var packet = demuxer.PollPacket();
        Assert.NotNull(packet);
        Assert.Equal(4, packet!.Payload.Length);
        Assert.Equal(960, packet.Pts);
    }

    [Fact]
    public void Adts_RoundTripsTwoAacFramesWithSynthesizedPts()
    {
        using var muxer = new AdtsMuxer(44100, 2);
        byte[] rawAac = new byte[100];
        Array.Fill(rawAac, (byte)0xAB);

        for (int i = 0; i < 2; i++)
        {
            using var packet = new Packet
            {
                StreamId = 0, Pts = 0, Dts = 0, Duration = 0, IsKeyframe = true, IsDiscard = false,
                Payload = rawAac,
            };
            muxer.PushPacket(packet);
        }

        muxer.Flush();
        using var adtsBytesOwner = muxer.PollBytes();
        var adtsBytes = adtsBytesOwner.Memory.Span;
        Assert.True(adtsBytes.Length > 2 && adtsBytes[0] == 0xFF && (adtsBytes[1] & 0xF0) == 0xF0, "sync word present");

        using var demuxer = new AdtsDemuxer();
        demuxer.PushBytes(adtsBytes);

        long expectedPts = 0;
        for (int i = 0; i < 2; i++)
        {
            using var packet = demuxer.PollPacket();
            Assert.NotNull(packet);
            Assert.Equal(expectedPts, packet!.Pts);
            Assert.Equal(100, packet.Payload.Length);
            expectedPts += 1024;
        }
    }

    [Fact]
    public void Flv_RoundTripsVideoAndAudioTags()
    {
        using var muxer = new FlvMuxer();
        using var headerOwner = muxer.WriteHeader(hasAudio: true, hasVideo: true);
        var flvBytes = new List<byte>(headerOwner.Memory.Span.ToArray());

        byte[] avcc = [1, 0x42, 0, 0x1e, 0xff, 0xe1, 0, 0];
        muxer.AddVideoTrack(new VideoTrackInfo
        {
            Id = 0, Codec = CodecKind.H264, TimeBase = new Rational(1, 1000),
            Width = 1280, Height = 720, ExtraData = avcc,
        });
        byte[] asc = [0x12, 0x10];
        muxer.AddAudioTrack(new AudioTrackInfo
        {
            Id = 0, Codec = CodecKind.Aac, TimeBase = new Rational(1, 1000),
            SampleRate = 44100, Channels = 2, ExtraData = asc,
        });

        byte[] videoPayload = [0, 0, 0, 2, 0x65, 0x88];
        using (var videoPacket = new Packet
        {
            StreamId = FlvMuxer.VideoTrackId, Pts = 45, Dts = 33, Duration = 0, IsKeyframe = true, IsDiscard = false,
            Payload = videoPayload,
        })
        using (var videoChunk = muxer.PushPacket(videoPacket))
        {
            flvBytes.AddRange(videoChunk.Memory.Span.ToArray());
        }

        byte[] audioPayload = [1, 2, 3, 4];
        using (var audioPacket = new Packet
        {
            StreamId = FlvMuxer.AudioTrackId, Pts = 23, Dts = 23, Duration = 0, IsKeyframe = true, IsDiscard = false,
            Payload = audioPayload,
        })
        using (var audioChunk = muxer.PushPacket(audioPacket))
        {
            flvBytes.AddRange(audioChunk.Memory.Span.ToArray());
        }

        Assert.True(flvBytes.Count > 3 && flvBytes[0] == (byte)'F' && flvBytes[1] == (byte)'L' && flvBytes[2] == (byte)'V',
            "file signature present");

        using var demuxer = new FlvDemuxer();
        demuxer.PushBytes(flvBytes.ToArray());
        bool gotVideo = false, gotAudio = false;
        while (demuxer.PollPacket() is { } p)
        {
            using (p)
            {
                if (p.StreamId == FlvMuxer.VideoTrackId) gotVideo = true;
                if (p.StreamId == FlvMuxer.AudioTrackId) gotAudio = true;
            }
        }

        Assert.True(gotVideo && gotAudio, "both tracks recovered");
    }

    [Fact]
    public void Ts_RoundTripsVideoAccessUnitAndFinishRecoversTrailingUnit()
    {
        const ushort videoPid = 0x100;
        const ushort audioPid = 0x101;
        var streams = new[]
        {
            new TsElementaryStream { Pid = videoPid, Codec = CodecKind.H264 },
            new TsElementaryStream { Pid = audioPid, Codec = CodecKind.Aac },
        };

        using var muxer = new TsMuxer(1, 0x1000, streams);
        var tsBytes = new List<byte>();
        using (var patPmt = muxer.WritePatPmt())
        {
            tsBytes.AddRange(patPmt.Memory.Span.ToArray());
        }

        byte[] videoAu = [0, 0, 0, 1, 0x65, 0x88];
        using (var chunk = muxer.WriteAccessUnit(videoPid, videoAu, 90000, null, true))
        {
            tsBytes.AddRange(chunk.Memory.Span.ToArray());
        }

        byte[] videoAu2 = [0, 0, 0, 1, 0x41];
        using (var chunk = muxer.WriteAccessUnit(videoPid, videoAu2, 90033, null, false))
        {
            tsBytes.AddRange(chunk.Memory.Span.ToArray());
        }

        using var demuxer = new TsDemuxer();
        demuxer.PushBytes(tsBytes.ToArray());
        using var packet = demuxer.PollPacket();
        Assert.NotNull(packet);
        Assert.Equal(90000, packet!.Pts);
        Assert.True(packet.IsKeyframe);

        // finish() recovers a trailing access unit with no confirming marker.
        using var muxer2 = new TsMuxer(1, 0x1000, streams);
        var tsBytes2 = new List<byte>();
        using (var patPmt = muxer2.WritePatPmt())
        {
            tsBytes2.AddRange(patPmt.Memory.Span.ToArray());
        }

        byte[] tail = [9, 9, 9];
        using (var chunk = muxer2.WriteAccessUnit(videoPid, tail, 90000, null, true))
        {
            tsBytes2.AddRange(chunk.Memory.Span.ToArray());
        }

        using var demuxer2 = new TsDemuxer();
        demuxer2.PushBytes(tsBytes2.ToArray());
        Assert.Null(demuxer2.PollPacket());

        var finished = demuxer2.Finish();
        Assert.Single(finished);
        Assert.Equal(tail, finished[0].Payload.ToArray());
    }

    [Fact]
    public void Mp3_RoundTripsOneFrame()
    {
        var header = new Mp3FrameHeader
        {
            Version = Mp3MpegVersion.Mpeg1,
            BitrateKbps = 128,
            SampleRate = 44100,
            ChannelMode = Mp3ChannelMode.Stereo,
        };
        using var muxer = new Mp3Muxer(header);

        // frame_len(false) for 128kbps/44100Hz = floor(144000*128/44100) = 417; body = 417-4 = 413.
        byte[] body = new byte[413];
        Array.Fill(body, (byte)0xAB);
        using var mp3BytesOwner = muxer.WriteFrame(body, padding: false);
        var mp3Bytes = mp3BytesOwner.Memory.Span;
        Assert.Equal(0xFF, mp3Bytes[0]);

        using var demuxer = new Mp3Demuxer();
        demuxer.PushBytes(mp3Bytes);
        using var packet = demuxer.PollPacket();
        Assert.NotNull(packet);
        Assert.Equal(413, packet!.Payload.Length);
    }

    [Fact]
    public void Wav_RoundTripsPcmAndSecondFinishThrows()
    {
        using var muxer = new WavMuxer(44100, 2, 16);
        byte[] pcm = [1, 2, 3, 4, 5, 6, 7, 8];
        using (var packet = new Packet
        {
            StreamId = 0, Pts = 0, Dts = 0, Duration = 0, IsKeyframe = true, IsDiscard = false, Payload = pcm,
        })
        {
            muxer.PushPacket(packet);
        }

        using var wavBytesOwner = muxer.Finish();
        var wavBytes = wavBytesOwner.Memory.ToArray();
        Assert.True(wavBytes.Length > 12 && wavBytes[0] == (byte)'R' && wavBytes[8] == (byte)'W',
            "RIFF/WAVE header present");

        using var result = WavContainer.Parse(wavBytes);
        Assert.Equal(44100u, result.Info.SampleRate);
        Assert.Equal(2, result.Info.Channels);
        Assert.Equal(pcm, result.Packet.Payload.ToArray());

        var ex = Assert.Throws<MediawayContainerException>(() => muxer.Finish());
        Assert.Equal(MediawayContainerStatus.InvalidState, ex.Status);
    }
}
