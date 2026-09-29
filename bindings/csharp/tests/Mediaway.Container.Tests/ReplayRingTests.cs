using System.Runtime.CompilerServices;
using Mediaway.Common;
using Mediaway.Container.Interop;
using Xunit;

namespace Mediaway.Container.Tests;

/// <summary>
/// The replay ring and MP4 placements binding (<c>adr/container/0009-replay-ring-c-abi.md</c>)
/// against the real native library: hermetic synthetic packets whose payload names the frame they
/// belong to. No encoder, no files.
/// </summary>
public sealed class ReplayRingTests
{
    private const uint Video = 1;
    private const uint Audio = 2;
    private static readonly Rational Fps = new(1, 30);
    private static readonly Rational AudioRate = new(1, 48_000);

    /// <summary>The byte a frame's payload is made of: the frame number, modulo 256.</summary>
    private static byte FrameByte(long n) => (byte)(((n % 256) + 256) % 256);

    /// <summary>Frame <paramref name="n"/> of a 30 fps stream, keyframe every 30 frames.</summary>
    private static Packet VideoPacket(long n) => new()
    {
        StreamId = Video,
        Pts = n,
        Dts = n,
        Duration = 1,
        IsKeyframe = n % 30 == 0,
        IsDiscard = false,
        Payload = new[] { FrameByte(n), FrameByte(n), FrameByte(n), FrameByte(n) },
    };

    /// <summary>The audio packet that goes with video frame <paramref name="n"/> (1600 samples at 48 kHz).</summary>
    private static Packet AudioPacket(long n) => new()
    {
        StreamId = Audio,
        Pts = n * 1600,
        Dts = n * 1600,
        Duration = 1600,
        IsKeyframe = true,
        IsDiscard = false,
        Payload = new byte[] { 0xA0, 0xA1, 0xA2 },
    };

    // ── layout and ABI version ──────────────────────────────────────────────────────────────

    private static long Offset<TStruct, TField>(ref TStruct s, ref TField f) =>
        (long)Unsafe.ByteOffset(ref Unsafe.As<TStruct, byte>(ref s), ref Unsafe.As<TField, byte>(ref f));

    /// <summary>
    /// The native structs are hand-mirrored. Pinned to what a gcc probe compiled against the real
    /// <c>container.h</c> reports (64-bit), so a reordered or resized field fails here instead of
    /// corrupting a call.
    /// </summary>
    [Fact]
    public void NativeStructs_MatchTheHeaderLayout()
    {
        Assert.Equal(48, Unsafe.SizeOf<NativeReplayRingConfig>());
        var config = new NativeReplayRingConfig();
        Assert.Equal(0, Offset(ref config, ref config.AnchorStreamId));
        Assert.Equal(8, Offset(ref config, ref config.AnchorTimeBase));
        Assert.Equal(24, Offset(ref config, ref config.WindowMs));
        Assert.Equal(32, Offset(ref config, ref config.MaxBytes));
        Assert.Equal(40, Offset(ref config, ref config.PayloadKind));

        Assert.Equal(40, Unsafe.SizeOf<NativePacketMeta>());
        var meta = new NativePacketMeta();
        Assert.Equal(0, Offset(ref meta, ref meta.StreamId));
        Assert.Equal(8, Offset(ref meta, ref meta.Pts));
        Assert.Equal(16, Offset(ref meta, ref meta.Dts));
        Assert.Equal(24, Offset(ref meta, ref meta.Duration));
        Assert.Equal(32, Offset(ref meta, ref meta.IsKeyframe));
        Assert.Equal(33, Offset(ref meta, ref meta.IsDiscard));

        Assert.Equal(24, Unsafe.SizeOf<NativeStoredPayload>());
        var stored = new NativeStoredPayload();
        Assert.Equal(0, Offset(ref stored, ref stored.File));
        Assert.Equal(8, Offset(ref stored, ref stored.Offset));
        Assert.Equal(16, Offset(ref stored, ref stored.Len));

        Assert.Equal(80, Unsafe.SizeOf<NativeReplayClipEntry>());
        var entry = new NativeReplayClipEntry();
        Assert.Equal(0, Offset(ref entry, ref entry.StreamId));
        Assert.Equal(8, Offset(ref entry, ref entry.Pts));
        Assert.Equal(16, Offset(ref entry, ref entry.Dts));
        Assert.Equal(24, Offset(ref entry, ref entry.Duration));
        Assert.Equal(32, Offset(ref entry, ref entry.IsKeyframe));
        Assert.Equal(33, Offset(ref entry, ref entry.IsDiscard));
        Assert.Equal(36, Offset(ref entry, ref entry.PayloadKind));
        Assert.Equal(40, Offset(ref entry, ref entry.Payload));
        Assert.Equal(48, Offset(ref entry, ref entry.PayloadLen));
        Assert.Equal(56, Offset(ref entry, ref entry.StoredFile));
        Assert.Equal(64, Offset(ref entry, ref entry.StoredOffset));
        Assert.Equal(72, Offset(ref entry, ref entry.StoredLen));

        Assert.Equal(32, Unsafe.SizeOf<NativePlacement>());
        var placement = new NativePlacement();
        Assert.Equal(0, Offset(ref placement, ref placement.TrackId));
        Assert.Equal(8, Offset(ref placement, ref placement.Dts));
        Assert.Equal(16, Offset(ref placement, ref placement.Offset));
        Assert.Equal(24, Offset(ref placement, ref placement.Len));
    }

