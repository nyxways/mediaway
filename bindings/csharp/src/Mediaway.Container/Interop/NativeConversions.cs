using System.Buffers;
using Mediaway.Common.Interop;

namespace Mediaway.Container.Interop;

/// <summary>
/// Shared native-owned-output → managed conversions, reused by every demuxer in this
/// assembly (MP4/WebM's <see cref="Container.Demuxer"/> plus the 6 dedicated-handle
/// demuxers) — <c>mediaway_packet_t</c>/<c>mediaway_stream_info_t</c> and their frees are
/// identical across every format's C ABI.
/// </summary>
internal static class NativeConversions
{
    internal static StreamDescriptor ToManaged(NativeStreamInfo native)
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

    internal static Packet ToManaged(NativePacket native)
    {
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
}
