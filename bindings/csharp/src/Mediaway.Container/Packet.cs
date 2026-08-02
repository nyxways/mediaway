using System.Buffers;
using Mediaway.Common.Interop;

namespace Mediaway.Container;

/// <summary>
/// One container packet — used both as input to <see cref="MuxerSession.PushPacket"/>
/// (construct directly; <see cref="Payload"/> is borrowed for the call only, no disposal
/// needed) and as output from <see cref="Demuxer.PollPacket"/> (native-owned; the returned
/// instance owns its buffer and must be disposed — <see cref="Payload"/> stays valid,
/// Zero-Copy over the native buffer, only until then).
/// </summary>
public sealed class Packet : IDisposable
{
    private readonly IMemoryOwner<byte> _owner;

    public Packet()
    {
        _owner = EmptyMemoryOwner<byte>.Instance; // Caller-constructed: nothing native to release.
    }

    internal Packet(IMemoryOwner<byte> owner) => _owner = owner;

    /// <summary>Stream / track id this packet belongs to.</summary>
    public required uint StreamId { get; init; }

    /// <summary>Presentation timestamp, in the track's timebase units.</summary>
    public required long Pts { get; init; }

    /// <summary>Decode timestamp, in the track's timebase units.</summary>
    public required long Dts { get; init; }

    /// <summary>Duration, in the track's timebase units.</summary>
    public required ulong Duration { get; init; }

    /// <summary>Whether this packet is a keyframe / random access point.</summary>
    public required bool IsKeyframe { get; init; }

    /// <summary>Whether this packet is outside the active edit window.</summary>
    public required bool IsDiscard { get; init; }

    public required ReadOnlyMemory<byte> Payload { get; init; }

    /// <summary>
    /// Releases the native buffer backing <see cref="Payload"/>, if this instance owns one
    /// (a no-op for a caller-constructed packet pushed via <see cref="MuxerSession.PushPacket"/>).
    /// </summary>
    public void Dispose() => _owner.Dispose();
}
