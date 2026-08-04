// EncodeAudio.cs — Mediaway C# quick start (AAC audio encode -> audio-only fragmented MP4).
//
// The `Mediaway.Common`/`Mediaway.Pipeline`/`Mediaway.Container` packages this targets are
// real — see bindings/csharp/src/ and adr/0003-auto-audio-encode-c-abi.md in
// crates/mediaway-ffi. `AudioEncoder.Open()` returns the encode session directly (single
// step — no intermediate handle, no consumption trap, unlike AutoVideoEncoder/EncodeSession
// in EncodeToMp4.cs). PCM is pushed as borrowed views, encoded packets are polled back, and
// an audio track registered with the encoder's AudioSpecificConfig (StreamInfo() —
// materialized after the first pushed frame) is muxed into an audio-only fMP4 via
// Mediaway.Container. Mirrors bindings/nodejs's own pipeline/encode-audio.ts.
//
// Deterministic: 96 frames of a 440 Hz sine (1024 samples @ 48 kHz, stereo f32le) — no
// microphone needed. No audio backend -> EncoderUnavailableException -> exit cleanly.
//
// Run:
//     dotnet run

using System.Buffers;
using Mediaway.Common;
using Mediaway.Container;
using Mediaway.Pipeline;

const int SampleRate = 48_000;
const int Channels = 2;
const int FrameSamples = 1024; // ~21 ms per pushed frame
const int FrameCount = 96; // ~2.0 s of audio

AudioEncoder encoder;
try
{
    encoder = AudioEncoder.Open(new AudioEncodeConfig
    {
        SampleRate = SampleRate,
        Channels = Channels,
        TimeBase = new Rational(1, SampleRate),
    });
}
catch (EncoderUnavailableException ex)
{
    Console.WriteLine($"EncodeAudio: no supported AAC encoder on this platform ({ex.Message}) — exiting.");
    return;
}

using (encoder)
{
    for (int i = 0; i < FrameCount; i++)
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

    var packets = new List<AudioPacket>();
    while (encoder.PollPacket() is { } packet)
    {
        packets.Add(packet);
    }

    if (packets.Count == 0)
    {
        Console.WriteLine($"EncodeAudio: encoder produced no packets for {FrameCount} PCM frames.");
        return;
    }

    AudioStreamInfo info = encoder.StreamInfo(); // ASC materialized after the first push.
    if (info.ExtraData.Length == 0)
    {
        Console.WriteLine("EncodeAudio: stream info carries no AudioSpecificConfig.");
        return;
    }

    Console.WriteLine($"EncodeAudio: encoded {packets.Count} AAC packet(s), ASC {info.ExtraData.Length} bytes.");

    using var muxer = new Muxer();
    uint audioTrackId = muxer.AddTrack(new AudioTrackInfo
    {
        Id = 0,
        Codec = info.Codec,
        TimeBase = info.TimeBase,
        SampleRate = info.SampleRate,
        Channels = info.Channels,
        ExtraData = info.ExtraData,
    });

    using MuxerSession session = muxer.Begin();
    foreach (AudioPacket packet in packets)
    {
        session.PushPacket(new Packet
        {
            StreamId = audioTrackId,
            Pts = packet.Pts,
            Dts = packet.Dts,
            Duration = packet.Duration,
            IsKeyframe = packet.IsKeyframe,
            IsDiscard = packet.IsDiscard,
            Payload = packet.Payload,
        });
    }

    session.Flush();

    using IMemoryOwner<byte> mp4Bytes = session.PollBytes();
    Console.WriteLine(
        $"EncodeAudio: muxed {packets.Count} AAC packet(s) into {mp4Bytes.Memory.Length} bytes of audio-only fragmented MP4.");
}

// One interleaved F32LE stereo frame of a deterministic 440 Hz sine.
static byte[] SineFrame(int frameIndex)
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
