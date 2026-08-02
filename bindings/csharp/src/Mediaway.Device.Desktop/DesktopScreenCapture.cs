using Mediaway.Common;
using Mediaway.Device.Desktop.Interop;

namespace Mediaway.Device.Desktop;

/// <summary>Opens a Zero-Copy Screen (DXGI Desktop Duplication) video capture session.</summary>
public static class DesktopScreenCapture
{
    /// <param name="outputIndex">Display output ordinal (0 = primary).</param>
    /// <param name="frameRate">Timebase for the frames this session will negotiate.</param>
    /// <param name="gpuDevice">
    /// A live <see cref="GpuDeviceHandle.DirectX11"/> handle — mandatory, no CPU fallback.
    /// The caller must keep the underlying <c>ID3D11Device</c> alive for the whole session.
    /// </param>
    /// <exception cref="CaptureUnavailableException">No supported capture backend is compiled in here.</exception>
    public static IDesktopVideoCapture Open(uint outputIndex, Rational frameRate, GpuDeviceHandle gpuDevice)
    {
        var config = BuildConfig(outputIndex, frameRate, gpuDevice);
        var status = NativeMethods.mediaway_desktop_capture_open(in config, out nint handle);
        MediawayDeviceException.ThrowIfError(status);
        return DesktopVideoCaptureSession.OpenFrom(DesktopCaptureHandle.Wrap(handle));
    }

    /// <summary>Non-throwing form of <see cref="Open"/> — returns <see langword="null"/> and the failure status instead of throwing.</summary>
    public static IDesktopVideoCapture? TryOpen(
        uint outputIndex, Rational frameRate, GpuDeviceHandle gpuDevice, out MediawayDeviceStatus? error)
    {
        var config = BuildConfig(outputIndex, frameRate, gpuDevice);
        var status = NativeMethods.mediaway_desktop_capture_open(in config, out nint handle);
        if (status != MediawayDeviceStatus.Ok)
        {
            error = status;
            return null;
        }

        error = null;
        return DesktopVideoCaptureSession.OpenFrom(DesktopCaptureHandle.Wrap(handle));
    }

    private static NativeDesktopCaptureConfig BuildConfig(uint outputIndex, Rational frameRate, GpuDeviceHandle gpuDevice) => new()
    {
        SourceKind = NativeDesktopCaptureSourceKind.Screen,
        SourceIndex = outputIndex,
        TimeBase = new NativeRational(frameRate),
        GpuDevice = gpuDevice,
    };
}
