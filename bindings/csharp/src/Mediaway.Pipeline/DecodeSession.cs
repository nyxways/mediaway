using Mediaway.Common;
using Mediaway.Pipeline.Interop;

namespace Mediaway.Pipeline;

/// <summary>
/// The best available video decode session for a <see cref="VideoDecodeConfig"/> on the
/// current platform — the handle IS the decoder (single-step open, no consumption trap,
/// mirrors <see cref="AutoVideoEncoder"/>'s <c>NoBackend</c> handling). CPU output only
/// (GPU decode output is deferred, <c>adr/0004-auto-decode-c-abi.md</c> §1/§5).
/// </summary>
public sealed class DecodeSession : IDisposable
{
    private readonly DecodeSessionHandle _handle;

    private DecodeSession(DecodeSessionHandle handle) => _handle = handle;

    /// <summary>
    /// Open the best available video decoder for <paramref name="config"/> on this platform.
    /// </summary>
    /// <exception cref="DecoderUnavailableException">
    /// No supported decoder backend is compiled in here — an expected, graceful outcome to
    /// catch and handle, not a bug.
    /// </exception>
    public static unsafe DecodeSession Open(VideoDecodeConfig config)
    {
        using var extraDataPin = config.ExtraData.Pin();
        var native = new NativeAutoVideoDecodeConfig
        {
            Codec = (CodecKind)(int)config.Codec, // VideoCodec's leading four values match CodecKind 1:1
            Width = config.Width,
            Height = config.Height,
            TimeBase = new NativeRational(config.TimeBase),
            PixelFormat = config.PixelFormat,
            ExtraData = config.ExtraData.IsEmpty ? null : (byte*)extraDataPin.Pointer,
            ExtraDataLen = (nuint)config.ExtraData.Length,
        };

        var status = NativeMethods.mediaway_decode_session_open(in native, out nint session);
        MediawayPipelineException.ThrowIfDecodeError(status);
        return new DecodeSession(DecodeSessionHandle.Wrap(session));
    }

    /// <summary>
    /// Push one compressed packet. <see cref="DecodePacket.Payload"/> is borrowed for the
    /// call only. May produce zero or more frames (drain via <see cref="PollFrame"/>).
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
        MediawayPipelineException.ThrowIfError(NativeMethods.mediaway_decode_session_push_packet(_handle, in native));
    }

    /// <summary>
    /// Pull the next decoded frame, if any is ready. <see langword="null"/> is a valid
    /// "nothing ready yet" result, not an error.
    /// </summary>
    public unsafe DecodedVideoFrame? PollFrame()
    {
        MediawayPipelineException.ThrowIfError(
            NativeMethods.mediaway_decode_session_poll_frame(_handle, out NativeDecodedVideoFrame native, out byte hasFrame));

        if (hasFrame == 0)
        {
            return null;
        }

        var frame = new DecodedVideoFrame
        {
            Pts = native.Pts,
            Duration = native.Duration,
            Width = native.Width,
            Height = native.Height,
            PixelFormat = native.PixelFormat,
            Data = native.Data is null
                ? Array.Empty<byte>()
                : new ReadOnlySpan<byte>(native.Data, (int)native.DataLen).ToArray(),
        };
        NativeMethods.mediaway_decoded_video_frame_free(ref native);
        return frame;
    }

    /// <summary>Signal end-of-input; drain the remaining frames with <see cref="PollFrame"/> afterward.</summary>
    public void Flush() =>
        MediawayPipelineException.ThrowIfError(NativeMethods.mediaway_decode_session_flush(_handle));

    /// <summary>Releases the native session. Always safe to call — this surface has no handle-consumption trap.</summary>
    public void Dispose() => _handle.Dispose();
}
