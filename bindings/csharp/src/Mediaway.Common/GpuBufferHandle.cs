using System.Runtime.InteropServices;

namespace Mediaway.Common;

/// <summary>
/// A GPU texture/buffer handle (e.g. <c>ID3D11Texture2D*</c> + subresource) — mirrors
/// <c>mediaway_gpu_buffer_handle_t</c>. <b>Borrowed</b>, never owned by this binding.
/// Ownership direction and lifetime depend on the consumer: <c>Mediaway.Device.Desktop</c>'s
/// <c>DesktopVideoFrame.GpuBuffer</c> (polled frame) uses it as a borrowed <b>output</b>
/// aliasing the capture session's own texture (valid until the matching
/// <c>ReleaseFrame</c> call — see that type's own doc comment for the full hazard
/// contract); <c>Mediaway.Pipeline</c>'s <c>GpuVideoFrame.GpuBuffer</c> uses it as a
/// borrowed <b>input</b> aliasing the caller's own texture (valid only for the duration of
/// the encode call — see that type's own doc comment).
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public readonly struct GpuBufferHandle
{
    public GpuBufferKind Kind { get; init; }

    /// <summary>Primary native handle (e.g. <c>ID3D11Texture2D*</c>), reinterpreted as <see cref="nint"/>.</summary>
    public nint NativeA { get; init; }

    /// <summary>Secondary native handle, if the kind needs one (e.g. a shared-handle pair); zero otherwise.</summary>
    public nint NativeB { get; init; }

    /// <summary>Subresource index into <see cref="NativeA"/> (e.g. a D3D11 array slice).</summary>
    public uint Subresource { get; init; }

    public ulong WebGpuTextureId { get; init; }
}
