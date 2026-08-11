using Microsoft.Win32.SafeHandles;

namespace Mediaway.Device.Interop;

/// <summary>
/// Owns one native <c>mediaway_gpu_device_t*</c>. Unlike the capture handles, closing this
/// is a plain device drop — no worker thread to join, so <c>mediaway_gpu_device_close</c>
/// returns <see langword="void"/> instead of a status.
/// </summary>
internal sealed class GpuDeviceSessionHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private GpuDeviceSessionHandle() : base(ownsHandle: true)
    {
    }

    internal static GpuDeviceSessionHandle Wrap(nint pointer)
    {
        var instance = new GpuDeviceSessionHandle();
        instance.SetHandle(pointer);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.mediaway_gpu_device_close(handle);
        return true;
    }
}
