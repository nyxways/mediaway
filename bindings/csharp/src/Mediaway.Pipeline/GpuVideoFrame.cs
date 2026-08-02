using Mediaway.Common;

namespace Mediaway.Pipeline;

/// <summary>
/// Input to <see cref="EncodeSession.WriteGpuFrame"/> — a GPU-backed frame, the sibling of
/// the CPU-only <see cref="VideoFrame"/>. <see cref="GpuBuffer"/> is <b>borrowed</b>: the
/// caller owns the underlying texture and must keep it valid and unmodified for the
/// duration of the <see cref="EncodeSession.WriteGpuFrame"/> call only — this type does not
/// extend its lifetime, does not free it, and (on Windows) does not call
/// <c>Release()</c> on it. Only usable on a session opened from a
/// <see cref="VideoEncodeConfig"/> whose <see cref="VideoEncodeConfig.GpuDevice"/> is a
/// real device — see <c>adr/0002-gpu-frame-input-c-abi.md</c> in the Rust crate for the
/// full read-window / immediate-context hazard documentation this handle carries.
/// </summary>
public sealed record GpuVideoFrame
{
    public required long Pts { get; init; }

    /// <summary><c>0</c> if unknown.</summary>
    public required ulong Duration { get; init; }

    public required uint Width { get; init; }

    public required uint Height { get; init; }

    public required PixelFormat PixelFormat { get; init; }

    public required GpuBufferHandle GpuBuffer { get; init; }
}
