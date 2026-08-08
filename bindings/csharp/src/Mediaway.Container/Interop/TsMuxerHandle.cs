using Microsoft.Win32.SafeHandles;

namespace Mediaway.Container.Interop;

/// <summary>
/// Owns one native <c>mediaway_ts_muxer_t*</c>. A NULL handle is an ORDINARY failure (an
/// invalid PID or unsupported codec in <c>streams</c>), not just a defensive panic guard —
/// there is no status side channel on this constructor, so <see cref="Create"/> throws
/// explicitly instead of returning an invalid handle.
/// </summary>
internal sealed class TsMuxerHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private TsMuxerHandle() : base(ownsHandle: true)
    {
    }

    internal static unsafe TsMuxerHandle Create(
        ushort programNumber, ushort pmtPid, NativeTsElementaryStream* streams, nuint streamCount)
    {
        var instance = new TsMuxerHandle();
        instance.SetHandle(NativeMethods.mediaway_ts_muxer_create(programNumber, pmtPid, streams, streamCount));
        if (instance.IsInvalid)
        {
            MediawayContainerException.Throw(
                MediawayContainerStatus.InvalidArgument,
                "Invalid PMT/elementary-stream PID, an unsupported elementary-stream codec, " +
                "or the native call panicked.");
        }

        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_ts_muxer_close(handle);
        return true;
    }
}
