using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>Result of <see cref="WavContainer.Parse"/>: the single track's info and its one whole-payload packet.</summary>
public readonly record struct WavParseResult(StreamDescriptor Info, Packet Packet) : IDisposable
{
    /// <summary>Releases the native buffers backing both <see cref="Info"/> and <see cref="Packet"/>.</summary>
    public void Dispose()
    {
        Info.Dispose();
        Packet.Dispose();
    }
}

/// <summary>One-shot RIFF/WAVE parsing — not a demuxer handle, since RIFF/WAVE carries no internal frame boundaries.</summary>
public static class WavContainer
{
    /// <summary>
    /// Parse a complete RIFF/WAVE buffer into its single track's stream info and one packet
    /// holding the whole PCM payload.
    /// </summary>
    public static unsafe WavParseResult Parse(ReadOnlySpan<byte> data)
    {
        fixed (byte* ptr = data)
        {
            MediawayContainerException.ThrowIfError(
                NativeMethods.mediaway_wav_parse(ptr, (nuint)data.Length, out var info, out var packet));
            return new WavParseResult(NativeConversions.ToManaged(info), NativeConversions.ToManaged(packet));
        }
    }
}
