using Mediaway.Common;
using Mediaway.Pipeline.Interop;

namespace Mediaway.Pipeline;

/// <summary>
/// An audio decode session — the handle IS the decoder
/// (<c>adr/pipeline/0006-audio-decode-c-abi.md</c>, <c>adr/pipeline/0007-…</c>; mirrors
/// <see cref="DecodeSession"/>'s video shape; no muxer to wire, no consumption trap).
/// <list type="bullet">
/// <item><see cref="Open"/> — Opus, the software decoder (<c>mediaway-sw</c>): cross-platform
/// and byte-identical on every host.</item>
/// <item><see cref="OpenAac"/> — AAC, <b>the operating system's own decoder</b> (Media
/// Foundation on Windows, AudioToolbox on macOS/iOS): its samples can differ between hosts and
/// OS versions, and it does not exist elsewhere. Ask <see cref="DecoderSupport.Query"/> first.
/// The Apple arm is compile-checked but has not been run.</item>
/// </list>
/// </summary>
public sealed class AudioDecodeSession : IDisposable
{
    private readonly AudioDecodeSessionHandle _handle;

    private AudioDecodeSession(AudioDecodeSessionHandle handle) => _handle = handle;

    /// <summary>Open an Opus (software) decode session for <paramref name="sampleRate"/>/<paramref name="channels"/>/<paramref name="timeBase"/>.</summary>
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
    /// Open an AAC decode session. <paramref name="audioSpecificConfig"/> is the stream's raw
    /// <c>AudioSpecificConfig</c> (an MP4's <c>esds</c> <c>DecoderSpecificInfo</c>, or
    /// <see cref="AudioStreamInfo.ExtraData"/> from <see cref="AudioEncoder"/>) and is
    /// <b>required</b>: an empty one throws <see cref="MediawayPipelineException"/>
    /// (<see cref="MediawayPipelineStatus.InvalidInput"/>), because a synthesised default would
    /// decode SBR/PS streams to quietly wrong output. Raw AAC only — de-header ADTS first.
    /// The native side copies the bytes during this call; nothing is retained.
    /// </summary>
    /// <param name="timeBase">Normally <c>1 / sampleRate</c>, so packet and frame timestamps are sample counts.</param>
    /// <exception cref="DecoderUnavailableException">No AAC decode backend is compiled in here.</exception>
    /// <exception cref="MediawayPipelineException">
    /// <see cref="MediawayPipelineStatus.Unsupported"/> on a platform with no OS AAC decoder;
    /// <see cref="MediawayPipelineStatus.InvalidInput"/> for an empty config or a zero rate/channel count.
    /// </exception>
    public static unsafe AudioDecodeSession OpenAac(
        uint sampleRate, ushort channels, Rational timeBase, ReadOnlyMemory<byte> audioSpecificConfig)
    {
        // Pinned for the call only: the native side borrows the bytes during open() and copies
        // what it needs (adr/pipeline/0007 §2).
        using var pin = audioSpecificConfig.Pin();
        var native = new NativeAudioDecodeConfig
        {
            Codec = CodecKind.Aac,
            SampleRate = sampleRate,
            Channels = channels,
            TimeBase = new NativeRational(timeBase),
            ExtraData = audioSpecificConfig.IsEmpty ? null : (byte*)pin.Pointer,
            ExtraDataLen = (nuint)audioSpecificConfig.Length,
        };

        var status = NativeMethods.mediaway_audio_decode_session_open(in native, out nint session);
        MediawayPipelineException.ThrowIfDecodeError(
            status, "No supported AAC decode backend is compiled in on this platform.");
        return new AudioDecodeSession(AudioDecodeSessionHandle.Wrap(session));
    }

    /// <summary>
    /// Push one compressed packet (Opus, or raw AAC for a session from <see cref="OpenAac"/>).
    /// May produce zero or more frames (drain via <see cref="PollFrame"/>).
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
