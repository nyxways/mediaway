using System.Buffers;
using Mediaway.Common;

namespace Mediaway.Device.Desktop;

/// <summary>
/// One captured Screen video frame — from <see cref="IDesktopVideoCapture.TryPollFrame"/>.
/// GPU-resident by default (<see cref="StorageKind"/> is <see cref="VideoFrameStorageKind.Gpu"/>
/// for every real Screen backend today — Zero-Copy is the only path Screen supports, no
/// CPU fallback). <see cref="Dispose"/> releases an owned CPU buffer when
/// <see cref="StorageKind"/> is <see cref="VideoFrameStorageKind.Cpu"/>; it is a
/// documented no-op for the GPU case — <see cref="GpuBuffer"/> is <b>not</b> owned by this
/// frame object at all, and is released by the session's
/// <see cref="IDesktopVideoCapture.ReleaseFrame"/> instead (see that method's docs).
/// </summary>
public sealed class DesktopVideoFrame : IDisposable
{
    private readonly IMemoryOwner<byte> _owner;

    internal DesktopVideoFrame(IMemoryOwner<byte> owner) => _owner = owner;

    public required long Pts { get; init; }

    /// <summary><c>0</c> if unknown.</summary>
    public required ulong Duration { get; init; }

    public required uint Width { get; init; }

    public required uint Height { get; init; }

    public required PixelFormat PixelFormat { get; init; }

    public required VideoFrameStorageKind StorageKind { get; init; }

    /// <summary>Owned plane bytes — valid only when <see cref="StorageKind"/> is <see cref="VideoFrameStorageKind.Cpu"/>; empty otherwise.</summary>
    public required ReadOnlyMemory<byte> Data { get; init; }

    /// <summary>
    /// Borrowed GPU texture handle — valid only when <see cref="StorageKind"/> is
    /// <see cref="VideoFrameStorageKind.Gpu"/>; default otherwise. See <see cref="GpuBufferHandle"/>'s
    /// own docs for its lifetime — it is <b>not</b> released by <see cref="Dispose"/>.
    /// </summary>
    public required GpuBufferHandle GpuBuffer { get; init; }

    /// <summary>
    /// Releases the owned CPU buffer backing <see cref="Data"/> (<see cref="VideoFrameStorageKind.Cpu"/>
    /// only) — a no-op for a GPU-backed frame. Does <b>not</b> release <see cref="GpuBuffer"/>;
    /// see <see cref="IDesktopVideoCapture.ReleaseFrame"/> for that.
    /// </summary>
    public void Dispose() => _owner.Dispose();
}
