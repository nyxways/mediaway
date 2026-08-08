using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>
/// Owns one native <c>mediaway_demuxer_t*</c>. The critical finalizer guarantees
/// <c>mediaway_demuxer_close</c> runs even if a caller forgets to <c>Dispose</c>/<c>using</c>
/// the owning <see cref="Demuxer"/>.
/// </summary>
internal sealed class DemuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private DemuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static DemuxerHandle Create()
    {
        var instance = new DemuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_demuxer_create());
        return instance;
    }

    internal static DemuxerHandle CreateForFormat(ContainerFormat format)
    {
        var instance = new DemuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_demuxer_create_for_format(format));
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_demuxer_close(handle);
        return true;
    }
}
