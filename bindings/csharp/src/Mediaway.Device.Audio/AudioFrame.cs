using System.Buffers;
using Mediaway.Common;

namespace Mediaway.Device.Audio;

/// <summary>
/// One captured PCM chunk — from <see cref="IAudioCapture.ReadFramesAsync"/>. Owns the
/// native buffer backing <see cref="Data"/>; dispose when done with it.
/// </summary>
public sealed class AudioFrame : IDisposable
{
    private readonly IMemoryOwner<byte> _owner;

    internal AudioFrame(IMemoryOwner<byte> owner) => _owner = owner;

    public required long Pts { get; init; }

    public required ulong Duration { get; init; }

    /// <summary>Negotiated by the backend (e.g. WASAPI <c>GetMixFormat</c>).</summary>
    public required uint SampleRate { get; init; }

    /// <summary>Negotiated by the backend.</summary>
    public required ushort Channels { get; init; }

    public required SampleFormat SampleFormat { get; init; }

    public required ReadOnlyMemory<byte> Data { get; init; }

    /// <summary>Releases the native buffer backing <see cref="Data"/>.</summary>
    public void Dispose() => _owner.Dispose();
}
