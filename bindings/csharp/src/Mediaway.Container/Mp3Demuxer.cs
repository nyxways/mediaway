using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>Feeds MPEG audio elementary-stream bytes in and pulls demuxed Layer III frames back out.</summary>
public sealed class Mp3Demuxer : IDisposable
{
    private readonly Mp3DemuxerHandle _handle;

    public Mp3Demuxer() => _handle = Mp3DemuxerHandle.Create();

    public unsafe void PushBytes(ReadOnlySpan<byte> data)
    {
        fixed (byte* ptr = data)
        {
            MediawayContainerException.ThrowIfError(
                NativeMethods.mediaway_mp3_demuxer_push_bytes(_handle, ptr, (nuint)data.Length));
        }
    }

    /// <summary>Streams discovered so far — 0 or 1 (MP3 carries a single implicit stream).</summary>
    public IReadOnlyList<StreamDescriptor> Streams
    {
        get
        {
            nuint count = NativeMethods.mediaway_mp3_demuxer_stream_count(_handle);
            var streams = new List<StreamDescriptor>(checked((int)count));
            for (nuint index = 0; index < count; index++)
            {
                MediawayContainerException.ThrowIfError(
                    NativeMethods.mediaway_mp3_demuxer_stream_at(_handle, index, out var native));
                streams.Add(NativeConversions.ToManaged(native));
            }

            return streams;
        }
    }

    /// <summary>
    /// Pop the next demuxed Layer III frame, if any is ready. Pts/duration are synthesized
    /// from a running samples-per-frame count — MPEG audio carries no per-frame timing of
    /// its own.
    /// </summary>
    public Packet? PollPacket()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_mp3_demuxer_poll_packet(_handle, out var native, out var hasPacket));
        return hasPacket == 0 ? null : NativeConversions.ToManaged(native);
    }

    public void Dispose() => _handle.Dispose();
}