    [Fact]
    public void ContainerAbiVersion_IsEight()
    {
        Assert.Equal(8u, NativeMethods.mediaway_container_ffi_abi_version());
    }

    // ── the ring ────────────────────────────────────────────────────────────────────────────

    [Fact]
    public void Clip_StartsAtAKeyframeRebasedToZeroAndCarriesEveryStream()
    {
        using var ring = ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(2));
        ring.AddStream(Audio, AudioRate);

        // 6 s of video with the matching audio.
        for (long n = 0; n < 180; n++)
        {
            ring.Push(VideoPacket(n));
            ring.Push(AudioPacket(n));
        }

        Assert.True(ring.Span >= TimeSpan.FromSeconds(2), $"ring holds {ring.Span}");

        using var clip = ring.ClipLast(TimeSpan.FromSeconds(2));
        Assert.NotNull(clip);
        Assert.Equal(ReplayPayloadKind.Bytes, clip.Kind);
        Assert.True(clip.Count > 0);
        Assert.True(clip.Duration >= TimeSpan.FromSeconds(2), $"clip is {clip.Duration}");

        var entries = clip.ToList();
        var video = entries.Where(e => e.StreamId == Video).ToList();
        var audio = entries.Where(e => e.StreamId == Audio).ToList();

        // The anchor's first packet is a keyframe that decodes at zero.
        Assert.True(video[0].IsKeyframe);
        Assert.Equal(0, video[0].Pts);
        Assert.Equal(0, video[0].Dts);
        Assert.NotEmpty(audio);

        // Decode order across streams, compared exactly: in units of 1/48000 s a video tick (1/30 s)
        // is 1600 audio samples.
        long TicksOf(ReplayClipEntry e) => e.StreamId == Video ? e.Dts * 1600 : e.Dts;
        for (var i = 1; i < entries.Count; i++)
        {
            Assert.True(TicksOf(entries[i]) >= TicksOf(entries[i - 1]), $"entry {i} decodes before {i - 1}");
        }

