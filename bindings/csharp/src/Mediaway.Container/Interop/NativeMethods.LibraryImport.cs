#if NET8_0_OR_GREATER
using System.Runtime.InteropServices;

namespace Mediaway.Container.Interop;

internal static unsafe partial class NativeMethods
{
    [LibraryImport(LibraryName)]
    internal static partial uint mediaway_container_ffi_abi_version();

    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_muxer_create();

    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_muxer_create_for_format(ContainerFormat format);

    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_muxer_create_with_fragment_batch(nuint batch);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_muxer_add_video_track(
        MuxerHandle muxer, in NativeVideoTrackInfo info);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_muxer_add_audio_track(
        MuxerHandle muxer, in NativeAudioTrackInfo info);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_muxer_begin(MuxerHandle muxer);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_muxer_push_packet(
        MuxerHandle muxer, in NativePacketView packet);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_muxer_flush(MuxerHandle muxer);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_muxer_poll_bytes(
        MuxerHandle muxer, out nint outData, out nuint outLen);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_muxer_close(nint muxer);

    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_demuxer_create();

    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_demuxer_create_for_format(ContainerFormat format);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_demuxer_push_bytes(
        DemuxerHandle demuxer, byte* data, nuint len);

    [LibraryImport(LibraryName)]
    internal static partial nuint mediaway_demuxer_stream_count(DemuxerHandle demuxer);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_demuxer_stream_at(
        DemuxerHandle demuxer, nuint index, out NativeStreamInfo outInfo);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_demuxer_poll_packet(
        DemuxerHandle demuxer, out NativePacket outPacket, out byte outHasPacket);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_demuxer_set_decryption_key(
        DemuxerHandle demuxer, byte* key, nuint keyLen);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_demuxer_clear_decryption_key(
        DemuxerHandle demuxer);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_demuxer_close(nint demuxer);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_buffer_free(nint data, nuint len);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_packet_free(ref NativePacket packet);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_stream_info_free(ref NativeStreamInfo info);
}
#endif
