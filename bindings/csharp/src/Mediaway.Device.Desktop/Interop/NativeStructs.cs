using System.Runtime.InteropServices;
using Mediaway.Common;

namespace Mediaway.Device.Desktop.Interop;

// Field order/sizes mirror crates/mediaway-ffi/src/desktop_video.rs +
// desktop_audio.rs + types.rs exactly (LayoutKind.Sequential preserves declaration
// order); native `bool` (1 byte) is a `byte` field here, not C# `bool` (4 bytes), so
// every struct stays fully blittable.

[StructLayout(LayoutKind.Sequential)]
internal readonly struct NativeRational
{
    public readonly ulong Num;
    public readonly uint Den;

    public NativeRational(Rational value)
    {
        Num = value.Num;
        Den = value.Den;
    }
}

internal enum NativeDesktopCaptureSourceKind
{
    Screen = 0,
    Window = 1,
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeDesktopCaptureConfig
{
    public NativeDesktopCaptureSourceKind SourceKind;
    public uint SourceIndex;
    public NativeRational TimeBase;
    public GpuDeviceHandle GpuDevice;

    // Appended by adr/device/0005-window-capture-c-abi.md (device ABI 2). Every tail field's
    // zero value is the previous behaviour, so a `new()` value still captures the whole
    // surface with no pointer, the OS default border and native dimensions. The public enums
    // are reused directly: their values are the native ones (0/1, 4 bytes).
    public ulong WindowHandle;
    public CursorCapture Cursor;
    public WindowBorder Border;
    public FrameDimensions Dimensions;
    public uint RegionX;
    public uint RegionY;
    public uint RegionWidth;
    public uint RegionHeight;
    public byte RegionEnabled;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeDesktopFrame
{
    public long Pts;
    public ulong Duration;
    public uint Width;
    public uint Height;
    public PixelFormat PixelFormat;
    public VideoFrameStorageKind StorageKind;
    public nint Data;
    public nuint DataLen;
    public GpuBufferHandle GpuBuffer;
}

internal enum NativeDesktopAudioSourceKind
{
    Loopback = 0,
    ProcessLoopback = 1,
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeDesktopAudioCaptureConfig
{
    public NativeDesktopAudioSourceKind SourceKind;
    public uint DeviceIndex;
    public uint ProcessId;
    public byte IncludeTargetProcessTree;
    public NativeRational TimeBase;
    public SampleFormat SampleFormat;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeDesktopAudioFrame
{
    public long Pts;
    public ulong Duration;
    public uint SampleRate;
    public ushort Channels;
    public SampleFormat SampleFormat;
    public nint Data;
    public nuint DataLen;
}
