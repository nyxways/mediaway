using Mediaway.Common;
using Mediaway.Device.Desktop.Interop;

namespace Mediaway.Device.Desktop;

/// <summary>
/// Opens a Zero-Copy single-window capture session (WGC, Windows). Off Windows every call
/// fails with <see cref="MediawayDeviceStatus.Unsupported"/> — other platforms' window
/// sources are not <c>HWND</c>s.
/// </summary>
public static class DesktopWindowCapture
{
    /// <param name="hwnd">
    /// The window's <c>HWND</c>. Caller-owned: it must stay valid for the whole session. Zero
    /// fails with <see cref="MediawayDeviceStatus.InvalidInput"/>.
    /// </param>
    /// <param name="frameRate">Timebase for the frames this session will negotiate.</param>
    /// <param name="gpuDevice">
    /// A live <see cref="GpuDeviceHandle.DirectX11"/> handle — mandatory, no CPU fallback. The
    /// caller must keep the underlying <c>ID3D11Device</c> alive for the whole session.
    /// </param>
    /// <param name="options"><see langword="null"/> means <see cref="WindowCaptureOptions"/>' defaults.</param>
    /// <exception cref="CaptureUnavailableException">No supported capture backend is compiled in here.</exception>
    /// <exception cref="MediawayDeviceException">
    /// <see cref="MediawayDeviceStatus.RegionOutOfBounds"/> when the region does not fit the
    /// window; <see cref="MediawayDeviceStatus.InvalidInput"/> for a bad handle or a zero-sized region.
    /// </exception>
    public static IDesktopWindowCapture Open(
        nint hwnd, Rational frameRate, GpuDeviceHandle gpuDevice, WindowCaptureOptions? options = null)
    {
        var config = BuildConfig(hwnd, frameRate, gpuDevice, options);
        var status = NativeMethods.mediaway_desktop_capture_open(in config, out nint handle);
        MediawayDeviceException.ThrowIfError(status);
        return DesktopWindowCaptureSession.OpenWindowFrom(DesktopCaptureHandle.Wrap(handle));
    }

    /// <summary>Non-throwing form of <see cref="Open"/> — returns <see langword="null"/> and the failure status instead of throwing.</summary>
    public static IDesktopWindowCapture? TryOpen(
        nint hwnd, Rational frameRate, GpuDeviceHandle gpuDevice, WindowCaptureOptions? options,
        out MediawayDeviceStatus? error)
    {
        var config = BuildConfig(hwnd, frameRate, gpuDevice, options);
        var status = NativeMethods.mediaway_desktop_capture_open(in config, out nint handle);
        if (status != MediawayDeviceStatus.Ok)
        {
            error = status;
            return null;
        }

        error = null;
        return DesktopWindowCaptureSession.OpenWindowFrom(DesktopCaptureHandle.Wrap(handle));
    }

    internal static NativeDesktopCaptureConfig BuildConfig(
        nint hwnd, Rational frameRate, GpuDeviceHandle gpuDevice, WindowCaptureOptions? options)
    {
        options ??= new WindowCaptureOptions();
        var config = new NativeDesktopCaptureConfig
        {
            SourceKind = NativeDesktopCaptureSourceKind.Window,
            SourceIndex = 0,
            TimeBase = new NativeRational(frameRate),
            GpuDevice = gpuDevice,
            // Widen through long: a sign-extended HWND is still the bit pattern the OS gave.
            WindowHandle = unchecked((ulong)(long)hwnd),
            Cursor = options.Cursor,
            Border = options.Border,
            Dimensions = options.Dimensions,
        };
        if (options.Region is { } region)
        {
            config.RegionEnabled = 1;
            config.RegionX = region.X;
            config.RegionY = region.Y;
            config.RegionWidth = region.Width;
            config.RegionHeight = region.Height;
        }

        return config;
    }
}
