#if !NET8_0_OR_GREATER
using System.Runtime.InteropServices;

namespace Mediaway.Pipeline.Interop;

internal static unsafe partial class NativeMethods
{
    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern uint mediaway_pipeline_ffi_abi_version();

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_auto_encoder_open(
        in NativeAutoVideoEncodeConfig config, out nint outEncoder);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_auto_encoder_close(nint encoder);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_encode_session_open(
        AutoEncoderHandle encoder, out nint outSession);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_encode_session_write_frame(
        EncodeSessionHandle session, in NativeVideoFrame frame);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_encode_session_finish(
        EncodeSessionHandle session, out nint outData, out nuint outLen);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_encode_session_close(nint session);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_pipeline_ffi_buffer_free(nint data, nuint len);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_audio_encoder_open(
        in NativeAudioEncodeConfig config, out nint outSession);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_audio_encode_session_stream_info(
        AudioEncodeSessionHandle session, out NativeAudioStreamInfo outInfo);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_audio_encode_session_push_pcm(
        AudioEncodeSessionHandle session, in NativeAudioFrameView frame);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_audio_encode_session_poll_packet(
        AudioEncodeSessionHandle session, out NativeAudioPacket outPacket, out byte outHasPacket);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_audio_encode_session_flush(
        AudioEncodeSessionHandle session);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_audio_encode_session_close(nint session);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_pipeline_ffi_packet_free(ref NativeAudioPacket packet);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_pipeline_ffi_stream_info_free(ref NativeAudioStreamInfo info);
}
#endif
