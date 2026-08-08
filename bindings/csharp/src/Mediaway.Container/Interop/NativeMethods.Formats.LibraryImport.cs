#if NET8_0_OR_GREATER
using System.Runtime.InteropServices;

namespace Mediaway.Container.Interop;

/// <summary>
/// P/Invoke declarations for the 6 dedicated-handle container formats (Ogg, ADTS, FLV,
/// MPEG-TS, MP3, WAV) — kept in their own partial-class file rather than appended to
/// <c>NativeMethods.LibraryImport.cs</c> (MP4/WebM's shared <c>mediaway_muxer_t</c>/
/// <c>mediaway_demuxer_t</c> shape) since none of these six share that shape. Native `bool`
/// parameters are declared `byte` (0/1) here, matching the blittable-struct convention in
/// <c>NativeStructs.cs</c> — <see cref="DisableRuntimeMarshallingAttribute"/> requires an
/// explicit marshaller for `bool` otherwise.
/// </summary>
internal static unsafe partial class NativeMethods
{
    // ── Ogg ──────────────────────────────────────────────────────────────────
    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_ogg_muxer_create(uint serial);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_ogg_muxer_push_packet(
        OggMuxerHandle muxer, in NativePacketView packet);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_ogg_muxer_flush(OggMuxerHandle muxer);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_ogg_muxer_poll_bytes(
        OggMuxerHandle muxer, out nint outData, out nuint outLen);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_ogg_muxer_close(nint muxer);

    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_ogg_demuxer_create();

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_ogg_demuxer_push_bytes(
        OggDemuxerHandle demuxer, byte* data, nuint len);

    [LibraryImport(LibraryName)]
    internal static partial nuint mediaway_ogg_demuxer_stream_count(OggDemuxerHandle demuxer);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_ogg_demuxer_stream_at(
        OggDemuxerHandle demuxer, nuint index, out NativeStreamInfo outInfo);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_ogg_demuxer_poll_packet(
        OggDemuxerHandle demuxer, out NativePacket outPacket, out byte outHasPacket);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_ogg_demuxer_close(nint demuxer);

    // ── ADTS ─────────────────────────────────────────────────────────────────
    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_adts_muxer_create(uint sampleRate, byte channels);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_adts_muxer_push_packet(
        AdtsMuxerHandle muxer, in NativePacketView packet);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_adts_muxer_flush(AdtsMuxerHandle muxer);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_adts_muxer_poll_bytes(
        AdtsMuxerHandle muxer, out nint outData, out nuint outLen);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_adts_muxer_close(nint muxer);

    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_adts_demuxer_create();

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_adts_demuxer_push_bytes(
        AdtsDemuxerHandle demuxer, byte* data, nuint len);

    [LibraryImport(LibraryName)]
    internal static partial nuint mediaway_adts_demuxer_stream_count(AdtsDemuxerHandle demuxer);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_adts_demuxer_stream_at(
        AdtsDemuxerHandle demuxer, nuint index, out NativeStreamInfo outInfo);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_adts_demuxer_poll_packet(
        AdtsDemuxerHandle demuxer, out NativePacket outPacket, out byte outHasPacket);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_adts_demuxer_close(nint demuxer);

    // ── FLV ──────────────────────────────────────────────────────────────────
    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_flv_muxer_create();

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_flv_muxer_write_header(
        FlvMuxerHandle muxer, byte hasAudio, byte hasVideo, out nint outData, out nuint outLen);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_flv_muxer_add_video_track(
        FlvMuxerHandle muxer, in NativeVideoTrackInfo info);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_flv_muxer_add_audio_track(
        FlvMuxerHandle muxer, in NativeAudioTrackInfo info);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_flv_muxer_push_packet(
        FlvMuxerHandle muxer, in NativePacketView packet, out nint outData, out nuint outLen);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_flv_muxer_close(nint muxer);

    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_flv_demuxer_create();

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_flv_demuxer_push_bytes(
        FlvDemuxerHandle demuxer, byte* data, nuint len);

    [LibraryImport(LibraryName)]
    internal static partial nuint mediaway_flv_demuxer_stream_count(FlvDemuxerHandle demuxer);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_flv_demuxer_stream_at(
        FlvDemuxerHandle demuxer, nuint index, out NativeStreamInfo outInfo);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_flv_demuxer_poll_packet(
        FlvDemuxerHandle demuxer, out NativePacket outPacket, out byte outHasPacket);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_flv_demuxer_close(nint demuxer);

    // ── MPEG-TS ──────────────────────────────────────────────────────────────
    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_ts_muxer_create(
        ushort programNumber, ushort pmtPid, NativeTsElementaryStream* streams, nuint streamCount);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_ts_muxer_write_pat_pmt(
        TsMuxerHandle muxer, out nint outData, out nuint outLen);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_ts_muxer_write_access_unit(
        TsMuxerHandle muxer, ushort pid, byte* data, nuint dataLen, ulong pts90k, byte hasDts,
        ulong dts90k, byte randomAccess, out nint outData, out nuint outLen);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_ts_muxer_close(nint muxer);

    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_ts_demuxer_create();

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_ts_demuxer_push_bytes(
        TsDemuxerHandle demuxer, byte* data, nuint len);

    [LibraryImport(LibraryName)]
    internal static partial nuint mediaway_ts_demuxer_stream_count(TsDemuxerHandle demuxer);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_ts_demuxer_stream_at(
        TsDemuxerHandle demuxer, nuint index, out NativeStreamInfo outInfo);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_ts_demuxer_poll_packet(
        TsDemuxerHandle demuxer, out NativePacket outPacket, out byte outHasPacket);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_ts_demuxer_finish(
        TsDemuxerHandle demuxer, out nint outPackets, out nuint outCount);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_ts_demuxer_finish_free(nint packets, nuint count);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_ts_demuxer_close(nint demuxer);

    // ── MP3 ──────────────────────────────────────────────────────────────────
    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_mp3_muxer_create(in NativeMp3FrameHeader header);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_mp3_muxer_write_frame(
        Mp3MuxerHandle muxer, byte* frameBody, nuint frameBodyLen, byte padding,
        out nint outData, out nuint outLen);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_mp3_muxer_close(nint muxer);

    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_mp3_demuxer_create();

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_mp3_demuxer_push_bytes(
        Mp3DemuxerHandle demuxer, byte* data, nuint len);

    [LibraryImport(LibraryName)]
    internal static partial nuint mediaway_mp3_demuxer_stream_count(Mp3DemuxerHandle demuxer);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_mp3_demuxer_stream_at(
        Mp3DemuxerHandle demuxer, nuint index, out NativeStreamInfo outInfo);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_mp3_demuxer_poll_packet(
        Mp3DemuxerHandle demuxer, out NativePacket outPacket, out byte outHasPacket);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_mp3_demuxer_close(nint demuxer);

    // ── WAV ──────────────────────────────────────────────────────────────────
    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_wav_muxer_create(uint sampleRate, ushort channels, ushort bitsPerSample);

    [LibraryImport(LibraryName)]
    internal static partial nint mediaway_wav_muxer_create_with_format(in NativeWaveFormat format);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_wav_muxer_push_packet(
        WavMuxerHandle muxer, in NativePacketView packet);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_wav_muxer_finish(
        WavMuxerHandle muxer, out nint outData, out nuint outLen);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_wav_muxer_close(nint muxer);

    [LibraryImport(LibraryName)]
    internal static partial MediawayContainerStatus mediaway_wav_parse(
        byte* data, nuint dataLen, out NativeStreamInfo outInfo, out NativePacket outPacket);
}
#endif
