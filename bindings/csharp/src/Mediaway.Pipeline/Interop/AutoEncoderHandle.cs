using Microsoft.Win32.SafeHandles;

namespace Mediaway.Pipeline.Interop;

/// <summary>
/// Owns one native <c>mediaway_auto_encoder_t*</c>. <see cref="EncodeSession.Open"/> consumes
/// the wrapped pointer unconditionally (success or failure — the native
/// <c>mediaway_encode_session_open</c> takes it by value on the Rust side) and calls
/// <see cref="System.Runtime.InteropServices.SafeHandle.SetHandleAsInvalid"/> on this
/// instance immediately afterward, so a later <c>Dispose</c>/finalize never calls
/// <c>mediaway_auto_encoder_close</c> on an already-consumed pointer (double-free).
/// </summary>
internal sealed class AutoEncoderHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private AutoEncoderHandle() : base(ownsHandle: true)
    {
    }

    internal static AutoEncoderHandle Wrap(nint pointer)
    {
        var instance = new AutoEncoderHandle();
        instance.SetHandle(pointer);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_auto_encoder_close(handle);
        return true;
    }
}
