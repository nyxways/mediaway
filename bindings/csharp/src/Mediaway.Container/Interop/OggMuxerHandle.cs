using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>Owns one native <c>mediaway_ogg_muxer_t*</c>.</summary>
internal sealed class OggMuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private OggMuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static OggMuxerHandle Create(uint serial)
    {
        var instance = new OggMuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_ogg_muxer_create(serial));
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_ogg_muxer_close(handle);
        return true;
    }
}
