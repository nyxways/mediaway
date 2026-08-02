using System.Buffers;
using Mediaway.Common;
using Mediaway.Common.Interop;

namespace Mediaway.Container;

/// <summary>Video-only geometry attached to a <see cref="StreamDescriptor"/>, when present.</summary>
public readonly record struct StreamGeometry(uint Width, uint Height);

/// <summary>
/// One stream/track discovered by a <see cref="Demuxer"/>, from <see cref="Demuxer.Streams"/>.
/// Owns the native buffer backing <see cref="ExtraData"/> — dispose when done with it.
/// </summary>
public sealed class StreamDescriptor : IDisposable
{
    private readonly IMemoryOwner<byte> _owner;

    internal StreamDescriptor(IMemoryOwner<byte> owner) => _owner = owner;

    public required uint Id { get; init; }

    public required CodecKind Codec { get; init; }

    /// <summary>Timebase for timestamps on packets belonging to this stream.</summary>
    public required Rational TimeBase { get; init; }

    /// <summary>Present only for video streams.</summary>
    public required StreamGeometry? Geometry { get; init; }

    /// <summary>Sample rate in Hz, or 0 if not applicable.</summary>
    public required uint SampleRate { get; init; }

    /// <summary>Channel count, or 0 if not applicable.</summary>
    public required ushort Channels { get; init; }

    public required ReadOnlyMemory<byte> ExtraData { get; init; }

    /// <summary>Releases the native buffer backing <see cref="ExtraData"/>.</summary>
    public void Dispose() => _owner.Dispose();
}
