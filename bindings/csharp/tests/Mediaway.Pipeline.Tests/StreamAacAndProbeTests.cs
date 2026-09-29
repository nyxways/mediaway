using System.Buffers;
using System.Runtime.InteropServices;
using Mediaway.Common;
using Mediaway.Container;
using Mediaway.Pipeline.Interop;
using Xunit;

namespace Mediaway.Pipeline.Tests;

/// <summary>
/// <c>adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md</c> against the real native
/// library: streaming fMP4 output, AAC decode, and the capability probes. Mirrors the Rust
/// integration tests <c>stream_bytes_smoke</c>, <c>aac_decode_smoke</c> and
/// <c>capability_probe_smoke</c> in <c>crates/mediaway-ffi/tests</c>. This machine has the real
/// Windows backends, so an unavailable one is a failure here, not a graceful skip.
/// </summary>
public sealed unsafe class StreamAacAndProbeTests
{
    /// <summary>
    /// Platforms with an OS AAC decoder behind the C ABI: Media Foundation on Windows, AudioToolbox
    /// on macOS. Elsewhere AAC decode is <c>Unsupported</c> by design. Found by the v0.2.0 release
    /// pipeline's RC gate: these tests once assumed Windows, and macOS's decoder did not open at all
    /// until its magic cookie was fixed (an <c>esds</c> descriptor, not the bare ASC).
    /// </summary>
    private static bool HasOsAacDecoder =>
        RuntimeInformation.IsOSPlatform(OSPlatform.Windows) || RuntimeInformation.IsOSPlatform(OSPlatform.OSX);

    // ── ABI + layout pins ────────────────────────────────────────────────────────────────

    [Fact]
    public void PipelineAbiVersion_IsSeven()
    {
        Assert.Equal(7u, NativeMethods.mediaway_pipeline_ffi_abi_version());
    }

    /// <summary>
    /// Offsets derived by compiling a probe with gcc against the real <c>pipeline.h</c>:
    /// <c>mediaway_audio_decode_config_t</c> is 48 bytes (align 8) with fields at
    /// 0/4/8/16/32/40, and <c>mediaway_encoder_capability_t</c> is 12 bytes (align 4) with
    /// fields at 0/4/8. A reordered or resized field here would silently corrupt every call.
    /// </summary>
    [Fact]
    public void NativeStructs_MatchTheHeaderLayout()
    {
        NativeAudioDecodeConfig cfg = default;
        byte* b = (byte*)&cfg;
        Assert.Equal(48, sizeof(NativeAudioDecodeConfig));
        Assert.Equal(0, (byte*)&cfg.Codec - b);
        Assert.Equal(4, (byte*)&cfg.SampleRate - b);
        Assert.Equal(8, (byte*)&cfg.Channels - b);
        Assert.Equal(16, (byte*)&cfg.TimeBase - b);
        Assert.Equal(32, (byte*)&cfg.ExtraData - b);
        Assert.Equal(40, (byte*)&cfg.ExtraDataLen - b);

        NativeEncoderCapability row = default;
        byte* r = (byte*)&row;
        Assert.Equal(12, sizeof(NativeEncoderCapability));
        Assert.Equal(0, (byte*)&row.Backend - r);
        Assert.Equal(4, (byte*)&row.State - r);
        Assert.Equal(8, (byte*)&row.PathClass - r);
    }

    [Fact]
    public void ProbeEnums_MatchTheHeaderValues()
    {
        Assert.Equal(0, (int)EncodeBackend.Os);
        Assert.Equal(5, (int)EncodeBackend.Software);
        Assert.Equal(255, (int)EncodeBackend.Unknown);
        Assert.Equal(0, (int)SupportState.Supported);
        Assert.Equal(1, (int)SupportState.NotImplemented);
        Assert.Equal(2, (int)SupportState.NoDevice);
        Assert.Equal(0, (int)EncodePathClass.None);
        Assert.Equal(1, (int)EncodePathClass.ZeroCopy);
        Assert.Equal(5, (int)EncodePathClass.Software);
    }

    // ── Streaming bytes ──────────────────────────────────────────────────────────────────

    private const int Width = 640;
    private const int Height = 480;
    private const int FrameCount = 100; // several default 30-sample fragments

    private static VideoEncodeConfig VideoConfig() =>
        VideoEncodeConfig.CreateDefault(VideoCodec.H264, Width, Height, new Rational(1, 30)) with
        {
            BitrateBps = 2_000_000,
        };

