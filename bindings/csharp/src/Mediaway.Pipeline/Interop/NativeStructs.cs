using System.Runtime.InteropServices;
using Mediaway.Common;

namespace Mediaway.Pipeline.Interop;

// Field order/sizes mirror crates/mediaway-ffi/include/mediaway/pipeline.h exactly
// (LayoutKind.Sequential preserves declaration order).

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

[StructLayout(LayoutKind.Sequential)]
internal struct NativeAutoVideoEncodeConfig
{
    public VideoCodec Codec;
    public uint Width;
    public uint Height;
    public NativeRational TimeBase;
    public uint BitrateBps;
    public PixelFormat PixelFormat;
    public GpuDeviceHandle GpuDevice;
}

[StructLayout(LayoutKind.Sequential)]
internal unsafe struct NativeVideoFrame
{
    public long Pts;
    public ulong Duration;
    public uint Width;
    public uint Height;
    public PixelFormat PixelFormat;
    public VideoFrameStorageKind StorageKind;
    public byte* RawBytes;
    public nuint RawBytesLen;
    public GpuBufferHandle GpuBuffer;
}
