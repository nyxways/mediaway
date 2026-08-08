using Microsoft.Win32.SafeHandles;

namespace Mediaway.Pipeline.Interop;

/// <summary>
/// Owns one native <c>mediaway_audio_decode_session_t*</c>. Like <see cref="DecodeSessionHandle"/>,
/// this surface has no handle-consumption trap (adr/pipeline/0006-audio-decode-c-abi.md: the
/// session IS the decoder, no intermediate handle) — <c>mediaway_audio_decode_session_close</c>
/// is always safe to call, including on a poisoned handle.
/// </summary>
internal sealed class AudioDecodeSessionHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private AudioDecodeSessionHandle() : base(ownsHandle: true)
    {
    }

    internal static AudioDecodeSessionHandle Wrap(nint pointer)
    {
        var instance = new AudioDecodeSessionHandle();
        instance.SetHandle(pointer);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_audio_decode_session_close(handle);
        return true;
    }
}
