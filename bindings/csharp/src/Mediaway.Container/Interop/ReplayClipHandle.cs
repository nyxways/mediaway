using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>
/// Owns one native <c>mediaway_replay_clip_t*</c> — an owned snapshot, independent of the ring
/// it was cut from. Releasing it invalidates every payload pointer read from it.
/// </summary>
internal sealed class ReplayClipHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private ReplayClipHandle() : base(ownsHandle: true)
    {
    }

    internal static ReplayClipHandle FromRaw(nint raw)
    {
        var instance = new ReplayClipHandle();
        instance.SetHandle(raw);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_replay_clip_free(handle);
        return true;
    }
}
