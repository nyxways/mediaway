using Mediaway.Common;

namespace Mediaway.Pipeline;

/// <summary>
/// Input to <see cref="EncodeSession.WriteFrame"/> — CPU-only raw frame data,
/// <see cref="Data"/> borrowed for the call only. See <see cref="GpuVideoFrame"/> for the
/// GPU-backed sibling accepted by <see cref="EncodeSession.WriteGpuFrame"/>. Frames only
/// ever go in, never come back out of the pipeline ABI, so unlike
/// <c>Mediaway.Container.Packet</c> this type needs no disposal.
/// </summary>
public sealed record VideoFrame
{
    public required long Pts { get; init; }

    /// <summary><c>0</c> if unknown.</summary>
    public required ulong Duration { get; init; }

    public required uint Width { get; init; }

    public required uint Height { get; init; }

    public required PixelFormat PixelFormat { get; init; }

    public required ReadOnlyMemory<byte> Data { get; init; }
}
