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
    internal static partial MediawayPipelineStatus mediaway_encode_session_finish(
        EncodeSessionHandle session, out nint outData, out nuint outLen);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_encode_session_close(nint session);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_pipeline_ffi_buffer_free(nint data, nuint len);
}
#endif
