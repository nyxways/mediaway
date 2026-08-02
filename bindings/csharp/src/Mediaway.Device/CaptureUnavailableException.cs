namespace Mediaway.Device;

/// <summary>
/// Thrown by every leaf package's <c>Open</c> (e.g. <c>Mediaway.Device.Camera.Camera.Open</c>,
/// <c>Mediaway.Device.Audio.Microphone.Open</c>) when no supported capture backend is
/// compiled in for the running platform — an expected, graceful outcome to catch, not a
/// bug. A specific device (e.g. camera index 3 of 1 available) failing to open surfaces as
/// the base <see cref="MediawayDeviceException"/> instead — that is a real, unexpected
/// failure the raw ABI itself does not distinguish as gracefully as "no backend".
/// </summary>
public sealed class CaptureUnavailableException : MediawayDeviceException
{
    internal CaptureUnavailableException(string message) : base(MediawayDeviceStatus.NoBackend, message)
    {
    }
}
