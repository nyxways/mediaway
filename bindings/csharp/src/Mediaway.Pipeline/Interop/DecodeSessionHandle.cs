using Microsoft.Win32.SafeHandles;

namespace Mediaway.Pipeline.Interop;

/// <summary>
/// Owns one native <c>mediaway_decode_session_t*</c>. Like <see cref="AudioDecodeSessionHandle"/>,
/// this surface has no handle-consumption trap (adr/0004-auto-decode-c-abi.md: the session IS
/// the decoder, no intermediate handle) — <c>mediaway_decode_session_close</c> is always safe
/// to call, including on a poisoned handle.
/// </summary>
internal sealed class DecodeSessionHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private DecodeSessionHandle() : base(ownsHandle: true)
    {
    }

    internal static DecodeSessionHandle Wrap(nint pointer)
    {
        var instance = new DecodeSessionHandle();
        instance.SetHandle(pointer);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_decode_session_close(handle);
        return true;
    }
}
