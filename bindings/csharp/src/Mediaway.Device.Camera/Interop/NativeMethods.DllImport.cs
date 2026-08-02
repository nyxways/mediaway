#if !NET8_0_OR_GREATER
using System.Runtime.InteropServices;
using Mediaway.Device;

namespace Mediaway.Device.Camera.Interop;

internal static unsafe partial class NativeMethods
{
    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_camera_capture_open(
        in NativeCameraCaptureConfig config, out nint outCapture);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_camera_capture_geometry(
        CameraCaptureHandle capture, out uint outWidth, out uint outHeight);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_camera_capture_poll_frame(
        CameraCaptureHandle capture, out NativeCameraFrame outFrame, out byte outHasFrame);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_camera_capture_release_frame(
        CameraCaptureHandle capture);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_camera_capture_close(nint capture);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_camera_frame_free(ref NativeCameraFrame frame);
}
#endif
