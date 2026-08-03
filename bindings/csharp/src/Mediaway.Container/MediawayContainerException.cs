using Mediaway.Common;

namespace Mediaway.Container;

/// <summary>
/// Thrown by <see cref="Muxer"/>/<see cref="MuxerSession"/>/<see cref="Demuxer"/> for any
/// native call that returns a non-<see cref="MediawayContainerStatus.Ok"/> status.
/// </summary>
public sealed class MediawayContainerException : MediawayException
{
    public MediawayContainerStatus Status { get; }

    private MediawayContainerException(MediawayContainerStatus status, string message) : base(message) =>
        Status = status;

    internal static void ThrowIfError(MediawayContainerStatus status)
    {
        if (status == MediawayContainerStatus.Ok)
        {
            return;
        }

        throw new MediawayContainerException(status, Describe(status));
    }

    private static string Describe(MediawayContainerStatus status) => status switch
    {
        MediawayContainerStatus.InvalidArgument =>
            "Invalid argument (null pointer, out-of-range index, or mismatched pointer/length).",
        MediawayContainerStatus.InvalidState =>
            "Invalid muxer/demuxer state for this operation (e.g. adding a track after " +
            "streaming began).",
        MediawayContainerStatus.InvalidTrack => "Invalid or duplicate track id.",
        MediawayContainerStatus.InvalidPacket =>
            "Packet does not match a registered track, or has bad framing.",
        MediawayContainerStatus.InvalidData => "Truncated or malformed ISOBMFF data.",
        MediawayContainerStatus.InternalPanic =>
            "The native call caught a Rust panic; this handle is now poisoned.",
        MediawayContainerStatus.HandlePoisoned => "A previous call already poisoned this handle.",
        _ => $"Unknown mediaway-ffi status ({(int)status}).",
    };
}
