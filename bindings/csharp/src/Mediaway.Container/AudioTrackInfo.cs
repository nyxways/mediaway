using Mediaway.Common;

namespace Mediaway.Container;

/// <summary>Input to <see cref="Muxer.AddTrack(AudioTrackInfo)"/>.</summary>
public sealed record AudioTrackInfo
{
    /// <summary>Caller-assigned track id; must be unique per muxer.</summary>
    public required uint Id { get; init; }

    public required CodecKind Codec { get; init; }

    /// <summary>Timebase for timestamps on packets belonging to this track.</summary>
    public required Rational TimeBase { get; init; }

    public required uint SampleRate { get; init; }

    public required ushort Channels { get; init; }

    /// <summary>Extra header data. Borrowed for the call only.</summary>
    public ReadOnlyMemory<byte> ExtraData { get; init; } = ReadOnlyMemory<byte>.Empty;
}
