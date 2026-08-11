using System.Runtime.InteropServices;

namespace Mediaway.Device.Interop;

// Field order/sizes mirror crates/mediaway-ffi/include/mediaway/device.h's GPU device
// factory section exactly (LayoutKind.Sequential preserves declaration order); native
// `bool` (1 byte) is a `byte` field here, not C# `bool` (4 bytes), so every struct stays
// fully blittable.

/// <summary>OWNED entry inside the array returned by <c>mediaway_gpu_adapter_list</c> — freed only as one array via <c>mediaway_gpu_adapter_list_free</c>, never per-entry.</summary>
[StructLayout(LayoutKind.Sequential)]
internal unsafe struct NativeGpuAdapterInfo
{
    public uint Index;
    public byte* Name;
    public uint VendorId;
    public uint DeviceId;
    public ulong DedicatedVideoMemory;
    public byte IsHardware;
}

internal enum NativeGpuAdapterSelectKind
{
    Default = 0,
    Index = 1,
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeGpuAdapterSelect
{
    public NativeGpuAdapterSelectKind Kind;

    /// <summary>Meaningful only when <see cref="Kind"/> is <see cref="NativeGpuAdapterSelectKind.Index"/>.</summary>
    public uint Index;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeGpuDeviceOptions
{
    public NativeGpuAdapterSelect Adapter;
    public byte VideoSupport;
    public byte DebugLayer;
}
