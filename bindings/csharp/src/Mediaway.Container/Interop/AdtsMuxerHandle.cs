using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>
/// Owns one native <c>mediaway_adts_muxer_t*</c>. Unlike <see cref="MuxerHandle"/>, a NULL
/// handle here is an ORDINARY failure (non-standard <c>sampleRate</c>), not just a
/// defensive panic guard — there is no status side channel on this constructor, so
/// <see cref="Create"/> throws explicitly instead of returning an invalid handle.
/// </summary>
internal sealed class AdtsMuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private AdtsMuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static AdtsMuxerHandle Create(uint sampleRate, byte channels)
    {
        var instance = new AdtsMuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_adts_muxer_create(sampleRate, channels));
        if (instance.IsInvalid)
        {
            MediawayContainerException.Throw(
                MediawayContainerStatus.InvalidArgument,
                $"Non-standard ADTS sample rate ({sampleRate} Hz), or the native call panicked.");
        }

        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_adts_muxer_close(handle);
        return true;
    }
}
