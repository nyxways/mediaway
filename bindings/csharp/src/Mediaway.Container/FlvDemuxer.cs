using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>Feeds FLV-container bytes in and pulls demuxed packets/stream info back out.</summary>
public sealed class FlvDemuxer : IDisposable
{
    private readonly FlvDemuxerHandle _handle;

    public FlvDemuxer() => _handle = FlvDemuxerHandle.Create();

    public unsafe void PushBytes(ReadOnlySpan<byte> data)
    {
        fixed (byte* ptr = data)
        {
            MediawayContainerException.ThrowIfError(
                NativeMethods.mediaway_flv_demuxer_push_bytes(_handle, ptr, (nuint)data.Length));
        }
    }

    /// <summary>Streams recognized so far — 0, 1, or 2 (fixed video-then-audio slots).</summary>
    public IReadOnlyList<StreamDescriptor> Streams
    {
        get
        {
            nuint count = NativeMethods.mediaway_flv_demuxer_stream_count(_handle);
            var streams = new List<StreamDescriptor>(checked((int)count));
            for (nuint index = 0; index < count; index++)
            {
                MediawayContainerException.ThrowIfError(
                    NativeMethods.mediaway_flv_demuxer_stream_at(_handle, index, out var native));
                streams.Add(NativeConversions.ToManaged(native));
            }

            return streams;
        }
    }

    /// <summary>
    /// Pop the next demuxed packet, if any is ready. Sequence-header tags (AVC/AAC config)
    /// update the matching stream's extra data internally and are not themselves returned.
    /// </summary>
    public Packet? PollPacket()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_flv_demuxer_poll_packet(_handle, out var native, out var hasPacket));
        return hasPacket == 0 ? null : NativeConversions.ToManaged(native);
    }

    public void Dispose() => _handle.Dispose();
}
