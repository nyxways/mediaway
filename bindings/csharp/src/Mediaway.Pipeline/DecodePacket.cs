namespace Mediaway.Pipeline;

/// <summary>
/// Input to <see cref="DecodeSession.PushPacket"/>/<see cref="AudioDecodeSession.PushPacket"/>
/// — a pipeline-scoped packet view, distinct from <c>Mediaway.Container.Packet</c>
/// (<c>adr/pipeline/0006-audio-decode-c-abi.md</c> §4: shared by both video and audio
/// decode). <see cref="Payload"/> is borrowed for the call only.
/// </summary>
public sealed record DecodePacket
{
    public required long Pts { get; init; }

    public long Dts { get; init; }

    /// <summary><c>0</c> if unknown.</summary>
    public ulong Duration { get; init; }

    public bool IsKeyframe { get; init; }

    /// <summary>
    /// For audio: an empty payload is Opus's packet-loss-concealment hint for a lost
    /// frame, not an error — pass it whenever a frame is known lost.
    /// </summary>
    public required ReadOnlyMemory<byte> Payload { get; init; }
}
