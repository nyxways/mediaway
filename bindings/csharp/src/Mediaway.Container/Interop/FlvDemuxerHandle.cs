using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>Owns one native <c>mediaway_flv_demuxer_t*</c>.</summary>
internal sealed class FlvDemuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private FlvDemuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static FlvDemuxerHandle Create()
    {
        var instance = new FlvDemuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_flv_demuxer_create());
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_flv_demuxer_close(handle);
        return true;
    }
}
