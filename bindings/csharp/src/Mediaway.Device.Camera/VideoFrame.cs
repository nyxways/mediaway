using System.Buffers;
using Mediaway.Common;

namespace Mediaway.Device.Camera;

/// <summary>
/// One captured camera video frame — from <see cref="IVideoCapture.ReadFramesAsync"/>.
/// CPU-only (Camera never uses Zero-Copy — see <c>Mediaway.Device.Desktop</c> for the
/// GPU-capable Screen frame type). Owns the native buffer backing <see cref="Data"/>;
/// dispose when done with it.
/// </summary>
public sealed class VideoFrame : IDisposable
{
    private readonly IMemoryOwner<byte> _owner;

    internal VideoFrame(IMemoryOwner<byte> owner) => _owner = owner;

    public required long Pts { get; init; }

    /// <summary><c>0</c> if unknown.</summary>
    public required ulong Duration { get; init; }

    public required uint Width { get; init; }

    public required uint Height { get; init; }

    public required PixelFormat PixelFormat { get; init; }

    public required ReadOnlyMemory<byte> Data { get; init; }

    /// <summary>Releases the native buffer backing <see cref="Data"/>.</summary>
    public void Dispose() => _owner.Dispose();
}
