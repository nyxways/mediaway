using Mediaway.Common;

namespace Mediaway.Pipeline;

/// <summary>
/// Config for <see cref="AudioEncoder.Open"/>. A plain value type — no native handle,
/// nothing to dispose. AAC is the only output codec the real backend accepts today
/// (adr/0003-auto-audio-encode-c-abi.md in mediaway-ffi); F32 is the only accepted input
/// PCM format, fixed internally by <see cref="AudioEncoder.Open"/>.
/// </summary>
public sealed record AudioEncodeConfig
{
    public required uint SampleRate { get; init; }

    public required ushort Channels { get; init; }

    /// <summary>Timebase for timestamps on pushed frames / polled packets.</summary>
    public required Rational TimeBase { get; init; }

    /// <summary><c>0</c> means "let the backend pick its own default (128 kbps)".</summary>
    public uint BitrateBps { get; init; }
}
