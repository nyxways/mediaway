namespace Mediaway.Pipeline;

/// <summary>
/// Thrown by <see cref="AutoVideoEncoder.Open"/> when no supported video encoder backend is
/// compiled in for the running platform — an expected, graceful outcome to catch, not a bug.
/// </summary>
public sealed class EncoderUnavailableException : MediawayPipelineException
{
    internal EncoderUnavailableException(string message) : base(MediawayPipelineStatus.NoBackend, message)
    {
    }
}
