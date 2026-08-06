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

    // gop_size/rate_control_* (ABI v6) — NOT YET HONORED by the auto-selected backend
    // on any platform (WMF here) — see VideoEncodeConfig.GopSize/RateControl's doc
    // comments. Native `bool` (1 byte) is `byte` here, same reasoning as
    // NativeAudioPacket.IsKeyframe.
    public uint GopSize;
    public byte RateControlEnabled;
    public uint RateControlTargetBitrateBps;
    public uint RateControlVbvBufferSizeBytes;
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

// ── Audio encode (adr/0003-auto-audio-encode-c-abi.md) ──────────────────────────────
//
// Codec below is Mediaway.Common.CodecKind, not Mediaway.Pipeline.VideoCodec — the native
// mediaway_pipeline_codec_kind_t mirrors CodecKind's numeric values 1:1 (see pipeline.h's own
// comment), and AAC (4) falls outside VideoCodec's H264..Vp9 range. Native `bool` (1 byte) is
// `byte` here for the same blittability reason as mediaway-container-ffi's NativePacketView.

[StructLayout(LayoutKind.Sequential)]
internal struct NativeAudioEncodeConfig
{
    public CodecKind Codec;
    public uint SampleRate;
    public ushort Channels;
    public SampleFormat SampleFormat;
    public NativeRational TimeBase;
    public uint BitrateBps;
}

[StructLayout(LayoutKind.Sequential)]
internal unsafe struct NativeAudioFrameView
{
    public long Pts;
    public ulong Duration;
    public uint SampleRate;
    public ushort Channels;
    public SampleFormat SampleFormat;
    public byte* Data;
    public nuint DataLen;
}

[StructLayout(LayoutKind.Sequential)]
internal unsafe struct NativeAudioPacket
{
    public long Pts;
    public long Dts;
    public ulong Duration;
    public byte IsKeyframe;
    public byte IsDiscard;
    public byte* Payload;
    public nuint PayloadLen;
}

[StructLayout(LayoutKind.Sequential)]
internal unsafe struct NativeAudioStreamInfo
{
    public CodecKind Codec;
    public NativeRational TimeBase;
    public uint SampleRate;
    public ushort Channels;
    public byte* ExtraData;
    public nuint ExtraDataLen;
}
