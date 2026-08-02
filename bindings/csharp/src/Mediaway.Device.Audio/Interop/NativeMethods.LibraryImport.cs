#if NET8_0_OR_GREATER
using System.Runtime.InteropServices;
using Mediaway.Device;

namespace Mediaway.Device.Audio.Interop;

internal static unsafe partial class NativeMethods
{
    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_audio_capture_open(
        in NativeAudioCaptureConfig config, out nint outCapture);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_audio_capture_poll_frame(
        AudioCaptureHandle capture, out NativeDeviceAudioFrame outFrame, out byte outHasFrame);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_audio_capture_close(nint capture);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_audio_frame_free(ref NativeDeviceAudioFrame frame);
}
#endif
