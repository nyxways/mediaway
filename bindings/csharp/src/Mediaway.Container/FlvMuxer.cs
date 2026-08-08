using System.Buffers;
using Mediaway.Common.Interop;
using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>
/// Muxes packets into FLV tags. Unlike <see cref="MuxerSession"/>, every write method here
/// returns its own freshly allocated output buffer directly — there is no separate
/// <c>PollBytes</c> step. FLV has exactly one video and one audio slot (no track-id field in
/// the format itself); <see cref="AddVideoTrack"/>/<see cref="AddAudioTrack"/> ignore the
/// info's own <c>Id</c> and return the fixed ids <see cref="VideoTrackId"/>/
/// <see cref="AudioTrackId"/> instead.
/// </summary>
public sealed class FlvMuxer : IDisposable
{
    /// <summary>Fixed stream id for the video slot — use as <see cref="Packet.StreamId"/>.</summary>
    public const uint VideoTrackId = 0;

    /// <summary>Fixed stream id for the audio slot — use as <see cref="Packet.StreamId"/>.</summary>
    public const uint AudioTrackId = 1;

    private readonly FlvMuxerHandle _handle;

    public FlvMuxer() => _handle = FlvMuxerHandle.Create();

    /// <summary>Write the FLV file header. Call before any track registration or packet.</summary>
    public IMemoryOwner<byte> WriteHeader(bool hasAudio, bool hasVideo)
    {
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_flv_muxer_write_header(
            _handle, (byte)(hasAudio ? 1 : 0), (byte)(hasVideo ? 1 : 0), out var data, out var len));
        return Wrap(data, len);
    }

    /// <summary>
    /// Register the video track. Only H264 is recognized
    /// (<see cref="MediawayContainerStatus.UnsupportedCodec"/> otherwise).
    /// </summary>
    public void AddVideoTrack(VideoTrackInfo info)
    {
        using var pin = info.ExtraData.Pin();
        var native = ToNative(info, pin);
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_flv_muxer_add_video_track(_handle, in native));
    }

    /// <summary>
    /// Register the audio track. AAC and MP3 are recognized
    /// (<see cref="MediawayContainerStatus.UnsupportedCodec"/> otherwise).
    /// </summary>
    public unsafe void AddAudioTrack(AudioTrackInfo info)
    {
        using var pin = info.ExtraData.Pin();
        var native = new NativeAudioTrackInfo
        {
            Id = info.Id,
            Codec = info.Codec,
            TimeBase = new NativeRational(info.TimeBase),
            SampleRate = info.SampleRate,
            Channels = info.Channels,
            ExtraData = info.ExtraData.IsEmpty ? null : (byte*)pin.Pointer,
            ExtraDataLen = (nuint)info.ExtraData.Length,
        };
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_flv_muxer_add_audio_track(_handle, in native));
    }

    /// <summary>
    /// Mux one packet: writes the track's sequence-header tag first (once, only for codecs
    /// that have one) then the data tag. <see cref="Packet.StreamId"/> selects
    /// <see cref="VideoTrackId"/> or <see cref="AudioTrackId"/> and must have a matching
    /// <see cref="AddVideoTrack"/>/<see cref="AddAudioTrack"/> call already made, else
    /// <see cref="MediawayContainerStatus.UnknownStream"/>.
    /// </summary>
    public unsafe IMemoryOwner<byte> PushPacket(Packet packet)
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
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_flv_muxer_push_packet(_handle, in native, out var data, out var len));
        return Wrap(data, len);
    }

    public void Dispose() => _handle.Dispose();

    private static unsafe NativeVideoTrackInfo ToNative(VideoTrackInfo info, MemoryHandle pin) => new()
    {
        Id = info.Id,
        Codec = info.Codec,
        TimeBase = new NativeRational(info.TimeBase),
        Width = info.Width,
        Height = info.Height,
        ExtraData = info.ExtraData.IsEmpty ? null : (byte*)pin.Pointer,
        ExtraDataLen = (nuint)info.ExtraData.Length,
    };

    private static IMemoryOwner<byte> Wrap(nint data, nuint len) =>
        data == 0 || len == 0
            ? EmptyMemoryOwner<byte>.Instance
            : new NativeOwnedMemoryManager(data, len, static (ptr, l) => NativeMethods.mediaway_buffer_free(ptr, l));
}
