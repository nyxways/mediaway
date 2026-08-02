using Microsoft.Win32.SafeHandles;

namespace Mediaway.Device.Audio.Interop;

/// <summary>
/// Owns one native <c>mediaway_audio_capture_t*</c>. <c>ReleaseHandle</c> joins the
/// backend's worker thread and can block for up to one period interval — a real,
/// non-instantaneous cost the native header documents rather than hides.
/// </summary>
internal sealed class AudioCaptureHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private AudioCaptureHandle() : base(ownsHandle: true)
    {
    }

    internal static AudioCaptureHandle Wrap(nint pointer)
    {
        var instance = new AudioCaptureHandle();
        instance.SetHandle(pointer);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        _ = NativeMethods.mediaway_audio_capture_close(handle);
        return true;
    }
}
