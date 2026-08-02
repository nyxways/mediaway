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

    public void Dispose() => _handle.Dispose();
}
