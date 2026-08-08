using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>Owns one native <c>mediaway_ogg_demuxer_t*</c>.</summary>
internal sealed class OggDemuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private OggDemuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static OggDemuxerHandle Create()
    {
        var instance = new OggDemuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_ogg_demuxer_create());
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_ogg_demuxer_close(handle);
        return true;
    }
}
