namespace Mediaway.Device;

/// <summary>
/// General-purpose device kind — mirrors <c>mediaway_device_kind_t</c>. Used by
/// <c>Mediaway.Device.Hotplug.DeviceHotplug.Open</c>'s watched kinds and decoded into
/// <c>Mediaway.Device.Hotplug.DeviceChangedEventArgs.Kind</c>. Lives in the shared base
/// package because hotplug v1 scope (Microphone/Loopback) spans two leaf packages
/// (<c>Mediaway.Device.Audio</c>, <c>Mediaway.Device.Desktop</c>).
/// </summary>
public enum DeviceKind
{
    Screen = 0,
    Window = 1,
    Camera = 2,
    Microphone = 3,
    Loopback = 4,
    ProcessLoopback = 5,

    /// <summary>
    /// Decode-only catch-all for a future native <c>DeviceKind</c> variant this binding
    /// does not know about yet. Never pass this to <c>DeviceHotplug.Open</c> — the native
    /// layer rejects it with <see cref="MediawayDeviceStatus.InvalidInput"/>.
    /// </summary>
    Unknown = 255,
}
