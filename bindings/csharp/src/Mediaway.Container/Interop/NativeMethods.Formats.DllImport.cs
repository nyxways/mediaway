#if !NET8_0_OR_GREATER
using System.Runtime.InteropServices;

namespace Mediaway.Container.Interop;

/// <summary>
/// netstandard2.0 mirror of <c>NativeMethods.Formats.LibraryImport.cs</c> — see that file's
/// doc comment. Classic <c>DllImport</c> runtime marshalling; exactly one of the two
/// compiles per target framework.
/// </summary>
internal static unsafe partial class NativeMethods
{
    // ── Ogg ──────────────────────────────────────────────────────────────────
    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_ogg_muxer_create(uint serial);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_ogg_muxer_push_packet(
        OggMuxerHandle muxer, in NativePacketView packet);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_ogg_muxer_flush(OggMuxerHandle muxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_ogg_muxer_poll_bytes(
        OggMuxerHandle muxer, out nint outData, out nuint outLen);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_ogg_muxer_close(nint muxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_ogg_demuxer_create();

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_ogg_demuxer_push_bytes(
        OggDemuxerHandle demuxer, byte* data, nuint len);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nuint mediaway_ogg_demuxer_stream_count(OggDemuxerHandle demuxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_ogg_demuxer_stream_at(
        OggDemuxerHandle demuxer, nuint index, out NativeStreamInfo outInfo);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_ogg_demuxer_poll_packet(
        OggDemuxerHandle demuxer, out NativePacket outPacket, out byte outHasPacket);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_ogg_demuxer_close(nint demuxer);

    // ── ADTS ─────────────────────────────────────────────────────────────────
    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_adts_muxer_create(uint sampleRate, byte channels);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_adts_muxer_push_packet(
        AdtsMuxerHandle muxer, in NativePacketView packet);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_adts_muxer_flush(AdtsMuxerHandle muxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_adts_muxer_poll_bytes(
        AdtsMuxerHandle muxer, out nint outData, out nuint outLen);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_adts_muxer_close(nint muxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_adts_demuxer_create();

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_adts_demuxer_push_bytes(
        AdtsDemuxerHandle demuxer, byte* data, nuint len);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nuint mediaway_adts_demuxer_stream_count(AdtsDemuxerHandle demuxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_adts_demuxer_stream_at(
        AdtsDemuxerHandle demuxer, nuint index, out NativeStreamInfo outInfo);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_adts_demuxer_poll_packet(
        AdtsDemuxerHandle demuxer, out NativePacket outPacket, out byte outHasPacket);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_adts_demuxer_close(nint demuxer);

    // ── FLV ──────────────────────────────────────────────────────────────────
    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_flv_muxer_create();

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_flv_muxer_write_header(
        FlvMuxerHandle muxer, byte hasAudio, byte hasVideo, out nint outData, out nuint outLen);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_flv_muxer_add_video_track(
        FlvMuxerHandle muxer, in NativeVideoTrackInfo info);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_flv_muxer_add_audio_track(
        FlvMuxerHandle muxer, in NativeAudioTrackInfo info);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_flv_muxer_push_packet(
        FlvMuxerHandle muxer, in NativePacketView packet, out nint outData, out nuint outLen);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_flv_muxer_close(nint muxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_flv_demuxer_create();

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_flv_demuxer_push_bytes(
        FlvDemuxerHandle demuxer, byte* data, nuint len);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nuint mediaway_flv_demuxer_stream_count(FlvDemuxerHandle demuxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_flv_demuxer_stream_at(
        FlvDemuxerHandle demuxer, nuint index, out NativeStreamInfo outInfo);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_flv_demuxer_poll_packet(
        FlvDemuxerHandle demuxer, out NativePacket outPacket, out byte outHasPacket);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_flv_demuxer_close(nint demuxer);

    // ── MPEG-TS ──────────────────────────────────────────────────────────────
    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_ts_muxer_create(
        ushort programNumber, ushort pmtPid, NativeTsElementaryStream* streams, nuint streamCount);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_ts_muxer_write_pat_pmt(
        TsMuxerHandle muxer, out nint outData, out nuint outLen);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_ts_muxer_write_access_unit(
        TsMuxerHandle muxer, ushort pid, byte* data, nuint dataLen, ulong pts90k, byte hasDts,
        ulong dts90k, byte randomAccess, out nint outData, out nuint outLen);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_ts_muxer_close(nint muxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_ts_demuxer_create();

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_ts_demuxer_push_bytes(
        TsDemuxerHandle demuxer, byte* data, nuint len);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nuint mediaway_ts_demuxer_stream_count(TsDemuxerHandle demuxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_ts_demuxer_stream_at(
        TsDemuxerHandle demuxer, nuint index, out NativeStreamInfo outInfo);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_ts_demuxer_poll_packet(
        TsDemuxerHandle demuxer, out NativePacket outPacket, out byte outHasPacket);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_ts_demuxer_finish(
        TsDemuxerHandle demuxer, out nint outPackets, out nuint outCount);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_ts_demuxer_finish_free(nint packets, nuint count);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_ts_demuxer_close(nint demuxer);

    // ── MP3 ──────────────────────────────────────────────────────────────────
    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_mp3_muxer_create(in NativeMp3FrameHeader header);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_mp3_muxer_write_frame(
        Mp3MuxerHandle muxer, byte* frameBody, nuint frameBodyLen, byte padding,
        out nint outData, out nuint outLen);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_mp3_muxer_close(nint muxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_mp3_demuxer_create();

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_mp3_demuxer_push_bytes(
        Mp3DemuxerHandle demuxer, byte* data, nuint len);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nuint mediaway_mp3_demuxer_stream_count(Mp3DemuxerHandle demuxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_mp3_demuxer_stream_at(
        Mp3DemuxerHandle demuxer, nuint index, out NativeStreamInfo outInfo);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_mp3_demuxer_poll_packet(
        Mp3DemuxerHandle demuxer, out NativePacket outPacket, out byte outHasPacket);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_mp3_demuxer_close(nint demuxer);

    // ── WAV ──────────────────────────────────────────────────────────────────
    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_wav_muxer_create(uint sampleRate, ushort channels, ushort bitsPerSample);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern nint mediaway_wav_muxer_create_with_format(in NativeWaveFormat format);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_wav_muxer_push_packet(
        WavMuxerHandle muxer, in NativePacketView packet);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_wav_muxer_finish(
        WavMuxerHandle muxer, out nint outData, out nuint outLen);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_wav_muxer_close(nint muxer);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayContainerStatus mediaway_wav_parse(
        byte* data, nuint dataLen, out NativeStreamInfo outInfo, out NativePacket outPacket);
}
#endif
