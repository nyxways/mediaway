namespace Mediaway.Container;

/// <summary>Explicit RIFF/WAVE <c>fmt</c> chunk for <see cref="WavMuxer.WavMuxer(WaveFormat)"/>.</summary>
public sealed record WaveFormat
{
    public required WavSampleFormat SampleFormat { get; init; }

    public required ushort Channels { get; init; }

    public required uint SampleRate { get; init; }

    public required ushort BitsPerSample { get; init; }
}
