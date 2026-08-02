using Microsoft.Win32.SafeHandles;

namespace Mediaway.Device.Hotplug.Interop;

/// <summary>
/// Owns one native <c>mediaway_device_hotplug_t*</c>. Always safe to release, including on
/// a poisoned handle or with an active callback registration (the native <c>close</c>
/// implicitly unregisters first).
/// </summary>
internal sealed class HotplugHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    private HotplugHandle() : base(ownsHandle: true)
    {
    }

    internal static HotplugHandle Wrap(nint pointer)
    {
        var instance = new HotplugHandle();
        instance.SetHandle(pointer);
        return instance;
    }

    protected override bool ReleaseHandle()
    {
        _ = NativeMethods.mediaway_device_hotplug_close(handle);
        return true;
    }
}
