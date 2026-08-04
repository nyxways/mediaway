using Microsoft.Win32.SafeHandles;

namespace Mediaway.Pipeline.Interop;

/// <summary>
/// Owns one native <c>mediaway_audio_encode_session_t*</c>. Unlike
/// <see cref="AutoEncoderHandle"/>/<see cref="EncodeSessionHandle"/>, this surface has no
/// handle-consumption trap (adr/0003-auto-audio-encode-c-abi.md: the session IS the encoder,
/// no intermediate handle) — <c>mediaway_audio_encode_session_close</c> is always safe to
/// call, including on a poisoned handle.
/// </summary>
internal sealed class AudioEncodeSessionHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private AudioEncodeSessionHandle() : base(ownsHandle: true)
    {
    }

    internal static AudioEncodeSessionHandle Wrap(nint pointer)
    {
        var instance = new AudioEncodeSessionHandle();
        instance.SetHandle(pointer);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_audio_encode_session_close(handle);
        return true;
    }
}
