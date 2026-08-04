using Mediaway.Common;

namespace Mediaway.Pipeline;

/// <summary>
/// Stream metadata from <see cref="AudioEncoder.StreamInfo"/>. <see cref="ExtraData"/> (the
/// AudioSpecificConfig) materializes only after the first pushed PCM frame — call
/// <see cref="AudioEncoder.StreamInfo"/> after <see cref="AudioEncoder.PushPcm"/>, before
/// registering a <c>Mediaway.Container.Muxer</c> audio track with it.
/// </summary>
public sealed record AudioStreamInfo
{
    public required CodecKind Codec { get; init; }

    public required Rational TimeBase { get; init; }

    /// <summary><c>0</c> when not yet known (before the first pushed frame).</summary>
    public required uint SampleRate { get; init; }

    /// <summary><c>0</c> when not yet known (before the first pushed frame).</summary>
    public required ushort Channels { get; init; }

    /// <summary>
    /// The AudioSpecificConfig (raw, MP4 <c>esds</c>-ready) — pass straight through as an
    /// audio track's <c>Mediaway.Container.AudioTrackInfo.ExtraData</c>.
    /// </summary>
    public required ReadOnlyMemory<byte> ExtraData { get; init; }
}
