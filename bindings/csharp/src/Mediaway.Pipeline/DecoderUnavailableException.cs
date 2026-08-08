namespace Mediaway.Pipeline;

/// <summary>
/// Thrown by <see cref="DecodeSession.Open"/>/<see cref="AudioDecodeSession.Open"/> when no
/// supported decoder backend is compiled in for the running platform — an expected, graceful
/// outcome to catch, not a bug.
/// </summary>
public sealed class DecoderUnavailableException : MediawayPipelineException
{
    internal DecoderUnavailableException(string message) : base(MediawayPipelineStatus.NoBackend, message)
    {
    }
}
