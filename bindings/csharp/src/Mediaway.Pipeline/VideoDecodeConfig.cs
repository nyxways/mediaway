using Mediaway.Common;

namespace Mediaway.Pipeline;

/// <summary>
/// Config for <see cref="DecodeSession.Open"/>. A plain value type — no native handle,
/// nothing to dispose.
/// </summary>
public sealed record VideoDecodeConfig
{
    public required VideoCodec Codec { get; init; }

    /// <summary>Expected; may be refined from the bitstream.</summary>
    public required uint Width { get; init; }

    public required uint Height { get; init; }

    public required Rational TimeBase { get; init; }

    /// <summary>Preferred output format when the backend converts.</summary>
    public PixelFormat PixelFormat { get; init; } = PixelFormat.Nv12;

    /// <summary>
    /// AVCC / SPS-PPS codec config, required at open time (not supplied via the first
    /// pushed packet — see <c>adr/0004-auto-decode-c-abi.md</c> §1 for why the
    /// muxer-track analogy does not hold for the wrapped decoder). Empty opens without a
    /// known codec config.
    /// </summary>
    public ReadOnlyMemory<byte> ExtraData { get; init; }
}
