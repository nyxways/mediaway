using System.Buffers;
using Mediaway.Common.Interop;
using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>
/// Muxes packets into Ogg pages for one logical bitstream. Unlike <see cref="Muxer"/>, Ogg
/// has no track-registration step or Open→Live typestate — ready for
/// <see cref="PushPacket"/> immediately after construction.
/// </summary>
public sealed class OggMuxer : IDisposable
{
    private readonly OggMuxerHandle _handle;

    /// <param name="serial">Logical bitstream serial number.</param>
    public OggMuxer(uint serial) => _handle = OggMuxerHandle.Create(serial);

    /// <summary>
    /// Write one Ogg page containing <paramref name="packet"/>'s payload.
    /// <see cref="Packet.Pts"/> becomes the page's granule position;
    /// <see cref="Packet.IsDiscard"/> becomes the page's EOS flag. Fails with
    /// <see cref="MediawayContainerStatus.InvalidData"/> if the payload exceeds a single
    /// Ogg page's capacity — this mux always emits one page per packet.
    /// </summary>
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
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_ogg_muxer_push_packet(_handle, in native));
    }

    /// <summary>
    /// No-op — every <see cref="PushPacket"/> call already wrote a complete, independently
    /// valid Ogg page. Exposed for shape parity with <see cref="MuxerSession.Flush"/>.
    /// </summary>
    public void Flush() =>
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_ogg_muxer_flush(_handle));

    /// <summary>Drain whatever muxed Ogg page bytes are ready right now — Zero-Copy over the native buffer.</summary>
    public IMemoryOwner<byte> PollBytes()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_ogg_muxer_poll_bytes(_handle, out var data, out var len));

        if (data == 0 || len == 0)
        {
            return EmptyMemoryOwner<byte>.Instance;
        }

        return new NativeOwnedMemoryManager(data, len, static (ptr, l) => NativeMethods.mediaway_buffer_free(ptr, l));
    }

    public void Dispose() => _handle.Dispose();
}
