namespace Mediaway.Pipeline;

/// <summary>
/// Output of <see cref="AudioDecodeSession.PollFrame"/> — always interleaved F32 PCM
/// (<c>adr/pipeline/0006-audio-decode-c-abi.md</c> § Decode side). <see cref="Data"/> is a
/// private copy, safe to keep past the polling call.
/// </summary>
public sealed record DecodedAudioFrame
{
    public required long Pts { get; init; }

    /// <summary><c>0</c> if unknown.</summary>
    public required ulong Duration { get; init; }

    public required uint SampleRate { get; init; }

    public required ushort Channels { get; init; }

    public required byte[] Data { get; init; }
}