        // The payload is the frame it claims to be; the cut is a GOP boundary.
        long cutFrame = 180 - video.Count;
        Assert.Equal(0, cutFrame % 30);
        foreach (var e in video)
        {
            Assert.Equal(ReplayPayloadKind.Bytes, e.Kind);
            Assert.Equal(new[] { FrameByte(cutFrame + e.Dts), FrameByte(cutFrame + e.Dts), FrameByte(cutFrame + e.Dts), FrameByte(cutFrame + e.Dts) }, e.Payload.ToArray());
        }
    }

    [Fact]
    public void Clip_IsAnOwnedSnapshot_ThatOutlivesPushesAndTheRing()
    {
        var ring = ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(1));
        for (long n = 0; n < 90; n++)
        {
            ring.Push(VideoPacket(n));
        }

        using var clip = ring.ClipLast(TimeSpan.FromSeconds(1));
        Assert.NotNull(clip);
        var before = clip.Select(e => (e.Dts, e.Payload.ToArray())).ToList();

        // Evict everything the clip was cut from, then dispose the ring.
        for (long n = 90; n < 390; n++)
        {
            ring.Push(VideoPacket(n));
        }

        ring.Dispose();

        var after = clip.Select(e => (e.Dts, e.Payload.ToArray())).ToList();
        Assert.Equal(before.Count, after.Count);
        for (var i = 0; i < before.Count; i++)
        {
            Assert.Equal(before[i].Dts, after[i].Dts);
            Assert.Equal(before[i].Item2, after[i].Item2);
        }
    }

    [Fact]
    public void ClipLast_IsNullUntilTheAnchorHasAKeyframe()
    {
        using var ring = ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(1));

        // Frame 5 is not a keyframe, and nothing can be decoded from before one: dropped, no error.
        ring.Push(VideoPacket(5));

        Assert.Null(ring.ClipLast(TimeSpan.FromSeconds(1)));
        Assert.Equal(TimeSpan.Zero, ring.Span);
    }

    [Fact]
    public void Payload_ReadAfterTheClipIsDisposed_ThrowsInsteadOfDangling()
    {
        using var ring = ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(1));
        ring.Push(VideoPacket(0));
        var clip = ring.ClipLast(TimeSpan.FromSeconds(1))!;
        var entry = clip[0];
        Assert.Equal(4, entry.Payload.Span.Length);

        clip.Dispose();

        Assert.Throws<ObjectDisposedException>(() => entry.Payload.Span.ToArray());
        Assert.Throws<ObjectDisposedException>(() => clip[0]);
        Assert.Throws<ArgumentOutOfRangeException>(() => clip[clip.Count]);
    }

    [Fact]
    public void ToPacket_FeedsAMuxer_AndWritesTheClipAsAStandaloneFile()
    {
        using var ring = ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(1));
        for (long n = 0; n < 120; n++)
        {
            ring.Push(VideoPacket(n));
        }

        using var clip = ring.ClipLast(TimeSpan.FromSeconds(1))!;

        using var muxer = new Muxer();
        muxer.AddTrack(new VideoTrackInfo { Id = Video, Codec = CodecKind.H264, TimeBase = Fps, Width = 64, Height = 64 });
        using var session = muxer.Begin();
        foreach (var entry in clip)
        {
            session.PushPacket(entry.ToPacket());
        }

        session.Flush();
        using var owner = session.PollBytes();
        var file = owner.Memory.ToArray();
        Assert.True(file.Length > 8);
        Assert.Equal("ftyp", System.Text.Encoding.ASCII.GetString(file, 4, 4));

        using var demuxer = new Demuxer();
        demuxer.PushBytes(file);
        var count = 0;
        Packet? first = null;
        while (demuxer.PollPacket() is { } packet)
        {
            first ??= packet;
            count++;
        }

        Assert.Equal(clip.Count, count);
        Assert.True(first!.IsKeyframe);
        Assert.Equal(0, first.Dts);
    }

    [Fact]
    public void StoredRing_HandsBackLocationsNotBytes()
    {
        using var ring = ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(2), kind: ReplayPayloadKind.Stored);
        Assert.Equal(ReplayPayloadKind.Stored, ring.Kind);
        for (long n = 0; n < 100; n++)
        {
            ring.PushStored(
                new PacketMeta(Video, n, n, 1, n % 30 == 0, false),
                new StoredPayload(File: 7, Offset: (ulong)n * 100, Length: 40));
        }

        using var clip = ring.ClipLast(TimeSpan.FromSeconds(1))!;
        Assert.Equal(ReplayPayloadKind.Stored, clip.Kind);
        long cut = 99 - clip[clip.Count - 1].Dts;
        foreach (var e in clip)
        {
            Assert.Equal(ReplayPayloadKind.Stored, e.Kind);
            Assert.True(e.Payload.IsEmpty);
            Assert.Equal(new StoredPayload(7, (ulong)(cut + e.Dts) * 100, 40), e.Stored);
        }

        Assert.Throws<InvalidOperationException>(() => clip[0].ToPacket());
    }

    [Fact]
    public void Push_ReportsWhyThePacketWasRefused()
    {
        using var ring = ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(1));

        // dts went backwards: refused, and the caller can carry on. (Anchor packets before the first
        // keyframe are dropped before any order check, so start on frame 0.)
        Assert.Equal(MediawayContainerStatus.Ok, ring.TryPush(VideoPacket(0)));
        Assert.Equal(MediawayContainerStatus.Ok, ring.TryPush(VideoPacket(10)));
        Assert.Equal(MediawayContainerStatus.InvalidPacket, ring.TryPush(VideoPacket(4)));
        var outOfOrder = Assert.Throws<MediawayContainerException>(() => ring.Push(VideoPacket(3)));
        Assert.Equal(MediawayContainerStatus.InvalidPacket, outOfOrder.Status);
        Assert.Equal(MediawayContainerStatus.Ok, ring.TryPush(VideoPacket(11)));

        // A stream nobody added.
        var unknown = Assert.Throws<MediawayContainerException>(() => ring.Push(AudioPacket(0)));
        Assert.Equal(MediawayContainerStatus.UnknownStream, unknown.Status);

        // The anchor is already there.
        var duplicate = Assert.Throws<MediawayContainerException>(() => ring.AddStream(Video, Fps));
        Assert.Equal(MediawayContainerStatus.InvalidTrack, duplicate.Status);

        // A degenerate timebase, on a new stream and on Create.
        var zero = new Rational(0, 1);
        Assert.Equal(
            MediawayContainerStatus.InvalidArgument,
            Assert.Throws<MediawayContainerException>(() => ring.AddStream(Audio, zero)).Status);
        Assert.Equal(
            MediawayContainerStatus.InvalidArgument,
            Assert.Throws<MediawayContainerException>(() => ReplayRing.Create(Video, zero, TimeSpan.FromSeconds(1))).Status);
    }

    [Fact]
    public void PushingTheWrongPayloadKind_IsInvalidState()
    {
        using var bytesRing = ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(1));
        using var storedRing = ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(1), kind: ReplayPayloadKind.Stored);
        var meta = new PacketMeta(Video, 0, 0, 1, true, false);

        Assert.Equal(
            MediawayContainerStatus.InvalidState,
            bytesRing.TryPushStored(meta, new StoredPayload(0, 0, 1)));
        Assert.Equal(MediawayContainerStatus.InvalidState, storedRing.TryPush(VideoPacket(0)));
        Assert.Equal(MediawayContainerStatus.InvalidState, storedRing.TryPush(in meta, new byte[] { 1 }));
        Assert.Equal(MediawayContainerStatus.Ok, bytesRing.TryPush(in meta, new byte[] { 1, 2, 3 }));
    }

    [Fact]
    public void Create_RejectsNegativeArguments()
    {
        Assert.Throws<ArgumentOutOfRangeException>(
            () => ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(-1)));
        Assert.Throws<ArgumentOutOfRangeException>(
            () => ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(1), maxBytes: -1));
    }

    [Fact]
    public void ByteCeiling_EvictsOldestGopFirst()
    {
        using var ring = ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(60), maxBytes: 30 * 4 * 2);
        for (long n = 0; n < 300; n++)
        {
            ring.Push(VideoPacket(n));
        }

        using var clip = ring.ClipLast(TimeSpan.FromSeconds(60))!;
        Assert.True(clip.Count <= 3 * 30, $"a two-GOP ceiling must bound the ring well below 300 pushed, holds {clip.Count}");
    }

    // ── placements ──────────────────────────────────────────────────────────────────────────

    /// <summary>Mux <paramref name="count"/> packets and return every output byte plus the placements.</summary>
    private static (byte[] File, IReadOnlyList<Placement>? Placements, List<byte[]> Payloads) MuxAll(
        Muxer muxer, uint trackId, int count, bool takePlacements)
    {
        muxer.AddTrack(new VideoTrackInfo { Id = trackId, Codec = CodecKind.H264, TimeBase = Fps, Width = 64, Height = 64 });
        using var session = muxer.Begin();
        var payloads = new List<byte[]>();
        for (var i = 0; i < count; i++)
        {
            // Length-prefixed (AVCC) NAL: written unchanged, so `len` equals the pushed length.
            var payload = new byte[] { 0, 0, 0, 4, 0x65, (byte)i, (byte)(i >> 8), 0x80 };
            payloads.Add(payload);
            session.PushPacket(new Packet
            {
                StreamId = trackId,
                Pts = i,
                Dts = i,
                Duration = 1,
                IsKeyframe = i % 30 == 0,
                IsDiscard = false,
                Payload = payload,
            });
        }

        session.Flush();
        using var owner = session.PollBytes();
        return (owner.Memory.ToArray(), takePlacements ? session.PollPlacements() : null, payloads);
    }

    [Fact]
    public void Placements_PointAtTheRealBytes_AndChangeNothing()
    {
        using var plain = new Muxer();
        var (plainFile, _, _) = MuxAll(plain, Video, 90, takePlacements: false);

        using var recording = Muxer.CreateWithPlacements();
        var (file, placements, payloads) = MuxAll(recording, Video, 90, takePlacements: true);

        Assert.Equal(plainFile, file);
        Assert.NotNull(placements);
        Assert.Equal(payloads.Count, placements.Count);
        for (var i = 0; i < payloads.Count; i++)
        {
            Assert.Equal(Video, placements[i].TrackId);
            Assert.Equal(i, placements[i].Dts);
            var slice = file.AsSpan((int)placements[i].Offset, (int)placements[i].Length).ToArray();
            Assert.Equal(payloads[i], slice);
        }
    }

    [Fact]
    public void PollPlacements_OnAWebMMuxer_IsInvalidState_AndOnAPlainMp4Muxer_IsEmpty()
    {
        // A plain MP4 muxer, one never created with placements, records nothing and returns an
        // empty list: the same answer the Rust API gives (ADR-0009 §4).
        using var plain = new Muxer();
        plain.AddTrack(new VideoTrackInfo { Id = Video, Codec = CodecKind.H264, TimeBase = Fps, Width = 64, Height = 64 });
        using var plainSession = plain.Begin();
        Assert.Empty(plainSession.PollPlacements());

        // WebM never records placements, even though it is a real muxer.
        using var webm = new Muxer(ContainerFormat.WebM);
        webm.AddTrack(new VideoTrackInfo { Id = Video, Codec = CodecKind.Vp9, TimeBase = Fps, Width = 64, Height = 64 });
        using var webmSession = webm.Begin();
        Assert.Equal(
            MediawayContainerStatus.InvalidState,
            Assert.Throws<MediawayContainerException>(() => webmSession.PollPlacements()).Status);
    }

    [Fact]
    public void PollPlacements_IsEmptyWhenNothingWasWritten()
    {
        using var recording = Muxer.CreateWithPlacements();
        recording.AddTrack(new VideoTrackInfo { Id = Video, Codec = CodecKind.H264, TimeBase = Fps, Width = 64, Height = 64 });
        using var session = recording.Begin();
        Assert.Empty(session.PollPlacements());
    }

    // ── a Stored ring fed by placements ─────────────────────────────────────────────────────

    [Fact]
    public void StoredRing_FedFromPlacements_ReadsBackTheSameBytesAsABytesRing()
    {
        using var recording = Muxer.CreateWithPlacements();
        var (file, placements, payloads) = MuxAll(recording, Video, 120, takePlacements: true);

        using var bytesRing = ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(1.5));
        using var storedRing = ReplayRing.Create(Video, Fps, TimeSpan.FromSeconds(1.5), kind: ReplayPayloadKind.Stored);
        for (var i = 0; i < payloads.Count; i++)
        {
            var meta = new PacketMeta(Video, i, i, 1, i % 30 == 0, false);
            bytesRing.Push(meta, payloads[i]);
            storedRing.PushStored(meta, new StoredPayload(1, placements![i].Offset, placements[i].Length));
        }

        using var fromBytes = bytesRing.ClipLast(TimeSpan.FromSeconds(1))!;
        using var fromStored = storedRing.ClipLast(TimeSpan.FromSeconds(1))!;
        Assert.Equal(fromBytes.Count, fromStored.Count);
        for (var i = 0; i < fromBytes.Count; i++)
        {
            var b = fromBytes[i];
            var s = fromStored[i];
            Assert.Equal((b.Pts, b.Dts, b.Duration, b.IsKeyframe), (s.Pts, s.Dts, s.Duration, s.IsKeyframe));
            var read = file.AsSpan((int)s.Stored!.Value.Offset, (int)s.Stored.Value.Length).ToArray();
            Assert.Equal(b.Payload.ToArray(), read);
        }
    }
}
