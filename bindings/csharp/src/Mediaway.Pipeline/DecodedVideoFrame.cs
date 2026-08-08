using Mediaway.Common;

namespace Mediaway.Pipeline;

/// <summary>
/// Output of <see cref="DecodeSession.PollFrame"/> — CPU-only (GPU decode output is
/// deferred, <c>adr/0004-auto-decode-c-abi.md</c> §1/§5). <see cref="Data"/> is a private
/// copy, safe to keep past the polling call.
/// </summary>
public sealed record DecodedVideoFrame
{
    public required long Pts { get; init; }

    /// <summary><c>0</c> if unknown.</summary>
    public required ulong Duration { get; init; }

    public required uint Width { get; init; }

    public required uint Height { get; init; }

    public required PixelFormat PixelFormat { get; init; }

    public required byte[] Data { get; init; }
}
