#if NET8_0_OR_GREATER
using System.Runtime.InteropServices;

namespace Mediaway.Pipeline.Interop;

internal static unsafe partial class NativeMethods
{
    [LibraryImport(LibraryName)]
    internal static partial uint mediaway_pipeline_ffi_abi_version();

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_auto_encoder_open(
        in NativeAutoVideoEncodeConfig config, out nint outEncoder);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_auto_encoder_close(nint encoder);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_encode_session_open(
        AutoEncoderHandle encoder, out nint outSession);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_encode_session_write_frame(
        EncodeSessionHandle session, in NativeVideoFrame frame);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_encode_session_set_bitrate(
        EncodeSessionHandle session, uint bitrateBps);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_encode_session_finish(
        EncodeSessionHandle session, out nint outData, out nuint outLen);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_encode_session_close(nint session);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_pipeline_ffi_buffer_free(nint data, nuint len);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_audio_encoder_open(
        in NativeAudioEncodeConfig config, out nint outSession);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_audio_encode_session_stream_info(
        AudioEncodeSessionHandle session, out NativeAudioStreamInfo outInfo);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_audio_encode_session_push_pcm(
        AudioEncodeSessionHandle session, in NativeAudioFrameView frame);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_audio_encode_session_poll_packet(
        AudioEncodeSessionHandle session, out NativeAudioPacket outPacket, out byte outHasPacket);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_audio_encode_session_flush(
        AudioEncodeSessionHandle session);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_audio_encode_session_close(nint session);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_pipeline_ffi_packet_free(ref NativeAudioPacket packet);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_pipeline_ffi_stream_info_free(ref NativeAudioStreamInfo info);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_decode_session_open(
        in NativeAutoVideoDecodeConfig config, out nint outSession);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_decode_session_push_packet(
        DecodeSessionHandle session, in NativeDecodePacketView packet);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_decode_session_poll_frame(
        DecodeSessionHandle session, out NativeDecodedVideoFrame outFrame, out byte outHasFrame);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_decode_session_flush(DecodeSessionHandle session);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_decode_session_close(nint session);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_decoded_video_frame_free(ref NativeDecodedVideoFrame frame);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_audio_decode_session_open(
        in NativeAudioDecodeConfig config, out nint outSession);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_audio_decode_session_push_packet(
        AudioDecodeSessionHandle session, in NativeDecodePacketView packet);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_audio_decode_session_poll_frame(
        AudioDecodeSessionHandle session, out NativeDecodedAudioFrame outFrame, out byte outHasFrame);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_audio_decode_session_flush(
        AudioDecodeSessionHandle session);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_audio_decode_session_close(nint session);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_decoded_audio_frame_free(ref NativeDecodedAudioFrame frame);

    // ── Capture-to-encode bridge (adr/pipeline/0005-capture-encode-bridge-c-abi.md) ──────

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_encode_session_write_frame_from_camera_capture(
        EncodeSessionHandle session, Mediaway.Device.Camera.Interop.CameraCaptureHandle capture,
        out byte outWroteFrame);

    [LibraryImport(LibraryName)]
    internal static partial MediawayPipelineStatus mediaway_encode_session_write_frame_from_desktop_capture(
        EncodeSessionHandle session, Mediaway.Device.Desktop.Interop.DesktopCaptureHandle capture,
        out byte outWroteFrame);
}
#endif
