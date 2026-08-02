using System.Buffers;
using Mediaway.Common.Interop;
using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>
/// Feeds container bytes in and pulls demuxed packets/stream info back out. Sans-io — never
/// touches a file or socket itself; the caller always owns the bytes.
/// </summary>
public sealed class Demuxer : IDisposable
{
    private readonly DemuxerHandle _handle;

    public Demuxer() => _handle = DemuxerHandle.Create();

    /// <summary>
    /// Feed container bytes in. Can be called incrementally as bytes arrive; the native core
    /// copies <paramref name="data"/> synchronously before returning, so it need not stay
    /// alive past this call.
    /// </summary>
    public unsafe void PushBytes(ReadOnlySpan<byte> data)
    {
        fixed (byte* ptr = data)
        {
            MediawayContainerException.ThrowIfError(
                NativeMethods.mediaway_demuxer_push_bytes(_handle, ptr, (nuint)data.Length));
        }
    }

    /// <summary>Streams/tracks discovered so far — a fresh snapshot on every access.</summary>
    public IReadOnlyList<StreamDescriptor> Streams
    {
        get
        {
            nuint count = NativeMethods.mediaway_demuxer_stream_count(_handle);
            var streams = new List<StreamDescriptor>(checked((int)count));
            for (nuint index = 0; index < count; index++)
            {
                MediawayContainerException.ThrowIfError(
                    NativeMethods.mediaway_demuxer_stream_at(_handle, index, out var native));
                streams.Add(ToManaged(native));
            }

            return streams;
        }
    }

    /// <summary>Pop the next demuxed packet, if any is ready yet.</summary>
    public Packet? PollPacket()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_demuxer_poll_packet(_handle, out var native, out var hasPacket));

        if (hasPacket == 0)
        {
            return null;
        }

        IMemoryOwner<byte> owner = native.PayloadLen == 0
            ? EmptyMemoryOwner<byte>.Instance
            : new NativeOwnedMemoryManager(
                native.Payload, native.PayloadLen,
                static (ptr, len) =>
                {
                    var owned = new NativePacket { Payload = ptr, PayloadLen = len };
                    NativeMethods.mediaway_packet_free(ref owned);
                });

        return new Packet(owner)
        {
            StreamId = native.StreamId,
            Pts = native.Pts,
            Dts = native.Dts,
            Duration = native.Duration,
            IsKeyframe = native.IsKeyframe != 0,
            IsDiscard = native.IsDiscard != 0,
            Payload = owner.Memory,
        };
    }

    /// <summary>
    /// Set the ClearKey decryption key applied to all encrypted tracks on this demuxer. Only
    /// affects samples drained from subsequent <see cref="PushBytes"/> calls, not packets
    /// already sitting in the poll queue. <paramref name="key"/> must be exactly 16 bytes.
    /// </summary>
    public unsafe void SetDecryptionKey(ReadOnlySpan<byte> key)
    {
        fixed (byte* ptr = key)
        {
            MediawayContainerException.ThrowIfError(
                NativeMethods.mediaway_demuxer_set_decryption_key(_handle, ptr, (nuint)key.Length));
        }
    }

    /// <summary>Clear a previously set ClearKey decryption key.</summary>
    public void ClearDecryptionKey() =>
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_demuxer_clear_decryption_key(_handle));

    public void Dispose() => _handle.Dispose();

    private static StreamDescriptor ToManaged(NativeStreamInfo native)
    {
        IMemoryOwner<byte> owner = native.ExtraDataLen == 0
            ? EmptyMemoryOwner<byte>.Instance
            : new NativeOwnedMemoryManager(
                native.ExtraData, native.ExtraDataLen,
                static (ptr, len) =>
                {
                    var owned = new NativeStreamInfo { ExtraData = ptr, ExtraDataLen = len };
                    NativeMethods.mediaway_stream_info_free(ref owned);
                });

        return new StreamDescriptor(owner)
        {
            Id = native.Id,
            Codec = native.Codec,
            TimeBase = native.TimeBase.ToManaged(),
            Geometry = native.HasGeometry != 0 ? new StreamGeometry(native.Width, native.Height) : null,
            SampleRate = native.SampleRate,
            Channels = native.Channels,
            ExtraData = owner.Memory,
        };
    }
}
