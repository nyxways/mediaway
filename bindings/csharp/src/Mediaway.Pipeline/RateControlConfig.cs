namespace Mediaway.Pipeline;

/// <summary>
/// Target bitrate + optional VBV buffer size for CBR-style rate control — see
/// <see cref="VideoEncodeConfig.RateControl"/>.
/// </summary>
public sealed record RateControlConfig
{
    public required uint TargetBitrateBps { get; init; }

    /// <summary><c>null</c> lets the backend pick a driver-suggested default.</summary>
    public uint? VbvBufferSizeBytes { get; init; }
}
