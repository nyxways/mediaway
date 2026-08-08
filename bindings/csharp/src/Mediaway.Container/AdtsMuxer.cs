using System.Buffers;
using Mediaway.Common.Interop;
using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>
/// Wraps raw AAC frames in ADTS headers. No track-registration step or Open→Live
/// typestate — ready for <see cref="PushPacket"/> immediately after construction.
/// </summary>
public sealed class AdtsMuxer : IDisposable
{
    private readonly AdtsMuxerHandle _handle;

    /// <param name="sampleRate">Must be a standard ADTS sample rate.</param>
    public AdtsMuxer(uint sampleRate, byte channels) => _handle = AdtsMuxerHandle.Create(sampleRate, channels);

    /// <summary>
    /// Append one AAC frame (raw, ADTS header added) from <paramref name="packet"/>'s
    /// payload. Fails with <see cref="MediawayContainerStatus.InvalidPacket"/> if the
    /// payload is too large for ADTS's 13-bit frame-length field.
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
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_adts_muxer_push_packet(_handle, in native));
    }

    /// <summary>
    /// No-op — ADTS frames are independently appendable; nothing is buffered beyond what
    /// <see cref="PollBytes"/> already exposes. Exposed for shape parity with
    /// <see cref="MuxerSession.Flush"/>.
    /// </summary>
    public void Flush() =>
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_adts_muxer_flush(_handle));

    /// <summary>Drain whatever muxed ADTS bytes are ready right now — Zero-Copy over the native buffer.</summary>
    public IMemoryOwner<byte> PollBytes()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_adts_muxer_poll_bytes(_handle, out var data, out var len));

        if (data == 0 || len == 0)
        {
            return EmptyMemoryOwner<byte>.Instance;
        }

        return new NativeOwnedMemoryManager(data, len, static (ptr, l) => NativeMethods.mediaway_buffer_free(ptr, l));
    }

    public void Dispose() => _handle.Dispose();
}
