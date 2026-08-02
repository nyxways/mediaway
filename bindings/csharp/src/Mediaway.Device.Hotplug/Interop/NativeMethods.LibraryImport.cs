#if NET8_0_OR_GREATER
using System.Runtime.InteropServices;
using Mediaway.Device;

namespace Mediaway.Device.Hotplug.Interop;

internal static unsafe partial class NativeMethods
{
    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_device_hotplug_open(
        DeviceKind* kinds, nuint kindsLen, out nint outHotplug);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_device_hotplug_close(nint hotplug);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_device_hotplug_poll_event(
        HotplugHandle hotplug, out NativeDeviceEvent outEvent, out byte outHasEvent);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_device_hotplug_event_free(ref NativeDeviceEvent @event);

    /// <summary>
    /// Raw function pointer, not a delegate — matches the native
    /// <c>mediaway_device_hotplug_callback_fn</c> C ABI exactly (a bare
    /// <c>extern "C"</c> function pointer, not a marshalled delegate thunk). <c>callback</c>
    /// must point to an <see cref="System.Runtime.InteropServices.UnmanagedCallersOnlyAttribute"/>-decorated
    /// static method — see <see cref="Device.DeviceHotplug"/>'s <c>NativeCallback</c>.
    /// </summary>
    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_device_hotplug_register_callback(
        HotplugHandle hotplug, delegate* unmanaged[Cdecl]<nint, NativeDeviceEvent*, void> callback, nint userData);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_device_hotplug_unregister_callback(
        HotplugHandle hotplug);
}
#endif
