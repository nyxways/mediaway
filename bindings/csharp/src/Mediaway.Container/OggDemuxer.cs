using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>Feeds Ogg-container bytes in and pulls demuxed packets/stream info back out.</summary>
public sealed class OggDemuxer : IDisposable
{
    private readonly OggDemuxerHandle _handle;

    public OggDemuxer() => _handle = OggDemuxerHandle.Create();

    public unsafe void PushBytes(ReadOnlySpan<byte> data)
    {
        fixed (byte* ptr = data)
        {
            MediawayContainerException.ThrowIfError(
                NativeMethods.mediaway_ogg_demuxer_push_bytes(_handle, ptr, (nuint)data.Length));
        }
    }

    /// <summary>Streams discovered so far — 0 or 1 (Ogg carries a single logical bitstream).</summary>
    public IReadOnlyList<StreamDescriptor> Streams
    {
        get
        {
            nuint count = NativeMethods.mediaway_ogg_demuxer_stream_count(_handle);
            var streams = new List<StreamDescriptor>(checked((int)count));
            for (nuint index = 0; index < count; index++)
            {
                MediawayContainerException.ThrowIfError(
                    NativeMethods.mediaway_ogg_demuxer_stream_at(_handle, index, out var native));
                streams.Add(NativeConversions.ToManaged(native));
            }

            return streams;
        }
    }

    public Packet? PollPacket()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_ogg_demuxer_poll_packet(_handle, out var native, out var hasPacket));
        return hasPacket == 0 ? null : NativeConversions.ToManaged(native);
    }

    public void Dispose() => _handle.Dispose();
}
