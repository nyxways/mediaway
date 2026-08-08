using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>Owns one native <c>mediaway_ts_demuxer_t*</c>.</summary>
internal sealed class TsDemuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private TsDemuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static TsDemuxerHandle Create()
    {
        var instance = new TsDemuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_ts_demuxer_create());
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_ts_demuxer_close(handle);
        return true;
    }
}
