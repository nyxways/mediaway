#if !NET8_0_OR_GREATER
using System.Runtime.InteropServices;
using Mediaway.Device;

namespace Mediaway.Device.Desktop.Interop;

internal static unsafe partial class NativeMethods
{
    // ── Desktop video (Screen) ──────────────────────────────────────────────────────

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_desktop_capture_open(
        in NativeDesktopCaptureConfig config, out nint outCapture);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_desktop_capture_geometry(
        DesktopCaptureHandle capture, out uint outWidth, out uint outHeight);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_desktop_capture_poll_frame(
        DesktopCaptureHandle capture, out NativeDesktopFrame outFrame, out byte outHasFrame);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_desktop_capture_release_frame(
        DesktopCaptureHandle capture);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_desktop_capture_close(nint capture);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_desktop_frame_free(ref NativeDesktopFrame frame);

    // ── Desktop audio (Loopback / ProcessLoopback) ──────────────────────────────────

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_desktop_audio_capture_open(
        in NativeDesktopAudioCaptureConfig config, out nint outCapture);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_desktop_audio_capture_poll_frame(
        DesktopAudioCaptureHandle capture, out NativeDesktopAudioFrame outFrame, out byte outHasFrame);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_desktop_audio_capture_close(nint capture);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_desktop_audio_frame_free(ref NativeDesktopAudioFrame frame);
}
#endif
