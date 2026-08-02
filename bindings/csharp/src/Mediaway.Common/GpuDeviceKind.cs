namespace Mediaway.Common;

/// <summary>GPU device handle discriminant — mirrors <c>mediaway_gpu_device_kind_t</c>.</summary>
public enum GpuDeviceKind
{
    None = 0,
    DirectX11 = 1,
    DirectX12 = 2,
    Vulkan = 3,
    Metal = 4,
    WebGpu = 5,
}
