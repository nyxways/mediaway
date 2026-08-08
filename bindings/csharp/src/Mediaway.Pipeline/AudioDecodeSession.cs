using Mediaway.Common;
using Mediaway.Pipeline.Interop;

namespace Mediaway.Pipeline;

/// <summary>
/// An Opus audio decode session — the handle IS the decoder
/// (<c>adr/pipeline/0006-audio-decode-c-abi.md</c>, mirrors <see cref="DecodeSession"/>'s
/// video shape; no muxer to wire, no consumption trap). Cross-platform (<c>mediaway-sw</c>,
/// no OS dependency), unlike <see cref="DecodeSession"/>'s Windows-only WMF backend.
/// </summary>
public sealed class AudioDecodeSession : IDisposable
{
    private readonly AudioDecodeSessionHandle _handle;

    private AudioDecodeSession(AudioDecodeSessionHandle handle) => _handle = handle;

    /// <summary>Open an Opus decode session for <paramref name="sampleRate"/>/<paramref name="channels"/>/<paramref name="timeBase"/>.</summary>
    /// <exception cref="DecoderUnavailableException">
    /// No supported Opus decode backend is compiled in here — an expected, graceful outcome
    /// to catch and handle, not a bug.
    /// </exception>
    public static AudioDecodeSession Open(uint sampleRate, ushort channels, Rational timeBase)
    {
        var native = new NativeAudioDecodeConfig
        {
            Codec = CodecKind.Opus,
            SampleRate = sampleRate,
            Channels = channels,
            TimeBase = new NativeRational(timeBase),
        };

        var status = NativeMethods.mediaway_audio_decode_session_open(in native, out nint session);
        MediawayPipelineException.ThrowIfDecodeError(
            status, "No supported Opus decode backend is compiled in on this platform.");
        return new AudioDecodeSession(AudioDecodeSessionHandle.Wrap(session));
    }

    /// <summary>
    /// Push one compressed Opus packet. May produce zero or more frames (drain via
    /// <see cref="PollFrame"/>).
    /// </summary>
    public unsafe void PushPacket(DecodePacket packet)
    {
        using var pin = packet.Payload.Pin();
        var native = new NativeDecodePacketView
        {
            StreamId = 0,
            Pts = packet.Pts,
            Dts = packet.Dts,
            Duration = packet.Duration,
            IsKeyframe = (byte)(packet.IsKeyframe ? 1 : 0),
            IsDiscard = 0,
            Payload = packet.Payload.IsEmpty ? null : (byte*)pin.Pointer,
            PayloadLen = (nuint)packet.Payload.Length,
        };
        MediawayPipelineException.ThrowIfError(
            NativeMethods.mediaway_audio_decode_session_push_packet(_handle, in native));
    }

    /// <summary>
    /// Pull the next decoded PCM frame, if any is ready. <see langword="null"/> is a valid
    /// "nothing ready yet" result, not an error.
    /// </summary>
    public unsafe DecodedAudioFrame? PollFrame()
    {
        MediawayPipelineException.ThrowIfError(NativeMethods.mediaway_audio_decode_session_poll_frame(
            _handle, out NativeDecodedAudioFrame native, out byte hasFrame));

        if (hasFrame == 0)
        {
            return null;
        }

        var frame = new DecodedAudioFrame
        {
            Pts = native.Pts,
            Duration = native.Duration,
            SampleRate = native.SampleRate,
            Channels = native.Channels,
            Data = native.Data is null
                ? Array.Empty<byte>()
                : new ReadOnlySpan<byte>(native.Data, (int)native.DataLen).ToArray(),
        };
        NativeMethods.mediaway_decoded_audio_frame_free(ref native);
        return frame;
    }

    /// <summary>Signal end-of-input; drain the remaining frames with <see cref="PollFrame"/> afterward.</summary>
    public void Flush() =>
        MediawayPipelineException.ThrowIfError(NativeMethods.mediaway_audio_decode_session_flush(_handle));

    /// <summary>Releases the native session. Always safe to call — this surface has no handle-consumption trap.</summary>
    public void Dispose() => _handle.Dispose();
}
