using System.Buffers;
using Mediaway.Common.Interop;
using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>
/// Muxes access units into MPEG-TS packets for one program. Unlike <see cref="Muxer"/>, the
/// full elementary-stream list is fixed at construction (no <c>add_track</c> after) and
/// every write method returns its own freshly allocated output buffer directly.
/// </summary>
public sealed class TsMuxer : IDisposable
{
    private readonly TsMuxerHandle _handle;

    /// <param name="streams">
    /// Elementary streams for this program's PMT. <paramref name="pmtPid"/> and every
    /// stream's <see cref="TsElementaryStream.Pid"/> must be in <c>2..=0x1FFF</c>; every
    /// stream's codec must be H264/HEVC/AAC/MP3.
    /// </param>
    public unsafe TsMuxer(ushort programNumber, ushort pmtPid, IReadOnlyList<TsElementaryStream> streams)
    {
        var native = new NativeTsElementaryStream[streams.Count];
        for (int i = 0; i < streams.Count; i++)
        {
            native[i] = new NativeTsElementaryStream { Pid = streams[i].Pid, Codec = streams[i].Codec };
        }

        fixed (NativeTsElementaryStream* ptr = native)
        {
            _handle = TsMuxerHandle.Create(programNumber, pmtPid, ptr, (nuint)native.Length);
        }
    }

    /// <summary>
    /// Write PAT + PMT packets. Call once at the start and periodically thereafter — real
    /// players expect PAT/PMT to repeat.
    /// </summary>
    public IMemoryOwner<byte> WritePatPmt()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_ts_muxer_write_pat_pmt(_handle, out var data, out var len));
        return Wrap(data, len);
    }

    /// <summary>
    /// Packetize one access unit for <paramref name="pid"/> into PES + TS packets.
    /// <paramref name="pts90k"/>/<paramref name="dts90k"/> are the real MPEG-TS 90 kHz clock
    /// values, not a track-relative timebase; <paramref name="dts90k"/> <c>null</c> means "no DTS".
    /// </summary>
    public unsafe IMemoryOwner<byte> WriteAccessUnit(
        ushort pid, ReadOnlySpan<byte> data, ulong pts90k, ulong? dts90k, bool randomAccess)
    {
        fixed (byte* ptr = data)
        {
            MediawayContainerException.ThrowIfError(NativeMethods.mediaway_ts_muxer_write_access_unit(
                _handle, pid, ptr, (nuint)data.Length, pts90k, (byte)(dts90k.HasValue ? 1 : 0),
                dts90k ?? 0, (byte)(randomAccess ? 1 : 0), out var outData, out var outLen));
            return Wrap(outData, outLen);
        }
    }

    public void Dispose() => _handle.Dispose();

    private static IMemoryOwner<byte> Wrap(nint data, nuint len) =>
        data == 0 || len == 0
            ? EmptyMemoryOwner<byte>.Instance
            : new NativeOwnedMemoryManager(data, len, static (ptr, l) => NativeMethods.mediaway_buffer_free(ptr, l));
}
