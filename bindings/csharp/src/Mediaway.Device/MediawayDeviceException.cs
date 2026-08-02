using Mediaway.Common;

namespace Mediaway.Device;

/// <summary>
/// Thrown by <c>Mediaway.Device.Camera</c>/<c>Mediaway.Device.Desktop</c>/
/// <c>Mediaway.Device.Audio</c>/<c>Mediaway.Device.Hotplug</c> for any native call that
/// returns a non-<see cref="MediawayDeviceStatus.Ok"/> status other than
/// <see cref="MediawayDeviceStatus.NoBackend"/> (which throws the more specific
/// <see cref="CaptureUnavailableException"/> instead). Shared across every leaf package
/// so a caller catches one exception type regardless of which capture domain failed.
/// </summary>
public class MediawayDeviceException : MediawayException
{
    public MediawayDeviceStatus Status { get; }

    internal MediawayDeviceException(MediawayDeviceStatus status, string message) : base(message) =>
        Status = status;

    internal static void ThrowIfError(MediawayDeviceStatus status)
    {
        switch (status)
        {
            case MediawayDeviceStatus.Ok:
                return;
            case MediawayDeviceStatus.NoBackend:
                throw new CaptureUnavailableException(
                    "No supported capture backend is compiled in on this platform.");
            default:
                throw new MediawayDeviceException(status, Describe(status));
        }
    }

    private static string Describe(MediawayDeviceStatus status) => status switch
    {
        MediawayDeviceStatus.InvalidArgument => "Null pointer, or mismatched pointer/length pair.",
        MediawayDeviceStatus.HandlePoisoned => "A previous call already poisoned this handle.",
        MediawayDeviceStatus.Unsupported =>
            "This capture source is not reachable from this binding yet (e.g. Window capture, " +
            "or Screen capture via the single-shot convenience call).",
        MediawayDeviceStatus.InvalidInput =>
            "Invalid config (e.g. a zero-denominator time base, or a mismatched GPU device).",
        MediawayDeviceStatus.BackendFailure => "An OS/API failure occurred inside the capture backend.",
        MediawayDeviceStatus.Closed => "This session already closed, or was never open.",
        MediawayDeviceStatus.AccessDenied => "Desktop duplication or device access was denied.",
        MediawayDeviceStatus.InternalPanic =>
            "The native call caught a Rust panic; this handle is now poisoned.",
        MediawayDeviceStatus.CallbackAlreadyRegistered =>
            "A hotplug callback is already registered on this handle.",
        MediawayDeviceStatus.CallbackModeActive =>
            "PollEvent() was called while a hotplug callback is registered on this handle.",
        MediawayDeviceStatus.Timeout => "The capture deadline elapsed with no frame.",
        _ => $"Unknown mediaway-device-ffi status ({(int)status}).",
    };
}
