using System.Runtime.InteropServices;

namespace Mediaway.Common;

/// <summary>
/// Caller-supplied GPU device handle — mirrors <c>mediaway_gpu_device_handle_t</c>. The
/// caller owns the underlying device (e.g. an <c>ID3D11Device*</c>) and must keep it alive
/// for at least the duration of the call it's passed to; this binding never constructs or
/// frees one. Blittable and used directly as the native P/Invoke parameter/field type — no
/// separate internal wrapper (nothing here needs hiding; it is a raw handle passthrough).
/// Shared by every package that accepts a GPU device as input (<c>Mediaway.Device.Desktop</c>'s
/// Screen capture, <c>Mediaway.Pipeline</c>'s <c>AutoVideoEncoder</c>) — the exact
/// accept/reject contract for <see cref="None"/> is documented per consumer.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public readonly struct GpuDeviceHandle
{
    public GpuDeviceKind Kind { get; init; }

    /// <summary>The native device pointer (e.g. <c>ID3D11Device*</c>), reinterpreted as <see cref="nint"/>.</summary>
    public nint Native { get; init; }

    public ulong WebGpuDeviceId { get; init; }

    /// <summary>No GPU device — the zero value. Consumer-specific meaning; see call site docs.</summary>
    public static readonly GpuDeviceHandle None = default;

    /// <param name="device">A live <c>ID3D11Device*</c>, kept alive by the caller for at least the duration of the call it's passed to.</param>
    public static GpuDeviceHandle DirectX11(nint device) => new()
    {
        Kind = GpuDeviceKind.DirectX11,
        Native = device,
    };
}
