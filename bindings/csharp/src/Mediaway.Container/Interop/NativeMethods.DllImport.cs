#if !NET8_0_OR_GREATER
using System.Runtime.InteropServices;

namespace Mediaway.Container.Interop;

internal static unsafe partial class NativeMethods
{
    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern uint mediaway_container_ffi_abi_version();

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_muxer_create();

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_muxer_create_with_fragment_batch(nuint batch);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_muxer_add_video_track(
        MuxerHandle muxer, in NativeVideoTrackInfo info);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_muxer_add_audio_track(
        MuxerHandle muxer, in NativeAudioTrackInfo info);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_muxer_begin(MuxerHandle muxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_muxer_push_packet(
        MuxerHandle muxer, in NativePacketView packet);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_muxer_flush(MuxerHandle muxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_muxer_poll_bytes(
        MuxerHandle muxer, out nint outData, out nuint outLen);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_muxer_close(nint muxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_demuxer_create();

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_demuxer_push_bytes(
        DemuxerHandle demuxer, byte* data, nuint len);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nuint mediaway_demuxer_stream_count(DemuxerHandle demuxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_demuxer_stream_at(
        DemuxerHandle demuxer, nuint index, out NativeStreamInfo outInfo);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_demuxer_poll_packet(
        DemuxerHandle demuxer, out NativePacket outPacket, out byte outHasPacket);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_demuxer_set_decryption_key(
        DemuxerHandle demuxer, byte* key, nuint keyLen);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_demuxer_clear_decryption_key(
        DemuxerHandle demuxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_demuxer_close(nint demuxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_buffer_free(nint data, nuint len);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_packet_free(ref NativePacket packet);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_stream_info_free(ref NativeStreamInfo info);
}
#endif
