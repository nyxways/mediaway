using Mediaway.Common;

namespace Mediaway.Container;

/// <summary>Input to <see cref="Muxer.AddTrack(VideoTrackInfo)"/>.</summary>
public sealed record VideoTrackInfo
{
    /// <summary>Caller-assigned track id; must be unique per muxer.</summary>
    public required uint Id { get; init; }

    public required CodecKind Codec { get; init; }

    /// <summary>Timebase for timestamps on packets belonging to this track.</summary>
    public required Rational TimeBase { get; init; }

    public required uint Width { get; init; }

    public required uint Height { get; init; }

    /// <summary>Extra header data (e.g. AVCC). Borrowed for the call only.</summary>
    public ReadOnlyMemory<byte> ExtraData { get; init; } = ReadOnlyMemory<byte>.Empty;
}
