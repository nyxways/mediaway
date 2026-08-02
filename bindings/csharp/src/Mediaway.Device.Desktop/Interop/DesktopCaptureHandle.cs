using Microsoft.Win32.SafeHandles;

namespace Mediaway.Device.Desktop.Interop;

/// <summary>
/// Owns one native <c>mediaway_desktop_capture_t*</c> (Screen). <c>ReleaseHandle</c> joins
/// the backend's worker thread and can block for up to one frame interval — a real,
/// non-instantaneous cost the native header documents rather than hides.
/// </summary>
internal sealed class DesktopCaptureHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private DesktopCaptureHandle() : base(ownsHandle: true)
    {
    }

    internal static DesktopCaptureHandle Wrap(nint pointer)
    {
        var instance = new DesktopCaptureHandle();
        instance.SetHandle(pointer);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        _ = NativeMethods.mediaway_desktop_capture_close(handle);
        return true;
    }
}
