#if NET8_0_OR_GREATER
using System.Runtime.InteropServices;
using Mediaway.Device;

namespace Mediaway.Device.Desktop.Interop;

internal static unsafe partial class NativeMethods
{
    // ── Desktop video (Screen) ──────────────────────────────────────────────────────

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_desktop_capture_open(
        in NativeDesktopCaptureConfig config, out nint outCapture);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_desktop_capture_geometry(
        DesktopCaptureHandle capture, out uint outWidth, out uint outHeight);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_desktop_capture_poll_frame(
        DesktopCaptureHandle capture, out NativeDesktopFrame outFrame, out byte outHasFrame);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_desktop_capture_release_frame(
        DesktopCaptureHandle capture);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_desktop_capture_close(nint capture);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_desktop_frame_free(ref NativeDesktopFrame frame);

    // ── Desktop audio (Loopback / ProcessLoopback) ──────────────────────────────────

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_desktop_audio_capture_open(
        in NativeDesktopAudioCaptureConfig config, out nint outCapture);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_desktop_audio_capture_poll_frame(
        DesktopAudioCaptureHandle capture, out NativeDesktopAudioFrame outFrame, out byte outHasFrame);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_desktop_audio_capture_close(nint capture);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_desktop_audio_frame_free(ref NativeDesktopAudioFrame frame);
}
#endif
