namespace Mediaway.Device;

/// <summary>
/// C ABI status code returned by fallible <c>mediaway-ffi</c> functions — mirrors
/// <c>mediaway_device_status_t</c>. A distinct, independently-numbered enum from
/// <see cref="Mediaway.Container.MediawayContainerStatus"/>/<see cref="Mediaway.Pipeline.MediawayPipelineStatus"/>.
/// </summary>
public enum MediawayDeviceStatus
{
    Ok = 0,
    InvalidArgument = 1,
    HandlePoisoned = 2,

    /// <summary>
    /// Window capture this pass, or <c>capture_once</c> on a Screen-kind config — a real
    /// capability with no C ABI path for this case yet, not "not implemented".
    /// </summary>
    Unsupported = 3,

    /// <summary>No supported capture backend is compiled in — expected/graceful.</summary>
    NoBackend = 4,

    InvalidInput = 5,
    BackendFailure = 6,
    Closed = 7,
    AccessDenied = 8,
    UnknownError = 9,
    InternalPanic = 10,
    CallbackAlreadyRegistered = 11,
    CallbackModeActive = 12,

    /// <summary><c>poll_frame_blocking</c>/<c>capture_once</c>'s deadline elapsed with no frame.</summary>
    Timeout = 13,
}
