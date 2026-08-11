using Mediaway.Device.Interop;

namespace Mediaway.Device;

/// <summary>Which DXGI adapter <see cref="GpuDevice.Create"/> should open — mirrors <c>mediaway_gpu_adapter_select_t</c>.</summary>
public readonly struct GpuAdapterSelect
{
    private readonly bool _isIndex;
    private readonly uint _index;

    private GpuAdapterSelect(bool isIndex, uint index)
    {
        _isIndex = isIndex;
        _index = index;
    }

    /// <summary>Let the backend pick its own default adapter (first hardware adapter on Windows).</summary>
    public static readonly GpuAdapterSelect Default = new(isIndex: false, index: 0);

    /// <summary>Select a specific adapter by <see cref="GpuAdapterInfo.Index"/> (from <see cref="GpuDevice.ListAdapters"/>).</summary>
    public static GpuAdapterSelect Index(uint index) => new(isIndex: true, index: index);

    internal NativeGpuAdapterSelect ToNative() => new()
    {
        Kind = _isIndex ? NativeGpuAdapterSelectKind.Index : NativeGpuAdapterSelectKind.Default,
        Index = _index,
    };
}
