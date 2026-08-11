namespace Mediaway.Device;

/// <summary>One enumerated DXGI adapter — mirrors <c>mediaway_gpu_adapter_info_t</c>.</summary>
public sealed record GpuAdapterInfo
{
    public required uint Index { get; init; }

    public required string Name { get; init; }

    public required uint VendorId { get; init; }

    public required uint DeviceId { get; init; }

    /// <summary>Dedicated VRAM, in bytes.</summary>
    public required ulong DedicatedVideoMemory { get; init; }

    public required bool IsHardware { get; init; }
}
