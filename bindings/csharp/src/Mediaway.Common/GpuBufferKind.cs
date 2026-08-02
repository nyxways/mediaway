namespace Mediaway.Common;

/// <summary>GPU buffer/texture handle discriminant — mirrors <c>mediaway_gpu_buffer_kind_t</c>.</summary>
public enum GpuBufferKind
{
    DirectX11 = 0,
    DirectX12 = 1,
    DirectXShared = 2,
    Metal = 3,
    AndroidSurface = 4,
    Vulkan = 5,
    WebGpu = 6,
    Unknown = 255,
}
