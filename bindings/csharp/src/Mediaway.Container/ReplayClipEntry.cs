namespace Mediaway.Container;

/// <summary>
/// One packet of a <see cref="ReplayClip"/>. <see cref="Pts"/> and <see cref="Dts"/> are rebased so
/// the cut keyframe decodes at zero.
/// </summary>
/// <param name="StreamId">Stream / track id.</param>
/// <param name="Pts">Presentation timestamp, rebased.</param>
/// <param name="Dts">Decode timestamp, rebased.</param>
/// <param name="Duration">Duration in the stream's timebase.</param>
/// <param name="IsKeyframe">Whether this is a keyframe.</param>
/// <param name="IsDiscard">Whether this packet is outside the active edit window.</param>
/// <param name="Kind">Which of <paramref name="Payload"/> / <paramref name="Stored"/> is meaningful.</param>
/// <param name="Payload">
/// <see cref="ReplayPayloadKind.Bytes"/> only: the payload, <b>borrowed</b> from the clip (Zero-Copy)
/// and valid until the <see cref="ReplayClip"/> is disposed; empty for a stored entry. Reading it
/// after the clip is disposed throws <see cref="ObjectDisposedException"/>.
/// </param>
/// <param name="Stored"><see cref="ReplayPayloadKind.Stored"/> only: where the bytes are.</param>
public readonly record struct ReplayClipEntry(
    uint StreamId,
    long Pts,
    long Dts,
    ulong Duration,
    bool IsKeyframe,
    bool IsDiscard,
    ReplayPayloadKind Kind,
    ReadOnlyMemory<byte> Payload,
    StoredPayload? Stored)
{
    /// <summary>
    /// This entry as a <see cref="Packet"/> for <see cref="MuxerSession.PushPacket"/>: the
    /// composition that writes a clip out as a standalone file. The packet's payload is the borrowed
    /// view above, so push it before the clip is disposed; the packet needs no disposal of its own.
    /// </summary>
    /// <exception cref="InvalidOperationException">A stored entry has no payload to push; read the
    /// bytes back from <see cref="Stored"/> first.</exception>
    public Packet ToPacket() =>
        Kind == ReplayPayloadKind.Stored
            ? throw new InvalidOperationException(
                "A stored replay entry carries a location, not bytes: read them back from Stored " +
                "and build a Packet yourself.")
            : new Packet
            {
                StreamId = StreamId,
                Pts = Pts,
                Dts = Dts,
                Duration = Duration,
                IsKeyframe = IsKeyframe,
                IsDiscard = IsDiscard,
                Payload = Payload,
            };
}
