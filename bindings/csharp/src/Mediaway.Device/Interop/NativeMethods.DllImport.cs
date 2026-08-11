#if !NET8_0_OR_GREATER
using System.Runtime.InteropServices;
using Mediaway.Common;

namespace Mediaway.Device.Interop;

internal static unsafe partial class NativeMethods
{
    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_gpu_adapter_list(
        out nint outAdapters, out nuint outCount);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_gpu_adapter_list_free(nint adapters, nuint count);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_gpu_device_create(
        in NativeGpuDeviceOptions options, out nint outDevice);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern MediawayDeviceStatus mediaway_gpu_device_handle(
        GpuDeviceSessionHandle device, out GpuDeviceHandle outHandle);

    [DllImport(LibraryName, ExactSpelling = true)]
    internal static extern void mediaway_gpu_device_close(nint device);
}
#endif
