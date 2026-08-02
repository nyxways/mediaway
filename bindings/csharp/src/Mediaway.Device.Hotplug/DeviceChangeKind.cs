namespace Mediaway.Device.Hotplug;

/// <summary>What kind of change a <see cref="DeviceChangedEventArgs"/> reports.</summary>
public enum DeviceChangeKind
{
    Added = 0,
    Removed = 1,
    DefaultChanged = 2,
    StateChanged = 3,
}
