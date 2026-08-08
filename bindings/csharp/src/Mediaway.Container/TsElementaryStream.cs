using Mediaway.Common;

namespace Mediaway.Container;

/// <summary>One elementary stream registered in <see cref="TsMuxer"/>'s constructed PMT.</summary>
public sealed record TsElementaryStream
{
    /// <summary>TS packet identifier, must be in <c>2..=0x1FFF</c> (0/1 are reserved for PAT/CAT).</summary>
    public required ushort Pid { get; init; }

    /// <summary>Must be H264, HEVC, AAC, or MP3.</summary>
    public required CodecKind Codec { get; init; }
}
