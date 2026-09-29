using Mediaway.Device.Desktop.Interop;

namespace Mediaway.Device.Desktop;

/// <summary>
/// A Window session. Derives from <see cref="DesktopVideoCaptureSession"/> so
/// <c>Mediaway.Pipeline</c>'s capture-to-encode bridge accepts it like a Screen session.
/// </summary>
internal sealed class DesktopWindowCaptureSession : DesktopVideoCaptureSession, IDesktopWindowCapture
{
    private DesktopWindowCaptureSession(DesktopCaptureHandle handle, uint width, uint height, bool borderHidden)
        : base(handle, width, height) =>
        BorderHidden = borderHidden;

    public bool BorderHidden { get; }

    internal static DesktopWindowCaptureSession OpenWindowFrom(DesktopCaptureHandle handle)
    {
        var (width, height) = ReadGeometry(handle);
        var status = NativeMethods.mediaway_desktop_capture_border_hidden(handle, out byte hidden);
        MediawayDeviceException.ThrowIfError(status);
        return new DesktopWindowCaptureSession(handle, width, height, hidden != 0);
    }
}
