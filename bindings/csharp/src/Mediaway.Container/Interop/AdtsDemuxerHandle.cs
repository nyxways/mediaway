using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>Owns one native <c>mediaway_adts_demuxer_t*</c>.</summary>
internal sealed class AdtsDemuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private AdtsDemuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static AdtsDemuxerHandle Create()
    {
        var instance = new AdtsDemuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_adts_demuxer_create());
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_adts_demuxer_close(handle);
        return true;
    }
}