    private static void WriteFrames(EncodeSession session, Action? afterEach)
    {
        var nv12 = new byte[Width * Height + Width * Height / 2];
        Array.Fill(nv12, (byte)128);
        for (long pts = 0; pts < FrameCount; pts++)
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
            afterEach?.Invoke();
        }
    }

    private static int CountPackets(byte[] fmp4)
    {
        using var demux = new Demuxer();
        demux.PushBytes(fmp4);
        int n = 0;
        while (demux.PollPacket() is { } packet)
        {
            packet.Dispose();
            n++;
        }

        return n;
    }

    [Fact]
    public void PollBytes_ChunksPlusFinishTail_AreTheWholeStream()
    {
        using var encoder = AutoVideoEncoder.Open(VideoConfig());
        using var session = EncodeSession.Open(encoder);

        var streamed = new MemoryStream();
        int nonEmptyPolls = 0;
        WriteFrames(session, () =>
        {
            using var chunk = session.PollBytes();
            if (chunk.Memory.Length > 0)
            {
                nonEmptyPolls++;
                streamed.Write(chunk.Memory.ToArray(), 0, chunk.Memory.Length);
            }
        });

        Assert.True(nonEmptyPolls >= 1, $"no fMP4 bytes were ready before Finish() over {FrameCount} frames");
        long polledBytes = streamed.Length;
        Assert.True(polledBytes > 8);
        Assert.Equal("ftyp", System.Text.Encoding.ASCII.GetString(streamed.GetBuffer(), 4, 4));

        long tailBytes;
        using (var tail = session.Finish())
        {
            tailBytes = tail.Memory.Length;
            streamed.Write(tail.Memory.ToArray(), 0, tail.Memory.Length);
        }

        // Finish after polling is the tail, not a second copy of the whole stream.
        Assert.True(tailBytes < polledBytes, $"Finish() returned {tailBytes} bytes; {polledBytes} were already polled");
        Assert.Equal(FrameCount, CountPackets(streamed.ToArray()));

        // The same input, never polled, is the reference: same packet count, same size.
        using var refEncoder = AutoVideoEncoder.Open(VideoConfig());
        using var refSession = EncodeSession.Open(refEncoder);
        WriteFrames(refSession, null);
        using var whole = refSession.Finish();
        Assert.Equal(FrameCount, CountPackets(whole.Memory.ToArray()));
        Assert.Equal(whole.Memory.Length, streamed.Length);
    }

    [Fact]
    public void PollBytes_BeforeAnyFrame_IsEmptyNotAnError()
    {
        using var encoder = AutoVideoEncoder.Open(VideoConfig());
        using var session = EncodeSession.Open(encoder);
        using var chunk = session.PollBytes();
        Assert.Equal(0, chunk.Memory.Length);
    }

    // ── AAC decode ───────────────────────────────────────────────────────────────────────

    private const int SampleRate = 48_000;
    private const int Channels = 2;
    private const int FrameSamples = 1024;
    private const int AacFrames = 48;

    private static byte[] SineFrame(int frameIndex)
    {
        var data = new byte[FrameSamples * Channels * sizeof(float)];
        for (int s = 0; s < FrameSamples; s++)
        {
            double t = (double)((frameIndex * FrameSamples) + s) / SampleRate;
            float v = (float)Math.Sin(2 * Math.PI * 440 * t);
            for (int c = 0; c < Channels; c++)
            {
                BitConverter.GetBytes(v).CopyTo(data, (s * Channels + c) * sizeof(float));
            }
        }

        return data;
    }

    private static (byte[] Asc, List<AudioPacket> Packets) EncodeAac()
    {
        using var encoder = AudioEncoder.Open(new AudioEncodeConfig
        {
            SampleRate = SampleRate,
            Channels = Channels,
            TimeBase = new Rational(1, SampleRate),
        });
        for (int i = 0; i < AacFrames; i++)
        {
            encoder.PushPcm(new AudioFrame
            {
                Pts = i * FrameSamples,
                Duration = FrameSamples,
                SampleRate = SampleRate,
                Channels = Channels,
                Data = SineFrame(i),
            });
        }

        encoder.Flush();
        var asc = encoder.StreamInfo().ExtraData.ToArray();
        var packets = new List<AudioPacket>();
        while (encoder.PollPacket() is { } p)
        {
            packets.Add(p);
        }

        return (asc, packets);
    }

    [Fact]
    public void AacEncodeDecode_RoundTripsThroughTheBinding()
    {
        var (asc, packets) = EncodeAac();
        Assert.NotEmpty(asc);
        Assert.NotEmpty(packets);

        using var decoder = AudioDecodeSession.OpenAac(SampleRate, Channels, new Rational(1, SampleRate), asc);
        foreach (var p in packets)
        {
            decoder.PushPacket(new DecodePacket
            {
                Pts = p.Pts,
                Dts = p.Dts,
                Duration = p.Duration,
                IsKeyframe = p.IsKeyframe,
                Payload = p.Payload,
            });
        }

        decoder.Flush();

        long samplesPerChannel = 0;
        double energy = 0;
        while (decoder.PollFrame() is { } frame)
        {
            Assert.Equal((ushort)Channels, frame.Channels);
            for (int i = 0; i + 4 <= frame.Data.Length; i += 4)
            {
                double v = BitConverter.ToSingle(frame.Data, i);
                energy += v * v;
            }

            samplesPerChannel += frame.Data.Length / sizeof(float) / Channels;
        }

        // Sample-exact on the reference machine (48 packets x 1024), and a real signal: a unit
        // sine has mean square 0.5, silence has 0.
        Assert.Equal((long)packets.Count * FrameSamples, samplesPerChannel);
        double meanSquare = energy / (samplesPerChannel * Channels);
        Assert.True(meanSquare > 0.1, $"decoded audio is near-silent (mean square {meanSquare})");
    }

    [Fact]
    public void OpenAac_WithoutAudioSpecificConfig_IsInvalidInput()
    {
        if (!HasOsAacDecoder)
        {
            return; // no OS decoder here: the open is Unsupported before the config is looked at
        }

        var ex = Assert.Throws<MediawayPipelineException>(
            () => AudioDecodeSession.OpenAac(SampleRate, Channels, new Rational(1, SampleRate), ReadOnlyMemory<byte>.Empty));
        Assert.Equal(MediawayPipelineStatus.InvalidInput, ex.Status);
    }

    [Fact]
    public void AacPushPacket_WithEmptyPayload_IsInvalidInput()
    {
        if (!HasOsAacDecoder)
        {
            return; // no OS AAC decoder on this platform
        }

        // Opus's empty packet is a loss-concealment hint; AAC has no such convention.
        using var decoder = AudioDecodeSession.OpenAac(
            SampleRate, Channels, new Rational(1, SampleRate), new byte[] { 0x11, 0x90 });
        var ex = Assert.Throws<MediawayPipelineException>(() => decoder.PushPacket(new DecodePacket
        {
            Pts = 0,
            Duration = FrameSamples,
            Payload = ReadOnlyMemory<byte>.Empty,
        }));
        Assert.Equal(MediawayPipelineStatus.InvalidInput, ex.Status);
    }

    [Fact]
    public void OpusDecodeStillOpens()
    {
        // The grown config must leave Opus (NULL extra_data) untouched.
        using var decoder = AudioDecodeSession.Open(48_000, 1, new Rational(1, 50));
        Assert.Null(decoder.PollFrame());
    }

    // ── Capability probes ────────────────────────────────────────────────────────────────

    [Fact]
    public void EncoderSupport_RowsKeepTheirInvariants()
    {
        foreach (var codec in new[] { VideoCodec.H264, VideoCodec.Hevc })
        {
            foreach (var row in EncoderSupport.Query(codec, 1280, 720))
            {
                if (row.State == SupportState.Supported)
                {
                    Assert.NotEqual(EncodePathClass.None, row.PathClass);
                }
                else
                {
                    Assert.Equal(EncodePathClass.None, row.PathClass);
                }
            }
        }
    }

    [Fact]
    public void EncoderSupport_AgreesWithActuallyOpeningAnEncoder()
    {
        var rows = EncoderSupport.Query(VideoCodec.H264, 1280, 720);
        if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
        {
            // Only Windows probes per backend; every other platform reports no rows (documented on
            // the probe), so there is nothing to compare with opening an encoder.
            Assert.Empty(rows);
            return;
        }

        Assert.NotEmpty(rows); // Windows reports at least one backend row
        bool anySupported = rows.Any(r => r.State == SupportState.Supported);

        bool opened;
        try
        {
            using var encoder = AutoVideoEncoder.Open(
                VideoEncodeConfig.CreateDefault(VideoCodec.H264, 1280, 720, new Rational(1, 30)));
            opened = true;
        }
        catch (EncoderUnavailableException)
        {
            opened = false;
        }

        Assert.Equal(anySupported, opened);
    }

    [Fact]
    public void EncoderSupport_ZeroSize_IsInvalidInput()
    {
        var ex = Assert.Throws<MediawayPipelineException>(() => EncoderSupport.Query(VideoCodec.H264, 0, 720));
        Assert.Equal(MediawayPipelineStatus.InvalidInput, ex.Status);
    }

    [Fact]
    public void DecoderSupport_AacIsSupportedWhereTheOsHasADecoder_AndASessionThenOpens()
    {
        var state = DecoderSupport.Query(CodecKind.Aac);
        if (!HasOsAacDecoder)
        {
            Assert.NotEqual(SupportState.Supported, state);
            return;
        }

        Assert.Equal(SupportState.Supported, state);
        using var decoder = AudioDecodeSession.OpenAac(
            SampleRate, Channels, new Rational(1, SampleRate), new byte[] { 0x11, 0x90 });
        Assert.Null(decoder.PollFrame());
    }

    [Fact]
    public void DecoderSupport_AnswersForAVideoCodec()
    {
        Assert.NotEqual(SupportState.Unknown, DecoderSupport.Query(CodecKind.H264));
    }
}
