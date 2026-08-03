using System.Runtime.InteropServices;

namespace Mediaway.Device.Hotplug.Interop;

// Field order/sizes mirror crates/mediaway-ffi/src/types.rs (hotplug section)
// exactly (LayoutKind.Sequential preserves declaration order).

[StructLayout(LayoutKind.Sequential)]
internal struct NativeDeviceEvent
{
    public DeviceChangeKind EventKind;
    public DeviceKind DeviceKind;
    public nint DeviceId;
}
