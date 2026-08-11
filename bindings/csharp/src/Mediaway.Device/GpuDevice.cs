using System.Runtime.InteropServices;
using Mediaway.Common;
using Mediaway.Device.Interop;

namespace Mediaway.Device;

/// <summary>
/// Owns a real GPU device (e.g. a DirectX11 <c>ID3D11Device</c>) created by the native
/// backend — closes the "no C-ABI caller can construct a GPU device" gap for Screen
/// capture (<c>Mediaway.Device.Desktop.DesktopScreenCapture</c>) and GPU-input encode
/// (<c>Mediaway.Pipeline.VideoEncodeConfig.GpuDevice</c>), both of which require a live
/// device handle with no CPU fallback.
/// </summary>
public sealed class GpuDevice : IDisposable
{
    private readonly GpuDeviceSessionHandle _handle;

    private GpuDevice(GpuDeviceSessionHandle handle, GpuDeviceHandle deviceHandle)
    {
        _handle = handle;
        Handle = deviceHandle;
    }

    /// <summary>
    /// The caller-facing handle — pass this into
    /// <c>Mediaway.Device.Desktop.DesktopScreenCapture.Open</c> or a
    /// <c>VideoEncodeConfig.GpuDevice</c> field. Stays valid only while this
    /// <see cref="GpuDevice"/> is not disposed.
    /// </summary>
    public GpuDeviceHandle Handle { get; }

    /// <summary>Enumerate every DXGI adapter on this machine (name, VRAM, hardware-vs-software).</summary>
    public static unsafe IReadOnlyList<GpuAdapterInfo> ListAdapters()
    {
        var status = NativeMethods.mediaway_gpu_adapter_list(out nint adaptersPtr, out nuint count);
        MediawayDeviceException.ThrowIfError(status);

        try
        {
            var result = new List<GpuAdapterInfo>((int)count);
            var entries = (NativeGpuAdapterInfo*)adaptersPtr;
            for (nuint i = 0; i < count; i++)
            {
                var entry = entries[i];
                result.Add(new GpuAdapterInfo
                {
                    Index = entry.Index,
                    Name = PtrToStringUtf8((nint)entry.Name) ?? string.Empty,
                    VendorId = entry.VendorId,
                    DeviceId = entry.DeviceId,
                    DedicatedVideoMemory = entry.DedicatedVideoMemory,
                    IsHardware = entry.IsHardware != 0,
                });
            }

            return result;
        }
        finally
        {
            NativeMethods.mediaway_gpu_adapter_list_free(adaptersPtr, count);
        }
    }

    /// <summary>Create a real GPU device from <paramref name="options"/> (default or an explicit adapter index).</summary>
    /// <exception cref="CaptureUnavailableException">No supported GPU device backend is compiled in here.</exception>
    public static GpuDevice Create(GpuDeviceOptions options)
    {
        var native = BuildOptions(options);
        var status = NativeMethods.mediaway_gpu_device_create(in native, out nint devicePtr);
        MediawayDeviceException.ThrowIfError(status);
        return OpenFrom(GpuDeviceSessionHandle.Wrap(devicePtr));
    }

    /// <summary>Non-throwing form of <see cref="Create"/> — returns <see langword="null"/> and the failure status instead of throwing.</summary>
    public static GpuDevice? TryCreate(GpuDeviceOptions options, out MediawayDeviceStatus? error)
    {
        var native = BuildOptions(options);
        var status = NativeMethods.mediaway_gpu_device_create(in native, out nint devicePtr);
        if (status != MediawayDeviceStatus.Ok)
        {
            error = status;
            return null;
        }

        error = null;
        return OpenFrom(GpuDeviceSessionHandle.Wrap(devicePtr));
    }

    private static GpuDevice OpenFrom(GpuDeviceSessionHandle handle)
    {
        var status = NativeMethods.mediaway_gpu_device_handle(handle, out GpuDeviceHandle deviceHandle);
        MediawayDeviceException.ThrowIfError(status);
        return new GpuDevice(handle, deviceHandle);
    }

    private static NativeGpuDeviceOptions BuildOptions(GpuDeviceOptions options) => new()
    {
        Adapter = options.Adapter.ToNative(),
        VideoSupport = options.VideoSupport ? (byte)1 : (byte)0,
        DebugLayer = options.DebugLayer ? (byte)1 : (byte)0,
    };

    /// <summary>
    /// Null-terminated UTF-8 C string -&gt; managed string, or <see langword="null"/> for a
    /// null pointer. <c>Marshal.PtrToStringUTF8</c> only exists from netstandard2.1/net5.0+ —
    /// this hand-rolled netstandard2.0 fallback walks the buffer itself instead of adding a
    /// dependency for one call site (see docs/adr/0018-csharp-netstandard20-unity.md).
    /// </summary>
    private static unsafe string? PtrToStringUtf8(nint ptr)
    {
#if NET8_0_OR_GREATER
        return Marshal.PtrToStringUTF8(ptr);
#else
        if (ptr == 0)
        {
            return null;
        }

        var p = (byte*)ptr;
        int len = 0;
        while (p[len] != 0)
        {
            len++;
        }

        return System.Text.Encoding.UTF8.GetString(p, len);
#endif
    }

    /// <summary>Releases the native device. Every handle obtained from it becomes invalid immediately.</summary>
    public void Dispose() => _handle.Dispose();
}
