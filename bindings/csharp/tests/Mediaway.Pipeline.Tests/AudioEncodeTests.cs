using Mediaway.Common;
using Xunit;

namespace Mediaway.Pipeline.Tests;

/// <summary>
/// Exercises the real native <c>mediaway_ffi</c> audio encode surface end-to-end — mirrors
/// <c>bindings/csharp/examples/Pipeline/EncodeAudio.cs</c>'s scenario (a synthetic 440 Hz sine
/// -&gt; AAC packets -&gt; AudioSpecificConfig) to verify the P/Invoke layer against the actual
/// ABI, not just that it compiles. This machine has a hardware-verified WMF AAC backend, so
/// <see cref="EncoderUnavailableException"/> is treated as a real test failure here, not a
/// graceful skip.
/// </summary>
public sealed class AudioEncodeTests
{
    private const int SampleRate = 48_000;
    private const int Channels = 2;
    private const int FrameSamples = 1024;

    private static AudioEncodeConfig MakeConfig() => new()
    {
        SampleRate = SampleRate,
        Channels = Channels,
        TimeBase = new Rational(1, SampleRate),
    };

    // One interleaved F32LE stereo frame of a deterministic 440 Hz sine.
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

    [Fact]
    public void EncodeSyntheticSine_ProducesAacPacketsAndAudioSpecificConfig()
    {
        using var encoder = AudioEncoder.Open(MakeConfig());

        for (int i = 0; i < 32; i++) // ~0.68 s — enough to exercise real encode, not the full 2 s.
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

        var packetCount = 0;
        while (encoder.PollPacket() is { } packet)
        {
            Assert.True(packet.Payload.Length > 0, "AAC packet carried an empty payload.");
            packetCount++;
        }

        Assert.True(packetCount > 0, "Encoder produced no AAC packets.");

        var info = encoder.StreamInfo();
        Assert.Equal(CodecKind.Aac, info.Codec);
        Assert.True(info.ExtraData.Length > 0, "StreamInfo carried no AudioSpecificConfig.");
    }

    [Fact]
    public void PollPacket_BeforeAnyPush_ReturnsNullNotError()
    {
        using var encoder = AudioEncoder.Open(MakeConfig());
        Assert.Null(encoder.PollPacket());
    }
}
