using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>
/// Owns one native <c>mediaway_replay_ring_t*</c>. The critical finalizer guarantees
/// <c>mediaway_replay_ring_close</c> runs even if a caller forgets to dispose the
/// <see cref="ReplayRing"/>.
/// </summary>
internal sealed class ReplayRingHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private ReplayRingHandle() : base(ownsHandle: true)
    {
    }

    internal static ReplayRingHandle FromRaw(nint raw)
    {
        var instance = new ReplayRingHandle();
        instance.SetHandle(raw);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_replay_ring_close(handle);
        return true;
    }
}
