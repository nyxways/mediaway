using System.Buffers;
using Mediaway.Common.Interop;
using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>
/// Appends encoded MPEG Layer III frames. <paramref name="header"/> — the bitrate/sample
/// rate/channel mode — stays constant for the whole mux session's lifetime; there is no
/// track registration at all.
/// </summary>
public sealed class Mp3Muxer : IDisposable
{
    private readonly Mp3MuxerHandle _handle;

    /// <param name="header">
    /// Must be a standard Layer III bitrate/sample-rate combination for its
    /// <see cref="Mp3FrameHeader.Version"/>.
    /// </param>
    public Mp3Muxer(Mp3FrameHeader header)
    {
        var native = new NativeMp3FrameHeader
        {
            Version = header.Version,
            BitrateKbps = header.BitrateKbps,
            SampleRate = header.SampleRate,
            ChannelMode = header.ChannelMode,
        };
        _handle = Mp3MuxerHandle.Create(in native);
    }

    /// <summary>
    /// Append one already-encoded Layer III frame body. Fails with
    /// <see cref="MediawayContainerStatus.InvalidPacket"/> when <paramref name="frameBody"/>'s
    /// length doesn't match what the header's bitrate/sample-rate/padding combination requires.
    /// </summary>
    public unsafe IMemoryOwner<byte> WriteFrame(ReadOnlySpan<byte> frameBody, bool padding)
    {
        fixed (byte* ptr = frameBody)
        {
            MediawayContainerException.ThrowIfError(NativeMethods.mediaway_mp3_muxer_write_frame(
                _handle, ptr, (nuint)frameBody.Length, (byte)(padding ? 1 : 0), out var data, out var len));
            return data == 0 || len == 0
                ? EmptyMemoryOwner<byte>.Instance
                : new NativeOwnedMemoryManager(data, len, static (p, l) => NativeMethods.mediaway_buffer_free(p, l));
        }
    }

    public void Dispose() => _handle.Dispose();
}
