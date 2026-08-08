using System.Buffers;
using Mediaway.Common.Interop;
using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>
/// Appends raw PCM and finalizes a complete RIFF/WAVE byte stream. Mux-only: RIFF chunk
/// sizes must be known up front, so there is no demux counterpart class — use the one-shot
/// <see cref="WavContainer.Parse"/> function instead.
/// </summary>
public sealed class WavMuxer : IDisposable
{
    private readonly WavMuxerHandle _handle;
    private bool _finished;

    /// <summary>Start an integer-PCM mux session.</summary>
    public WavMuxer(uint sampleRate, ushort channels, ushort bitsPerSample) =>
        _handle = WavMuxerHandle.Create(sampleRate, channels, bitsPerSample);

    /// <summary>Start a mux session for an explicit format (e.g. IEEE float PCM).</summary>
    public WavMuxer(WaveFormat format)
    {
        var native = new NativeWaveFormat
        {
            SampleFormat = format.SampleFormat,
            Channels = format.Channels,
            SampleRate = format.SampleRate,
            BitsPerSample = format.BitsPerSample,
        };
        _handle = WavMuxerHandle.CreateWithFormat(in native);
    }

    /// <summary>Append raw interleaved PCM bytes, already encoded per the session's format.</summary>
    public unsafe void PushPacket(Packet packet)
    {
        using var pin = packet.Payload.Pin();
        var native = new NativePacketView
        {
            StreamId = packet.StreamId,
            Pts = packet.Pts,
            Dts = packet.Dts,
            Duration = packet.Duration,
            IsKeyframe = (byte)(packet.IsKeyframe ? 1 : 0),
            IsDiscard = (byte)(packet.IsDiscard ? 1 : 0),
            Payload = packet.Payload.IsEmpty ? null : (byte*)pin.Pointer,
            PayloadLen = (nuint)packet.Payload.Length,
        };
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_wav_muxer_push_packet(_handle, in native));
    }

    /// <summary>
    /// Finalize the mux session and return the complete RIFF/WAVE byte stream. Only the
    /// native-side internal state is consumed — unlike <see cref="Muxer.Begin"/>, this
    /// <see cref="WavMuxer"/> instance stays usable (for <see cref="Dispose"/>) afterward. A
    /// second call fails with <see cref="MediawayContainerStatus.InvalidState"/> rather than
    /// re-finalizing.
    /// </summary>
    public IMemoryOwner<byte> Finish()
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_wav_muxer_finish(_handle, out var data, out var len));
        _finished = true;

        return data == 0 || len == 0
            ? EmptyMemoryOwner<byte>.Instance
            : new NativeOwnedMemoryManager(data, len, static (p, l) => NativeMethods.mediaway_buffer_free(p, l));
    }

    /// <summary>Whether <see cref="Finish"/> has already been called on this session.</summary>
    public bool IsFinished => _finished;

    public void Dispose() => _handle.Dispose();
}
