using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>Owns one native <c>mediaway_flv_muxer_t*</c>.</summary>
internal sealed class FlvMuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private FlvMuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static FlvMuxerHandle Create()
    {
        var instance = new FlvMuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_flv_muxer_create());
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_flv_muxer_close(handle);
        return true;
    }
}
