using Mediaway.Common;
using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>
/// A rolling buffer of the last <i>N</i> of encoded packets that cuts "the last <i>M</i>" at a
/// keyframe, in decode order, as a packet sequence rebased to zero — which a fresh
/// <see cref="Muxer"/> writes as a standalone file (<c>adr/container/0009-replay-ring-c-abi.md</c>).
/// </summary>
/// <remarks>
/// <para>
/// <b>Limit:</b> the C ABI has no video-packet source. <c>EncodeSession</c> muxes its encoder's
/// packets internally, and packet-level output exists only for the audio encoder. Feed the ring
/// packets from a <see cref="Demuxer"/>, from an audio encoder, or from an encoder you drive yourself.
/// </para>
/// <para>
/// One <i>anchor</i> stream (normally video) decides where a clip may start: at one of its
/// keyframes. Other streams (audio) are added with <see cref="AddStream"/> and cut by time to match.
/// Eviction is by whole GOPs, so a clip can begin up to one keyframe interval earlier than asked,
/// never later.
/// </para>
/// <para>
/// Not thread-safe: do not use one ring from two threads concurrently.
/// </para>
/// </remarks>
public sealed class ReplayRing : IDisposable
{
    private readonly ReplayRingHandle _handle;

    private ReplayRing(ReplayRingHandle handle, ReplayPayloadKind kind)
    {
        _handle = handle;
        Kind = kind;
    }

    /// <summary>What each packet's payload is held as.</summary>
    public ReplayPayloadKind Kind { get; }

    /// <summary>Create a ring.</summary>
    /// <param name="anchorStreamId">The stream whose keyframes decide where a clip may start.</param>
    /// <param name="anchorTimeBase">The anchor's timebase.</param>
    /// <param name="window">How much history to keep. Millisecond resolution.</param>
    /// <param name="maxBytes">
    /// Evict oldest GOP first while more than this many payload bytes are held; <c>0</c> = no ceiling.
    /// The newest GOP is always kept, so one larger GOP can exceed it.
    /// </param>
    /// <param name="kind">What each packet's payload is held as.</param>
    /// <exception cref="ArgumentOutOfRangeException"><paramref name="window"/> or
    /// <paramref name="maxBytes"/> is negative.</exception>
    /// <exception cref="MediawayContainerException">A timebase with a zero numerator or denominator
    /// (<see cref="MediawayContainerStatus.InvalidArgument"/>).</exception>
    public static ReplayRing Create(
        uint anchorStreamId,
        Rational anchorTimeBase,
        TimeSpan window,
        long maxBytes = 0,
        ReplayPayloadKind kind = ReplayPayloadKind.Bytes)
    {
        var native = new NativeReplayRingConfig
        {
            AnchorStreamId = anchorStreamId,
            AnchorTimeBase = new NativeRational(anchorTimeBase),
            WindowMs = ToMilliseconds(window, nameof(window)),
            MaxBytes = maxBytes >= 0
                ? (ulong)maxBytes
                : throw new ArgumentOutOfRangeException(nameof(maxBytes), maxBytes, "Must not be negative."),
            PayloadKind = (int)kind,
        };
        MediawayContainerException.ThrowIfError(NativeMethods.mediaway_replay_ring_create(in native, out var raw));
        return new ReplayRing(ReplayRingHandle.FromRaw(raw), kind);
    }

