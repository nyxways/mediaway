using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>
/// A muxer in the track-registration state. Register tracks with
/// <see cref="AddTrack(VideoTrackInfo)"/>/<see cref="AddTrack(AudioTrackInfo)"/>, then call
/// <see cref="Begin"/> once to close registration and obtain the live
/// <see cref="MuxerSession"/>. <see cref="Begin"/> transfers ownership of the underlying
/// native handle to the returned session — this instance becomes inert afterward, matching
/// the native muxer's own Open→Live typestate.
/// </summary>
public sealed class Muxer : IDisposable
{
    private MuxerHandle? _handle;

    public Muxer() => _handle = MuxerHandle.Create();

    /// <param name="format">
    /// Container format to open. WebM's TrackNumber element must not be <c>0</c> — unlike
    /// MP4, a video/audio track registered with <see cref="VideoTrackInfo.Id"/>/
    /// <see cref="AudioTrackInfo.Id"/> <c>== 0</c> is rejected with
    /// <see cref="MediawayContainerStatus.InvalidData"/>.
    /// </param>
    public Muxer(ContainerFormat format) => _handle = MuxerHandle.CreateForFormat(format);

    /// <param name="fragmentBatch">
    /// Samples-per-fragment batch size. <c>0</c> is accepted and clamped to 1 by the native
    /// core — no diagnostic is raised for passing 0 by mistake.
    /// </param>
    public Muxer(nuint fragmentBatch) => _handle = MuxerHandle.CreateWithFragmentBatch(fragmentBatch);

    /// <summary>Register a video track. Returns <paramref name="info"/>'s own <c>Id</c>.</summary>
    public unsafe uint AddTrack(VideoTrackInfo info)
    {
        var handle = RequireOpen();
        using var pin = info.ExtraData.Pin();
        var native = new NativeVideoTrackInfo
        {
            Id = info.Id,
            Codec = info.Codec,
            TimeBase = new NativeRational(info.TimeBase),
            Width = info.Width,
            Height = info.Height,
            ExtraData = info.ExtraData.IsEmpty ? null : (byte*)pin.Pointer,
            ExtraDataLen = (nuint)info.ExtraData.Length,
        };
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_muxer_add_video_track(handle, in native));
        return info.Id;
    }

    /// <summary>Register an audio (or subtitle) track. Returns <paramref name="info"/>'s own <c>Id</c>.</summary>
    public unsafe uint AddTrack(AudioTrackInfo info)
    {
        var handle = RequireOpen();
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
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_muxer_add_audio_track(handle, in native));
        return info.Id;
    }

    /// <summary>
    /// Close track registration and start accepting packets. This <see cref="Muxer"/>
    /// instance is inert after this call — use the returned <see cref="MuxerSession"/>
    /// instead. Disposing this instance afterward is a safe no-op.
    /// </summary>
    public MuxerSession Begin()
    {
        var handle = RequireOpen();
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_muxer_begin(handle));
        _handle = null; // Ownership transferred to the MuxerSession — this instance goes inert.
        return new MuxerSession(handle);
    }

    /// <summary>
    /// Releases the native muxer. A no-op once <see cref="Begin"/> has transferred ownership
    /// to a <see cref="MuxerSession"/> — dispose that instead.
    /// </summary>
    public void Dispose() => _handle?.Dispose();

    private MuxerHandle RequireOpen() => _handle ?? throw new ObjectDisposedException(
        nameof(Muxer),
        "This Muxer has already begun streaming (use the MuxerSession returned by Begin()) " +
        "or was disposed.");
}
