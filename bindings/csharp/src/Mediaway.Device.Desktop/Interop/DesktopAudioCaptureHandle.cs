using Microsoft.Win32.SafeHandles;

namespace Mediaway.Device.Desktop.Interop;

/// <summary>
/// Owns one native <c>mediaway_desktop_audio_capture_t*</c> (Loopback/ProcessLoopback).
/// <c>ReleaseHandle</c> joins the backend's worker thread and can block for up to one
/// period interval — a real, non-instantaneous cost the native header documents rather
/// than hides.
/// </summary>
internal sealed class DesktopAudioCaptureHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private DesktopAudioCaptureHandle() : base(ownsHandle: true)
    {
    }

    internal static DesktopAudioCaptureHandle Wrap(nint pointer)
    {
        var instance = new DesktopAudioCaptureHandle();
        instance.SetHandle(pointer);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        _ = NativeMethods.mediaway_desktop_audio_capture_close(handle);
        return true;
    }
}