    /// <summary>Carry another stream (e.g. audio), cut by time to match the anchor.</summary>
    /// <exception cref="MediawayContainerException"><see cref="MediawayContainerStatus.InvalidTrack"/>
    /// for a duplicate stream, <see cref="MediawayContainerStatus.InvalidArgument"/> for a degenerate
    /// timebase.</exception>
    public void AddStream(uint streamId, Rational timeBase) =>
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_replay_ring_add_stream(_handle, streamId, new NativeRational(timeBase)));

    /// <summary>
    /// <see cref="ReplayPayloadKind.Bytes"/> ring: add a packet, then evict what fell out of the
    /// window. <b>The payload is copied</b> into the ring (it is borrowed for the call only); every
    /// clip then shares that copy by reference count. Anchor packets before the anchor's first
    /// keyframe are dropped without error.
    /// </summary>
    /// <exception cref="MediawayContainerException">
    /// <see cref="MediawayContainerStatus.UnknownStream"/> for a stream never added;
    /// <see cref="MediawayContainerStatus.InvalidPacket"/> when this stream's <c>Dts</c> went
    /// backwards (the packet is not added; carry on — see <see cref="TryPush(Packet)"/> to avoid the
    /// exception); <see cref="MediawayContainerStatus.InvalidState"/> on a stored ring.
    /// </exception>
    public void Push(Packet packet) => MediawayContainerException.ThrowIfError(TryPush(packet));

    /// <summary>
    /// <see cref="Push(Packet)"/> without the exception: returns the status, so a dropped
    /// out-of-order packet costs no stack trace on the hot path.
    /// </summary>
    public unsafe MediawayContainerStatus TryPush(Packet packet)
    {
        if (packet is null)
        {
            throw new ArgumentNullException(nameof(packet));
        }

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
        return NativeMethods.mediaway_replay_ring_push(_handle, in native);
    }

    /// <summary><see cref="Push(Packet)"/> for a packet that is not a <see cref="Packet"/> object:
    /// metadata plus a payload span, copied in.</summary>
    public void Push(in PacketMeta meta, ReadOnlySpan<byte> payload) =>
        MediawayContainerException.ThrowIfError(TryPush(in meta, payload));

    /// <summary><see cref="Push(in PacketMeta, ReadOnlySpan{byte})"/> without the exception.</summary>
    public unsafe MediawayContainerStatus TryPush(in PacketMeta meta, ReadOnlySpan<byte> payload)
    {
        fixed (byte* p = payload)
        {
            var native = new NativePacketView
            {
                StreamId = meta.StreamId,
                Pts = meta.Pts,
                Dts = meta.Dts,
                Duration = meta.Duration,
                IsKeyframe = (byte)(meta.IsKeyframe ? 1 : 0),
                IsDiscard = (byte)(meta.IsDiscard ? 1 : 0),
                Payload = payload.IsEmpty ? null : p,
                PayloadLen = (nuint)payload.Length,
            };
            return NativeMethods.mediaway_replay_ring_push(_handle, in native);
        }
    }

    /// <summary>
    /// <see cref="ReplayPayloadKind.Stored"/> ring: add a packet's metadata and where its bytes are.
    /// The ring never reads the file and holds a few dozen bytes per packet. Take the locations from
    /// <see cref="MuxerSession.PollPlacements"/> when every polled muxer byte is written to a file.
    /// Same errors as <see cref="Push(Packet)"/>, with
    /// <see cref="MediawayContainerStatus.InvalidState"/> on a bytes ring.
    /// </summary>
    public void PushStored(in PacketMeta meta, in StoredPayload stored) =>
        MediawayContainerException.ThrowIfError(TryPushStored(in meta, in stored));

    /// <summary><see cref="PushStored"/> without the exception.</summary>
    public MediawayContainerStatus TryPushStored(in PacketMeta meta, in StoredPayload stored)
    {
        var nativeMeta = new NativePacketMeta
        {
            StreamId = meta.StreamId,
            Pts = meta.Pts,
            Dts = meta.Dts,
            Duration = meta.Duration,
            IsKeyframe = (byte)(meta.IsKeyframe ? 1 : 0),
            IsDiscard = (byte)(meta.IsDiscard ? 1 : 0),
        };
        var nativeStored = new NativeStoredPayload
        {
            File = stored.File,
            Offset = stored.Offset,
            Len = stored.Length,
        };
        return NativeMethods.mediaway_replay_ring_push_stored(_handle, in nativeMeta, in nativeStored);
    }

    /// <summary>
    /// The longest clip <see cref="ClipLast"/> can return right now: how far back the oldest cut point
    /// is from the newest packet. Zero before the anchor's first keyframe.
    /// </summary>
    public TimeSpan Span
    {
        get
        {
            MediawayContainerException.ThrowIfError(
                NativeMethods.mediaway_replay_ring_span_ms(_handle, out var ms));
            return TimeSpan.FromMilliseconds(ms);
        }
    }

    /// <summary>
    /// Cut the last <paramref name="span"/> of every stream at a keyframe: the clip starts at the
    /// latest anchor keyframe at or before <c>newest - span</c> — up to one keyframe interval
    /// earlier than asked, never later; with less than <paramref name="span"/> held it starts at the
    /// oldest keyframe. <c>null</c> until the anchor's first keyframe has been pushed.
    /// </summary>
    /// <returns>An owned snapshot; dispose it. It stays valid after further pushes and after this
    /// ring is disposed.</returns>
    public ReplayClip? ClipLast(TimeSpan span)
    {
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_replay_ring_clip_last(
                _handle, ToMilliseconds(span, nameof(span)), out var raw, out var has));
        return has == 0 || raw == 0 ? null : new ReplayClip(ReplayClipHandle.FromRaw(raw), Kind);
    }

    /// <summary>Closes the ring. Clips already taken from it stay valid: they own their data.</summary>
    public void Dispose() => _handle.Dispose();

    private static ulong ToMilliseconds(TimeSpan value, string name) =>
        value < TimeSpan.Zero
            ? throw new ArgumentOutOfRangeException(name, value, "Must not be negative.")
            : (ulong)value.TotalMilliseconds;
}
