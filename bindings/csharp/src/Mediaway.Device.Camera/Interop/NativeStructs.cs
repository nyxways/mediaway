using System.Runtime.InteropServices;
using Mediaway.Common;

namespace Mediaway.Device.Camera.Interop;

// Field order/sizes mirror crates/mediaway-device-ffi/src/camera.rs + types.rs exactly
// (LayoutKind.Sequential preserves declaration order); native `bool` (1 byte) is a `byte`
// field here, not C# `bool` (4 bytes), so every struct stays fully blittable.

[StructLayout(LayoutKind.Sequential)]
internal readonly struct NativeRational
{
    public readonly ulong Num;
    public readonly uint Den;

    public NativeRational(Rational value)
    {
        Num = value.Num;
        Den = value.Den;
    }
}

/// <summary>
/// No <c>gpu_device</c> field — every shipped Camera backend rejects Zero-Copy today
/// (`adr/0004-domain-feature-split.md`), unlike Desktop's Screen config.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
internal struct NativeCameraCaptureConfig
{
    public uint DeviceIndex;
    public NativeRational TimeBase;
}

/// <summary>CPU-only — no storage-kind tag, no GPU buffer field (unlike Desktop's frame).</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct NativeCameraFrame
{
    public long Pts;
    public ulong Duration;
    public uint Width;
    public uint Height;
    public PixelFormat PixelFormat;
    public nint Data;
    public nuint DataLen;
}
