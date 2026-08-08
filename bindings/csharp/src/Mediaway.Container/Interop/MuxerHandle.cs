using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>
/// Owns one native <c>mediaway_muxer_t*</c>. The critical finalizer guarantees
/// <c>mediaway_muxer_close</c> runs even if a caller forgets to <c>Dispose</c>/<c>using</c>
/// the owning <see cref="Muxer"/>/<see cref="MuxerSession"/>.
/// </summary>
internal sealed class MuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private MuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static MuxerHandle Create()
    {
        var instance = new MuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_muxer_create());
        return instance;
    }

    internal static MuxerHandle CreateForFormat(ContainerFormat format)
    {
        var instance = new MuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_muxer_create_for_format(format));
        return instance;
    }

    internal static MuxerHandle CreateWithFragmentBatch(nuint batch)
    {
        var instance = new MuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_muxer_create_with_fragment_batch(batch));
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_muxer_close(handle);
        return true;
    }
}
