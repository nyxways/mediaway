using System.Buffers;
using Mediaway.Common.Interop;
using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>
/// A muxer that has begun streaming — obtained from <see cref="Muxer.Begin"/>, never
/// constructed directly. Wraps the same native handle <see cref="Muxer"/> created; that
/// muxer instance is inert from this point on.
/// </summary>
public sealed class MuxerSession : IDisposable
{
    private readonly MuxerHandle _handle;

    internal MuxerSession(MuxerHandle handle) => _handle = handle;

    /// <summary>Push one packet. <see cref="Packet.Payload"/> is borrowed for the call only.</summary>
    public unsafe void PushPacket(Packet packet)
    {
        using var pin = packet.Payload.Pin();
        var native = new NativePacketView
        {
            StreamId = packet.StreamId,
            Pts = packet.Pts,
            Dts = packet.Dts,
            Duration = packet.Duration,
            IsKeyframe = (byte)(packet.IsKeyframe ? 1 : 0),
            IsDiscard = (byte)(packet.IsDiscard ? 1 : 0),
            Payload = packet.Payload.IsEmpty ? null : (byte*)pin.Pointer,
            PayloadLen = (nuint)packet.Payload.Length,
        };
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_muxer_push_packet(_handle, in native));
    }

    /// <summary>Flush any pending fragments so they become available via <see cref="PollBytes"/>.</summary>
    public void Flush() =>
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_muxer_flush(_handle));

    /// <summary>
    /// Drain whatever muxed container bytes are ready right now — Zero-Copy over the native
    /// buffer; dispose the returned owner to release it. Empty (already-disposed no-op
    /// owner) when nothing is ready yet — not an error.
    /// </summary>
    public IMemoryOwner<byte> PollBytes()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_muxer_poll_bytes(_handle, out var data, out var len));

        if (data == 0 || len == 0)
        {
            return EmptyMemoryOwner<byte>.Instance;
        }

        return new NativeOwnedMemoryManager(
            data, len, static (ptr, l) => NativeMethods.mediaway_buffer_free(ptr, l));
    }

    /// <summary>
    /// Take the placements recorded since the last call, in write order: where each sample's payload
    /// landed in the bytes <see cref="PollBytes"/> returned. Empty when nothing was recorded.
    /// A placement's bytes are available from <see cref="PollBytes"/> by the time the placement is.
    /// </summary>
    /// <remarks>
    /// Write order is not push order once there are two tracks: a fragment writes its samples
    /// grouped by track. Match a placement to its packet by (<see cref="Placement.TrackId"/>, dts),
    /// never by position.
    /// </remarks>
    /// <exception cref="MediawayContainerException"><see cref="MediawayContainerStatus.InvalidState"/>
    /// on a WebM muxer, which never records. A plain MP4 muxer (not created by
    /// <see cref="Muxer.CreateWithPlacements"/>) records nothing and returns an empty list, the same
    /// answer the Rust API gives.</exception>
    public unsafe IReadOnlyList<Placement> PollPlacements()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_muxer_poll_placements(_handle, out var rows, out var count));

        if (rows == 0 || count == 0)
        {
            return Array.Empty<Placement>();
        }

        try
        {
            var native = new ReadOnlySpan<NativePlacement>((void*)rows, checked((int)count));
            var result = new Placement[native.Length];
            for (var i = 0; i < result.Length; i++)
            {
                result[i] = new Placement(native[i].TrackId, native[i].Dts, native[i].Offset, native[i].Len);
            }

            return result;
        }
        finally
        {
            NativeMethods.mediaway_placements_free(rows, count);
        }
    }

    public void Dispose() => _handle.Dispose();
}
