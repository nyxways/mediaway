using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>Feeds MPEG-TS bytes in and pulls demuxed access units back out.</summary>
public sealed class TsDemuxer : IDisposable
{
    private readonly TsDemuxerHandle _handle;

    public TsDemuxer() => _handle = TsDemuxerHandle.Create();

    /// <summary>Feed bytes — need not be 188-byte aligned across calls.</summary>
    public unsafe void PushBytes(ReadOnlySpan<byte> data)
    {
        fixed (byte* ptr = data)
        {
            MediawayContainerException.ThrowIfError(
                NativeMethods.mediaway_ts_demuxer_push_bytes(_handle, ptr, (nuint)data.Length));
        }
    }

    /// <summary>
    /// Streams whose <c>stream_type</c> maps to a recognized codec (H264/HEVC/AAC/MP3).
    /// Empty until <see cref="PollPacket"/> has actually consumed the PMT (lazy PSI parsing).
    /// </summary>
    public IReadOnlyList<StreamDescriptor> Streams
    {
        get
        {
            nuint count = NativeMethods.mediaway_ts_demuxer_stream_count(_handle);
            var streams = new List<StreamDescriptor>(checked((int)count));
            for (nuint index = 0; index < count; index++)
            {
                MediawayContainerException.ThrowIfError(
                    NativeMethods.mediaway_ts_demuxer_stream_at(_handle, index, out var native));
                streams.Add(NativeConversions.ToManaged(native));
            }

            return streams;
        }
    }

    /// <summary>
    /// Pop the next demuxed packet, if any is ready. A PID with no recognized codec mapping
    /// is silently skipped.
    /// </summary>
    public Packet? PollPacket()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_ts_demuxer_poll_packet(_handle, out var native, out var hasPacket));
        return hasPacket == 0 ? null : NativeConversions.ToManaged(native);
    }

    /// <summary>
    /// Force-emit whatever is still accumulating per PID — call once at the end of a stream
    /// so the very last access unit per PID isn't lost (MPEG-TS only confirms a PES boundary
    /// once the next packet on the same PID starts). Unlike <see cref="PollPacket"/>, the
    /// returned packets own managed copies of their payload, not native-owned buffers — the
    /// native array this reads from is released as a single unit (<c>finish_free</c>), which
    /// does not compose with this assembly's usual one-<c>Dispose</c>-per-packet ownership.
    /// </summary>
    public unsafe IReadOnlyList<Packet> Finish()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_ts_demuxer_finish(_handle, out var outPackets, out var outCount));

        try
        {
            var result = new List<Packet>(checked((int)outCount));
            var array = (NativePacket*)outPackets;
            for (nuint i = 0; i < outCount; i++)
            {
                NativePacket native = array[i];
                byte[] payload = native.PayloadLen == 0
                    ? []
                    : new Span<byte>((void*)native.Payload, checked((int)native.PayloadLen)).ToArray();
                result.Add(new Packet
                {
                    StreamId = native.StreamId,
                    Pts = native.Pts,
                    Dts = native.Dts,
                    Duration = native.Duration,
                    IsKeyframe = native.IsKeyframe != 0,
                    IsDiscard = native.IsDiscard != 0,
                    Payload = payload,
                });
            }

            return result;
        }
        finally
        {
            NativeMethods.mediaway_ts_demuxer_finish_free(outPackets, outCount);
        }
    }

    public void Dispose() => _handle.Dispose();
}
