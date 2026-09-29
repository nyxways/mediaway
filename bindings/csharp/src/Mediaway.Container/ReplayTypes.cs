namespace Mediaway.Container;

/// <summary>What a <see cref="ReplayRing"/> holds for each packet. Mirrors
/// <c>mediaway_replay_payload_kind_t</c>.</summary>
public enum ReplayPayloadKind
{
    /// <summary>
    /// The payload bytes, copied in by <see cref="ReplayRing.Push(Packet)"/> and shared with every
    /// clip by reference count.
    /// </summary>
    Bytes = 0,

    /// <summary>
    /// Only where the bytes are (<see cref="ReplayRing.PushStored"/>): the caller keeps them in a
    /// file it already writes, and reads them back itself when it saves a clip.
    /// </summary>
    Stored = 1,
}

/// <summary>A packet's metadata without its payload — input to
/// <see cref="ReplayRing.PushStored"/> and <see cref="ReplayRing.Push(in PacketMeta, ReadOnlySpan{byte})"/>.</summary>
/// <param name="StreamId">Stream / track id.</param>
/// <param name="Pts">Presentation timestamp in the stream's timebase.</param>
/// <param name="Dts">Decode timestamp in the stream's timebase. Must not go backwards within a stream.</param>
/// <param name="Duration">Duration in the stream's timebase.</param>
/// <param name="IsKeyframe">Whether this is a keyframe / random access point.</param>
/// <param name="IsDiscard">Whether this packet is outside the active edit window.</param>
public readonly record struct PacketMeta(
    uint StreamId, long Pts, long Dts, ulong Duration, bool IsKeyframe, bool IsDiscard);

/// <summary>Where a packet's payload was stored on the caller's disk.</summary>
/// <param name="File">The caller's own id for the file the bytes are in.</param>
/// <param name="Offset">Byte offset of the payload in that file.</param>
/// <param name="Length">Payload length in bytes.</param>
public readonly record struct StoredPayload(uint File, ulong Offset, uint Length);

/// <summary>
/// Where one sample's payload landed in an MP4 muxer's output, from
/// <see cref="MuxerSession.PollPlacements"/>.
/// </summary>
/// <param name="TrackId">The packet's stream id (the caller's id, not the ISOBMFF <c>track_ID</c>).</param>
/// <param name="Dts">The sample's decode timestamp, as pushed.</param>
/// <param name="Offset">
/// Absolute byte offset of the payload in the muxer's output: counted from the first byte
/// <see cref="MuxerSession.PollBytes"/> ever returned, so it is the file offset when every polled
/// byte is written sequentially from 0.
/// </param>
/// <param name="Length">
/// Payload length in bytes <b>as written</b>: H.264/HEVC Annex-B becomes length-prefixed and AAC
/// loses its ADTS header, so it can differ from the pushed payload's length.
/// </param>
public readonly record struct Placement(uint TrackId, long Dts, ulong Offset, uint Length);
