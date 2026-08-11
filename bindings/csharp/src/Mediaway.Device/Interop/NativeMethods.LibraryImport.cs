#if NET8_0_OR_GREATER
using System.Runtime.InteropServices;
using Mediaway.Common;

namespace Mediaway.Device.Interop;

internal static unsafe partial class NativeMethods
{
    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_gpu_adapter_list(
        out nint outAdapters, out nuint outCount);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_gpu_adapter_list_free(nint adapters, nuint count);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_gpu_device_create(
        in NativeGpuDeviceOptions options, out nint outDevice);

    [LibraryImport(LibraryName)]
    internal static partial MediawayDeviceStatus mediaway_gpu_device_handle(
        GpuDeviceSessionHandle device, out GpuDeviceHandle outHandle);

    [LibraryImport(LibraryName)]
    internal static partial void mediaway_gpu_device_close(nint device);
}
#endif
