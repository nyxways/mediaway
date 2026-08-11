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
    internal static extern MediawayPipelineStatus mediaway_encode_session_set_bitrate(
        EncodeSessionHandle session, uint bitrateBps);

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

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_decode_session_open(
        in NativeAutoVideoDecodeConfig config, out nint outSession);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_decode_session_push_packet(
        DecodeSessionHandle session, in NativeDecodePacketView packet);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_decode_session_poll_frame(
        DecodeSessionHandle session, out NativeDecodedVideoFrame outFrame, out byte outHasFrame);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_decode_session_flush(DecodeSessionHandle session);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_decode_session_close(nint session);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_decoded_video_frame_free(ref NativeDecodedVideoFrame frame);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_audio_decode_session_open(
        in NativeAudioDecodeConfig config, out nint outSession);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_audio_decode_session_push_packet(
        AudioDecodeSessionHandle session, in NativeDecodePacketView packet);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_audio_decode_session_poll_frame(
        AudioDecodeSessionHandle session, out NativeDecodedAudioFrame outFrame, out byte outHasFrame);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_audio_decode_session_flush(
        AudioDecodeSessionHandle session);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_audio_decode_session_close(nint session);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_decoded_audio_frame_free(ref NativeDecodedAudioFrame frame);

    // ── Capture-to-encode bridge (adr/pipeline/0005-capture-encode-bridge-c-abi.md) ──────

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_encode_session_write_frame_from_camera_capture(
        EncodeSessionHandle session, Mediaway.Device.Camera.Interop.CameraCaptureHandle capture,
        out byte outWroteFrame);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayPipelineStatus mediaway_encode_session_write_frame_from_desktop_capture(
        EncodeSessionHandle session, Mediaway.Device.Desktop.Interop.DesktopCaptureHandle capture,
        out byte outWroteFrame);
}
#endif
