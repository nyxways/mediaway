namespace Mediaway.Container;

/// <summary>
/// Fixed Layer III frame header for <see cref="Mp3Muxer"/> — bitrate/sample rate/channel
/// mode stay constant for the whole mux session's lifetime.
/// </summary>
public sealed record Mp3FrameHeader
{
    public required Mp3MpegVersion Version { get; init; }

    /// <summary>Must be one of the 14 standard Layer III bitrates for <see cref="Version"/>.</summary>
    public required ushort BitrateKbps { get; init; }

    /// <summary>Must be one of the 3 standard sample rates for <see cref="Version"/>.</summary>
    public required uint SampleRate { get; init; }

    public required Mp3ChannelMode ChannelMode { get; init; }
}
