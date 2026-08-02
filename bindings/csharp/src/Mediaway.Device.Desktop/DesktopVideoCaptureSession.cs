using System.Buffers;
using Mediaway.Common;
using Mediaway.Common.Interop;
using Mediaway.Device.Desktop.Interop;

namespace Mediaway.Device.Desktop;

internal sealed class DesktopVideoCaptureSession : IDesktopVideoCapture
{
    private readonly DesktopCaptureHandle _handle;

    private DesktopVideoCaptureSession(DesktopCaptureHandle handle, uint width, uint height)
    {
        _handle = handle;
        Width = width;
        Height = height;
    }

    public uint Width { get; }

    public uint Height { get; }

    internal static DesktopVideoCaptureSession OpenFrom(DesktopCaptureHandle handle)
    {
        var status = NativeMethods.mediaway_desktop_capture_geometry(handle, out uint width, out uint height);
        MediawayDeviceException.ThrowIfError(status);
        return new DesktopVideoCaptureSession(handle, width, height);
    }

    public bool TryPollFrame(out DesktopVideoFrame? frame)
    {
        var status = NativeMethods.mediaway_desktop_capture_poll_frame(_handle, out var native, out byte hasFrame);
        MediawayDeviceException.ThrowIfError(status);
        if (hasFrame == 0)
        {
            frame = null;
            return false;
        }

        // Deliberately no auto-ReleaseFrame here (unlike Camera's session) — see
        // IDesktopVideoCapture's own docs for why a GPU-backed frame's release must stay
        // caller-driven.
        frame = ToManaged(native);
        return true;
    }

    public void ReleaseFrame() =>
        MediawayDeviceException.ThrowIfError(NativeMethods.mediaway_desktop_capture_release_frame(_handle));

    private static DesktopVideoFrame ToManaged(NativeDesktopFrame native)
    {
        if (native.StorageKind == VideoFrameStorageKind.Cpu)
        {
            IMemoryOwner<byte> owner = native.DataLen == 0
                ? EmptyMemoryOwner<byte>.Instance
                : new NativeOwnedMemoryManager(
                    native.Data, native.DataLen,
                    static (ptr, len) =>
                    {
                        var owned = new NativeDesktopFrame { Data = ptr, DataLen = len };
                        NativeMethods.mediaway_desktop_frame_free(ref owned);
                    });

            return new DesktopVideoFrame(owner)
            {
                Pts = native.Pts,
                Duration = native.Duration,
                Width = native.Width,
                Height = native.Height,
                PixelFormat = native.PixelFormat,
                StorageKind = VideoFrameStorageKind.Cpu,
                Data = owner.Memory,
                GpuBuffer = default,
            };
        }

        // Gpu case: nothing owned by this frame object at all — mediaway_desktop_frame_free
        // is a documented no-op here (data/data_len are already null/0), and the real
        // texture is released via IDesktopVideoCapture.ReleaseFrame, not frame disposal.
        return new DesktopVideoFrame(EmptyMemoryOwner<byte>.Instance)
        {
            Pts = native.Pts,
            Duration = native.Duration,
            Width = native.Width,
            Height = native.Height,
            PixelFormat = native.PixelFormat,
            StorageKind = VideoFrameStorageKind.Gpu,
            Data = ReadOnlyMemory<byte>.Empty,
            GpuBuffer = native.GpuBuffer,
        };
    }

    public void Dispose() => _handle.Dispose();
}
