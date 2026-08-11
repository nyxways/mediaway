namespace Mediaway.Device;

/// <summary>Options for <see cref="GpuDevice.Create"/> — mirrors <c>mediaway_gpu_device_options_t</c>.</summary>
public sealed record GpuDeviceOptions
{
    public GpuAdapterSelect Adapter { get; init; } = GpuAdapterSelect.Default;

    /// <summary><c>D3D11_CREATE_DEVICE_VIDEO_SUPPORT</c> — required for GPU-input encode.</summary>
    public bool VideoSupport { get; init; }

    /// <summary><c>D3D11_CREATE_DEVICE_DEBUG</c> — requires the D3D11 SDK debug layer installed locally.</summary>
    public bool DebugLayer { get; init; }
}
