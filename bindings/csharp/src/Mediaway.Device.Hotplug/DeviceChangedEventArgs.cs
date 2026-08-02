namespace Mediaway.Device.Hotplug;

/// <summary>
/// One hotplug notification — from <see cref="DeviceHotplug.PollEvent"/> (pull mode) or
/// <see cref="DeviceHotplug.DeviceChanged"/> (push mode).
/// </summary>
public sealed record DeviceChangedEventArgs
{
    public required DeviceChangeKind ChangeType { get; init; }

    public required Device.DeviceKind Kind { get; init; }

    /// <summary>
    /// The device's identity string (e.g. <c>"wasapi:&lt;endpoint-id&gt;"</c>). Null only for
    /// <see cref="DeviceChangeKind.DefaultChanged"/> when the kind now has no default.
    /// </summary>
    public string? DeviceId { get; init; }
}
