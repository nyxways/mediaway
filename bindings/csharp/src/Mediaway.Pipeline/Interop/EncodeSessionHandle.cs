using Microsoft.Win32.SafeHandles;

namespace Mediaway.Pipeline.Interop;

/// <summary>
/// Owns one native <c>mediaway_encode_session_t*</c>. <see cref="EncodeSession.Finish"/>
/// consumes the wrapped pointer unconditionally (success or failure — the native
/// <c>mediaway_encode_session_finish</c> takes it by value on the Rust side) and calls
/// <see cref="System.Runtime.InteropServices.SafeHandle.SetHandleAsInvalid"/> on this
/// instance immediately afterward, so a later <c>Dispose</c>/finalize never calls
/// <c>mediaway_encode_session_close</c> on an already-consumed pointer (double-free).
/// </summary>
internal sealed class EncodeSessionHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private EncodeSessionHandle() : base(ownsHandle: true)
    {
    }

    internal static EncodeSessionHandle Wrap(nint pointer)
    {
        var instance = new EncodeSessionHandle();
        instance.SetHandle(pointer);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_encode_session_close(handle);
        return true;
    }
}
