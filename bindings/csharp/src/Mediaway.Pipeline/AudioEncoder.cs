using Mediaway.Common;
using Mediaway.Pipeline.Interop;

namespace Mediaway.Pipeline;

/// <summary>
/// A live AAC audio encode session. The returned handle IS the session
/// (adr/0003-auto-audio-encode-c-abi.md in mediaway-ffi: audio has no internal muxer, so
/// there is no intermediate encoder handle and no handle-consumption trap, unlike
/// <see cref="AutoVideoEncoder"/>/<see cref="EncodeSession"/>). Push PCM via
/// <see cref="PushPcm"/>, call <see cref="Flush"/>, then drain <see cref="PollPacket"/> until
/// it returns <see langword="null"/>; read <see cref="StreamInfo"/> after the first pushed
/// frame to register a <c>Mediaway.Container.Muxer</c> audio track for the encoded stream.
/// </summary>
public sealed class AudioEncoder : IDisposable
{
    private readonly AudioEncodeSessionHandle _handle;

    private AudioEncoder(AudioEncodeSessionHandle handle) => _handle = handle;

    /// <summary>
    /// Open the best available AAC audio encoder for <paramref name="config"/>. Only F32
    /// input PCM is accepted by the real Windows backend today — fixed internally, the
    /// native config carries no format choice yet.
    /// </summary>
    /// <exception cref="EncoderUnavailableException">
    /// No supported audio encoder backend is compiled in here — an expected, graceful
    /// outcome to catch and handle, not a bug.
    /// </exception>
    public static AudioEncoder Open(AudioEncodeConfig config)
    {
        var native = new NativeAudioEncodeConfig
        {
            Codec = CodecKind.Aac,
            SampleRate = config.SampleRate,
            Channels = config.Channels,
            SampleFormat = SampleFormat.F32,
            TimeBase = new NativeRational(config.TimeBase),
            BitrateBps = config.BitrateBps,
        };

        var status = NativeMethods.mediaway_audio_encoder_open(in native, out nint session);
        MediawayPipelineException.ThrowIfError(
            status, "No supported audio encoder backend is compiled in on this platform.");
        return new AudioEncoder(AudioEncodeSessionHandle.Wrap(session));
    }

    /// <summary>
    /// Push one PCM buffer. <paramref name="frame"/>'s <see cref="AudioFrame.Data"/> is
    /// borrowed for the call only — the encoder copies synchronously (same cost class as the
    /// video CPU-upload path).
    /// </summary>
    public unsafe void PushPcm(AudioFrame frame)
    {
        using var pin = frame.Data.Pin();
        var native = new NativeAudioFrameView
        {
            Pts = frame.Pts,
            Duration = frame.Duration,
            SampleRate = frame.SampleRate,
            Channels = frame.Channels,
            SampleFormat = SampleFormat.F32,
            Data = frame.Data.IsEmpty ? null : (byte*)pin.Pointer,
            DataLen = (nuint)frame.Data.Length,
        };
        MediawayPipelineException.ThrowIfError(
            NativeMethods.mediaway_audio_encode_session_push_pcm(_handle, in native));
    }

    /// <summary>
    /// Query the session's stream metadata. <see cref="AudioStreamInfo.ExtraData"/> (the
    /// AudioSpecificConfig) materializes only after the first pushed frame — call this after
    /// <see cref="PushPcm"/>, before registering a muxer track.
    /// </summary>
    public unsafe AudioStreamInfo StreamInfo()
    {
        MediawayPipelineException.ThrowIfError(
            NativeMethods.mediaway_audio_encode_session_stream_info(_handle, out NativeAudioStreamInfo native));

        var codec = native.Codec;
        var timeBase = new Rational(native.TimeBase.Num, native.TimeBase.Den);
        var sampleRate = native.SampleRate;
        var channels = native.Channels;
        // Copied out immediately (the AudioSpecificConfig blob is ~15-20 bytes) so the native
        // buffer can be freed right away — not worth an IMemoryOwner for output this small.
        var extraData = native.ExtraData is null
            ? Array.Empty<byte>()
            : new ReadOnlySpan<byte>(native.ExtraData, (int)native.ExtraDataLen).ToArray();
        NativeMethods.mediaway_pipeline_ffi_stream_info_free(ref native);

        return new AudioStreamInfo
        {
            Codec = codec,
            TimeBase = timeBase,
            SampleRate = sampleRate,
            Channels = channels,
            ExtraData = extraData,
        };
    }

    /// <summary>
    /// Pull the next encoded packet, if one is ready. <see langword="null"/> is a valid
    /// "nothing ready yet" result, not an error.
    /// </summary>
    public unsafe AudioPacket? PollPacket()
    {
        MediawayPipelineException.ThrowIfError(NativeMethods.mediaway_audio_encode_session_poll_packet(
            _handle, out NativeAudioPacket native, out byte hasPacket));

        if (hasPacket == 0)
        {
            return null;
        }

        var packet = new AudioPacket
        {
            Pts = native.Pts,
            Dts = native.Dts,
            Duration = native.Duration,
            IsKeyframe = native.IsKeyframe != 0,
            IsDiscard = native.IsDiscard != 0,
            // Copied out here (one AAC frame, typically small) so the native packet can be
            // freed immediately — same reasoning as StreamInfo's extra_data above.
            Payload = native.Payload is null
                ? Array.Empty<byte>()
                : new ReadOnlySpan<byte>(native.Payload, (int)native.PayloadLen).ToArray(),
        };
        NativeMethods.mediaway_pipeline_ffi_packet_free(ref native);
        return packet;
    }

    /// <summary>Signal end-of-input; drain the remaining packets with <see cref="PollPacket"/> afterwards.</summary>
    public void Flush() =>
        MediawayPipelineException.ThrowIfError(NativeMethods.mediaway_audio_encode_session_flush(_handle));

    /// <summary>Releases the native session. Always safe to call — this surface has no handle-consumption trap.</summary>
    public void Dispose() => _handle.Dispose();
}
