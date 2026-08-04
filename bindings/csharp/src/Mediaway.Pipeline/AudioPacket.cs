namespace Mediaway.Pipeline;

/// <summary>
/// Output of <see cref="AudioEncoder.PollPacket"/>. <see cref="Payload"/> is already copied
/// out of native memory (see <see cref="AudioEncoder.PollPacket"/>'s own doc comment for why),
/// so unlike <c>Mediaway.Container.Packet</c> this type needs no disposal.
/// </summary>
public sealed record AudioPacket
{
    public required long Pts { get; init; }

    public required long Dts { get; init; }

    public required ulong Duration { get; init; }

    public required bool IsKeyframe { get; init; }

    public required bool IsDiscard { get; init; }

    public required ReadOnlyMemory<byte> Payload { get; init; }
}
