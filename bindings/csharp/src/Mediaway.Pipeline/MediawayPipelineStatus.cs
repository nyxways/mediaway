namespace Mediaway.Pipeline;

/// <summary>
/// C ABI status code returned by fallible <c>mediaway-pipeline-ffi</c> functions —
/// mirrors <c>mediaway_pipeline_status_t</c>. A distinct, independently-numbered enum
/// from <see cref="Mediaway.Container.MediawayContainerStatus"/> — the two ABIs are not
/// unified.
/// </summary>
public enum MediawayPipelineStatus
{
    Ok = 0,
    InvalidArgument = 1,
    HandlePoisoned = 2,

    /// <summary>No supported encoder backend is compiled in — expected/graceful.</summary>
    NoBackend = 3,

    Unsupported = 4,
    InvalidInput = 5,
    EncoderBackendFailure = 6,
    EncoderClosed = 7,
    MuxInvalidTrack = 8,
    MuxInvalidPacket = 9,
    MuxInvalidData = 10,
    UnknownError = 11,
    InternalPanic = 12,
}
