#if NET8_0_OR_GREATER
using System.Runtime.InteropServices;
using Mediaway.Device;

namespace Mediaway.Device.Camera.Interop;

internal static unsafe partial class NativeMethods
{
    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_camera_capture_open(
        in NativeCameraCaptureConfig config, out nint outCapture);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_camera_capture_geometry(
        CameraCaptureHandle capture, out uint outWidth, out uint outHeight);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_camera_capture_poll_frame(
        CameraCaptureHandle capture, out NativeCameraFrame outFrame, out byte outHasFrame);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_camera_capture_release_frame(
        CameraCaptureHandle capture);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_camera_capture_close(nint capture);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_camera_frame_free(ref NativeCameraFrame frame);
}
#endif
