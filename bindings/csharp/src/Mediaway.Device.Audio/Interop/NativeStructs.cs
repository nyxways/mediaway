using System.Runtime.InteropServices;
using Mediaway.Common;

namespace Mediaway.Device.Audio.Interop;

// Field order/sizes mirror crates/mediaway-ffi/src/audio.rs + types.rs exactly
// (LayoutKind.Sequential preserves declaration order); native `bool` (1 byte) is a `byte`
// field here, not C# `bool` (4 bytes), so every struct stays fully blittable.

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

/// <summary>
/// Microphone only — no source-kind tag. Loopback/ProcessLoopback moved to
/// <c>Mediaway.Device.Desktop</c> (`adr/0004-domain-feature-split.md`).
/// </summary>
[StructLayout(LayoutKind.Sequential)]
internal struct NativeAudioCaptureConfig
{
    public uint DeviceIndex;
    public NativeRational TimeBase;
    public SampleFormat SampleFormat;
}

[StructLayout(LayoutKind.Sequential)]
internal struct NativeDeviceAudioFrame
{
    public long Pts;
    public ulong Duration;
    public uint SampleRate;
    public ushort Channels;
    public SampleFormat SampleFormat;
    public nint Data;
    public nuint DataLen;
}
