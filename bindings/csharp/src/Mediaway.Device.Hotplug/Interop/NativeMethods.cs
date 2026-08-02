namespace Mediaway.Device.Hotplug.Interop;

/// <summary>
/// Raw P/Invoke declarations, 1:1 with <c>mediaway_device_hotplug_*</c> in
/// <c>crates/mediaway-device-ffi/src/hotplug.rs</c> (feature <c>"hotplug"</c>). Never
/// public — every call is wrapped by <see cref="Device.DeviceHotplug"/> into a safe,
/// idiomatic surface (poll mode via <c>PollEvent</c>, push mode via the
/// <c>DeviceChanged</c> event — see that type's doc comment for the native callback
/// contract, <c>adr/0002-callback-event-delivery.md</c>).
///
/// Declarations split across <c>NativeMethods.LibraryImport.cs</c> (net8.0,
/// source-generated, raw <c>delegate* unmanaged</c> function pointer) and
/// <c>NativeMethods.DllImport.cs</c> (netstandard2.0, classic delegate marshalling) — see
/// docs/adr/0018-csharp-netstandard20-unity.md. Exactly one half compiles per target
/// framework.
/// </summary>
internal static unsafe partial class NativeMethods
{
    private const string LibraryName = "mediaway_device_ffi";
}
