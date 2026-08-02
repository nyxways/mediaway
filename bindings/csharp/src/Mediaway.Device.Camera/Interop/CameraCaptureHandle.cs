using Microsoft.Win32.SafeHandles;

namespace Mediaway.Device.Camera.Interop;

/// <summary>
/// Owns one native <c>mediaway_camera_capture_t*</c>. <c>ReleaseHandle</c> joins the
/// backend's worker thread and can block for up to one frame interval — a real,
/// non-instantaneous cost the native header documents rather than hides.
/// </summary>
internal sealed class CameraCaptureHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private CameraCaptureHandle() : base(ownsHandle: true)
    {
    }

    internal static CameraCaptureHandle Wrap(nint pointer)
    {
        var instance = new CameraCaptureHandle();
        instance.SetHandle(pointer);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        _ = NativeMethods.mediaway_camera_capture_close(handle);
        return true;
    }
}
