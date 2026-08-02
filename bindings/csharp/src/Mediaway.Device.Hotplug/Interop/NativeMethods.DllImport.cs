#if !NET8_0_OR_GREATER
using System.Runtime.InteropServices;
using Mediaway.Device;

namespace Mediaway.Device.Hotplug.Interop;

/// <summary>
/// Matches the native <c>mediaway_device_hotplug_callback_fn</c> C ABI. Only a single,
/// <c>static readonly</c> instance of this delegate (wrapping a static method with no
/// captured state) is ever created — see <see cref="Device.DeviceHotplug"/>'s
/// <c>CallbackDelegate</c> field — so its classic-marshalling thunk stays valid for the
/// whole process lifetime, not just one registration.
/// </summary>
internal unsafe delegate void NativeHotplugCallback(nint userData, NativeDeviceEvent* @event);

internal static unsafe partial class NativeMethods
{
    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_device_hotplug_open(
        DeviceKind* kinds, nuint kindsLen, out nint outHotplug);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_device_hotplug_close(nint hotplug);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_device_hotplug_poll_event(
        HotplugHandle hotplug, out NativeDeviceEvent outEvent, out byte outHasEvent);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_device_hotplug_event_free(ref NativeDeviceEvent @event);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_device_hotplug_register_callback(
        HotplugHandle hotplug, NativeHotplugCallback? callback, nint userData);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_device_hotplug_unregister_callback(
        HotplugHandle hotplug);
}
#endif
