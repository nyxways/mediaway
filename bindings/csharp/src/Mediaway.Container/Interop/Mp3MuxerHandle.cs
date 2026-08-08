using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>
/// Owns one native <c>mediaway_mp3_muxer_t*</c>. A NULL handle is an ORDINARY failure (a
/// non-standard bitrate/sample-rate combination), not just a defensive panic guard — there
/// is no status side channel on this constructor, so <see cref="Create"/> throws explicitly
/// instead of returning an invalid handle.
/// </summary>
internal sealed class Mp3MuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private Mp3MuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static Mp3MuxerHandle Create(in NativeMp3FrameHeader header)
    {
        var instance = new Mp3MuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_mp3_muxer_create(in header));
        if (instance.IsInvalid)
        {
            MediawayContainerException.Throw(
                MediawayContainerStatus.InvalidArgument,
                "Non-standard MP3 bitrate/sample-rate combination, or the native call panicked.");
        }

        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_mp3_muxer_close(handle);
        return true;
    }
}
