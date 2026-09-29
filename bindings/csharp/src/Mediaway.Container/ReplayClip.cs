using System.Collections;
using Mediaway.Container.Interop;

namespace Mediaway.Container;

/// <summary>
/// A cut of a <see cref="ReplayRing"/>: which packets, in decode order across streams, with
/// timestamps rebased so the cut keyframe decodes at zero.
/// </summary>
/// <remarks>
/// <para>
/// <b>An owned snapshot.</b> Unlike the Rust <c>Clip</c>, which borrows the ring and must be
/// written out before the next push, this holds its own copy of the metadata and shares payload
/// <i>bytes</i> by reference count — so the ring may keep taking packets, evicting what the clip was
/// cut from, or be disposed, while the clip is read.
/// </para>
/// <para>
/// For a <see cref="ReplayPayloadKind.Bytes"/> ring each entry's
/// <see cref="ReplayClipEntry.Payload"/> is a Zero-Copy view valid until this clip is disposed.
/// Write a clip out through a muxer with
/// <c>foreach (var e in clip) session.PushPacket(e.ToPacket());</c>.
/// </para>
/// </remarks>
public sealed class ReplayClip : IReadOnlyList<ReplayClipEntry>, IDisposable
{
    private readonly ReplayClipHandle _handle;

    internal ReplayClip(ReplayClipHandle handle, ReplayPayloadKind kind)
    {
        _handle = handle;
        Kind = kind;
        Count = checked((int)NativeMethods.mediaway_replay_clip_packet_count(handle));
        MediawayContainerException.ThrowIfError(
            NativeMethods.mediaway_replay_clip_duration_ms(handle, out var ms));
        Duration = TimeSpan.FromMilliseconds(ms);
    }

    /// <summary>What the ring this clip was cut from holds for each packet.</summary>
    public ReplayPayloadKind Kind { get; }

    /// <summary>Number of packets in the clip.</summary>
    public int Count { get; }

    /// <summary>From the cut keyframe to the newest packet pushed when the clip was taken.</summary>
    public TimeSpan Duration { get; }

    /// <summary>Packet <paramref name="index"/>, in decode order across streams.</summary>
    /// <exception cref="ArgumentOutOfRangeException"><paramref name="index"/> is out of range.</exception>
    public ReplayClipEntry this[int index]
    {
        get
        {
            if ((uint)index >= (uint)Count)
            {
                throw new ArgumentOutOfRangeException(nameof(index), index, "Index is outside the clip.");
            }

            MediawayContainerException.ThrowIfError(
                NativeMethods.mediaway_replay_clip_packet_at(_handle, (nuint)index, out var native));
            return ToManaged(native);
        }
    }

    public IEnumerator<ReplayClipEntry> GetEnumerator()
    {
        for (var i = 0; i < Count; i++)
        {
            yield return this[i];
        }
    }

    IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();

    /// <summary>
    /// Releases the native snapshot. Every <see cref="ReplayClipEntry.Payload"/> read from it stops
    /// being valid; the ring it was cut from is unaffected.
    /// </summary>
    public void Dispose() => _handle.Dispose();

    private ReplayClipEntry ToManaged(NativeReplayClipEntry native)
    {
        var kind = (ReplayPayloadKind)native.PayloadKind;
        var payload = kind == ReplayPayloadKind.Bytes && native.Payload != 0 && native.PayloadLen != 0
            ? new ClipPayloadMemoryManager(_handle, native.Payload, native.PayloadLen).Memory
            : Memory<byte>.Empty;
        StoredPayload? stored = kind == ReplayPayloadKind.Stored
            ? new StoredPayload(native.StoredFile, native.StoredOffset, native.StoredLen)
            : null;

        return new ReplayClipEntry(
            native.StreamId,
            native.Pts,
            native.Dts,
            native.Duration,
            native.IsKeyframe != 0,
            native.IsDiscard != 0,
            kind,
            payload,
            stored);
    }
}
