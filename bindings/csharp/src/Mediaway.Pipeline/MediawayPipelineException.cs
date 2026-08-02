using Mediaway.Common;

namespace Mediaway.Pipeline;

/// <summary>
/// Thrown by <see cref="AutoVideoEncoder"/>/<see cref="EncodeSession"/> for any native call
/// that returns a non-<see cref="MediawayPipelineStatus.Ok"/> status other than
/// <see cref="MediawayPipelineStatus.NoBackend"/> (which throws the more specific
/// <see cref="EncoderUnavailableException"/> instead).
/// </summary>
public class MediawayPipelineException : MediawayException
{
    public MediawayPipelineStatus Status { get; }

    internal MediawayPipelineException(MediawayPipelineStatus status, string message) : base(message) =>
        Status = status;

    internal static void ThrowIfError(MediawayPipelineStatus status)
    {
        switch (status)
        {
            case MediawayPipelineStatus.Ok:
                return;
            case MediawayPipelineStatus.NoBackend:
                throw new EncoderUnavailableException(
                    "No supported video encoder backend is compiled in on this platform.");
            default:
                throw new MediawayPipelineException(status, Describe(status));
        }
    }

    private static string Describe(MediawayPipelineStatus status) => status switch
    {
        MediawayPipelineStatus.InvalidArgument => "Null pointer, or mismatched pointer/length pair.",
        MediawayPipelineStatus.HandlePoisoned => "A previous call already poisoned this handle.",
        MediawayPipelineStatus.Unsupported =>
            "Unsupported codec, pixel format, or geometry for this encoder backend.",
        MediawayPipelineStatus.InvalidInput => "Invalid dimensions, rates, or frame metadata.",
        MediawayPipelineStatus.EncoderBackendFailure => "An OS/API failure occurred inside the encoder backend.",
        MediawayPipelineStatus.EncoderClosed => "This session already finished, or was never open.",
        MediawayPipelineStatus.MuxInvalidTrack => "The muxer rejected the encoder's stream info.",
        MediawayPipelineStatus.MuxInvalidPacket => "A packet did not match the registered track.",
        MediawayPipelineStatus.MuxInvalidData => "Malformed container data.",
        MediawayPipelineStatus.InternalPanic =>
            "The native call caught a Rust panic; this handle is now poisoned.",
        _ => $"Unknown mediaway-pipeline-ffi status ({(int)status}).",
    };
}
