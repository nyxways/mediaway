using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>Feeds ADTS elementary-stream bytes in and pulls demuxed AAC frames back out.</summary>
public sealed class AdtsDemuxer : IDisposable
{
    private readonly AdtsDemuxerHandle _handle;

    public AdtsDemuxer() => _handle = AdtsDemuxerHandle.Create();

    public unsafe void PushBytes(ReadOnlySpan<byte> data)
    {
        fixed (byte* ptr = data)
        {
            MediawayContainerException.ThrowIfError(
                NativeMethods.mediaway_adts_demuxer_push_bytes(_handle, ptr, (nuint)data.Length));
        }
    }

    /// <summary>Streams discovered so far — 0 or 1 (ADTS carries a single implicit stream).</summary>
    public IReadOnlyList<StreamDescriptor> Streams
    {
        get
        {
            nuint count = NativeMethods.mediaway_adts_demuxer_stream_count(_handle);
            var streams = new List<StreamDescriptor>(checked((int)count));
            for (nuint index = 0; index < count; index++)
            {
                MediawayContainerException.ThrowIfError(
                    NativeMethods.mediaway_adts_demuxer_stream_at(_handle, index, out var native));
                streams.Add(NativeConversions.ToManaged(native));
            }

            return streams;
        }
    }

    /// <summary>
    /// Pop the next demuxed AAC frame, if any is ready. Pts/duration are synthesized from a
    /// running 1024-samples-per-frame count — ADTS carries no per-frame timing of its own.
    /// </summary>
    public Packet? PollPacket()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_adts_demuxer_poll_packet(_handle, out var native, out var hasPacket));
        return hasPacket == 0 ? null : NativeConversions.ToManaged(native);
    }

    public void Dispose() => _handle.Dispose();
}
