using System.Runtime.InteropServices;
using Mediaway.Common;

namespace Mediaway.Container.Interop;

// Field order/sizes below mirror crates/mediaway-ffi/include/mediaway/container.h
// exactly (LayoutKind.Sequential preserves declaration order); native `bool` (1 byte, per
// Rust's `#[repr(C)] bool`) is represented as `byte` here rather than C# `bool` (4 bytes by
// default) so every struct stays fully blittable — no per-field MarshalAs needed.

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

    public Rational ToManaged() => new(Num, Den);
}

[StructLayout(LayoutKind.Sequential)]
internal unsafe struct NativeVideoTrackInfo
{
    public uint Id;
    public CodecKind Codec;
    public NativeRational TimeBase;
    public uint Width;
    public uint Height;
    public byte* ExtraData;
    public nuint ExtraDataLen;
}

[StructLayout(LayoutKind.Sequential)]
internal unsafe struct NativeAudioTrackInfo
{
    public uint Id;
    public CodecKind Codec;
    public NativeRational TimeBase;
    public uint SampleRate;
    public ushort Channels;
    public byte* ExtraData;
    public nuint ExtraDataLen;
}

[StructLayout(LayoutKind.Sequential)]
internal unsafe struct NativePacketView
{
    public uint StreamId;
    public long Pts;
    public long Dts;
    public ulong Duration;
    public byte IsKeyframe;
    public byte IsDiscard;
    public byte* Payload;
    public nuint PayloadLen;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativePacket
{
    public uint StreamId;
    public long Pts;
    public long Dts;
    public ulong Duration;
    public byte IsKeyframe;
    public byte IsDiscard;
    public nint Payload;
    public nuint PayloadLen;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeStreamInfo
{
    public uint Id;
    public CodecKind Codec;
    public NativeRational TimeBase;
    public byte HasGeometry;
    public uint Width;
    public uint Height;
    public uint SampleRate;
    public ushort Channels;
    public nint ExtraData;
    public nuint ExtraDataLen;
}

/// <summary>One elementary stream in <see cref="TsMuxer"/>'s constructed PMT.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct NativeTsElementaryStream
{
    public ushort Pid;
    public CodecKind Codec;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeMp3FrameHeader
{
    public Mp3MpegVersion Version;
    public ushort BitrateKbps;
    public uint SampleRate;
    public Mp3ChannelMode ChannelMode;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeWaveFormat
{
    public WavSampleFormat SampleFormat;
    public ushort Channels;
    public uint SampleRate;
    public ushort BitsPerSample;
}

// -- Replay ring and placements (adr/container/0009-replay-ring-c-abi.md) --------------
// Offsets/sizes (64-bit) verified against the real container.h with a gcc probe and pinned by
// ReplayLayoutTests: config 48, packet meta 40, stored payload 24, clip entry 80, placement 32.

[StructLayout(LayoutKind.Sequential)]
internal struct NativeReplayRingConfig
{
    public uint AnchorStreamId;
    public NativeRational AnchorTimeBase;
    public ulong WindowMs;
    public ulong MaxBytes;
    public int PayloadKind;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativePacketMeta
{
    public uint StreamId;
    public long Pts;
    public long Dts;
    public ulong Duration;
    public byte IsKeyframe;
    public byte IsDiscard;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeStoredPayload
{
    public uint File;
    public ulong Offset;
    public uint Len;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeReplayClipEntry
{
    public uint StreamId;
    public long Pts;
    public long Dts;
    public ulong Duration;
    public byte IsKeyframe;
    public byte IsDiscard;
    public int PayloadKind;
    public nint Payload;
    public nuint PayloadLen;
    public uint StoredFile;
    public ulong StoredOffset;
    public uint StoredLen;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativePlacement
{
    public uint TrackId;
    public long Dts;
    public ulong Offset;
    public uint Len;
}
