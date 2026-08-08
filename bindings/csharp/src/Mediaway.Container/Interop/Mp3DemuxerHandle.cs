using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>Owns one native <c>mediaway_mp3_demuxer_t*</c>.</summary>
internal sealed class Mp3DemuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private Mp3DemuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static Mp3DemuxerHandle Create()
    {
        var instance = new Mp3DemuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_mp3_demuxer_create());
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_mp3_demuxer_close(handle);
        return true;
    }
}
