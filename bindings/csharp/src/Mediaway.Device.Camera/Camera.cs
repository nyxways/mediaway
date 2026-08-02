using Mediaway.Common;
using Mediaway.Device.Camera.Interop;

namespace Mediaway.Device.Camera;

/// <summary>Opens a video capture session against a camera / video capture device.</summary>
public static class Camera
{
    /// <param name="deviceIndex">Device ordinal (0 = default/first camera).</param>
    /// <param name="frameRate">Timebase for the frames this session will negotiate.</param>
    /// <exception cref="CaptureUnavailableException">No supported capture backend is compiled in here.</exception>
    public static IVideoCapture Open(uint deviceIndex, Rational frameRate)
    {
        var config = BuildConfig(deviceIndex, frameRate);
        var status = NativeMethods.mediaway_camera_capture_open(in config, out nint handle);
        MediawayDeviceException.ThrowIfError(status);
        return CameraCaptureSession.OpenFrom(CameraCaptureHandle.Wrap(handle));
    }

    /// <summary>Non-throwing form of <see cref="Open"/> — returns <see langword="null"/> and the failure status instead of throwing.</summary>
    public static IVideoCapture? TryOpen(uint deviceIndex, Rational frameRate, out MediawayDeviceStatus? error)
    {
        var config = BuildConfig(deviceIndex, frameRate);
        var status = NativeMethods.mediaway_camera_capture_open(in config, out nint handle);
        if (status != MediawayDeviceStatus.Ok)
        {
            error = status;
            return null;
        }

        error = null;
        return CameraCaptureSession.OpenFrom(CameraCaptureHandle.Wrap(handle));
    }

    private static NativeCameraCaptureConfig BuildConfig(uint deviceIndex, Rational frameRate) => new()
    {
        DeviceIndex = deviceIndex,
        TimeBase = new NativeRational(frameRate),
    };
}
