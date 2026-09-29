// ReplayRing.cs — Mediaway C# replay ring quick start (adr/container/0009-replay-ring-c-abi.md).
//
// Keep the last few seconds of encoded packets in memory, then cut "the last 2 seconds" at a
// keyframe and write it out as a standalone MP4 — the "save a replay" button of a recorder.
//
// LIMIT: the C ABI has no video-packet source. EncodeSession muxes its encoder's packets
// internally, and packet-level output exists only for the audio encoder. Feed the ring packets from
// a Demuxer, from an audio encoder, or from an encoder you drive yourself. This example synthesizes
// packets so it runs anywhere with no encoder.
//
// Scenario: push 6 s of 30 fps "video" (a keyframe every 30 frames) into a 3 s ring, cut the last
// 2 s, mux it with a fresh Muxer, and demux the result to prove it is a standalone file.

using System;
using Mediaway.Common;
using Mediaway.Container;

var frameRate = new Rational(1, 30);
const uint videoId = 1;
const int frames = 180; // 6 s at 30 fps

// ── 1. The ring: anchored on the video stream, keeping 3 s of history ───────────────────────────
using var ring = ReplayRing.Create(videoId, frameRate, TimeSpan.FromSeconds(3));

for (long i = 0; i < frames; i++)
{
    // The payload here is fake; a real caller pushes the encoder's packet bytes. Push COPIES them
    // into the ring, and every clip then shares that copy by reference count.
    var status = ring.TryPush(new Packet
    {
        StreamId = videoId,
        Pts = i,
        Dts = i,
        Duration = 1,
        IsKeyframe = i % 30 == 0,
        IsDiscard = false,
        Payload = new byte[] { 0, 0, 0, 4, 0x65, (byte)i, 0, 0x80 }, // one length-prefixed NAL
    });

    // A packet whose dts went backwards is refused with InvalidPacket; drop it and carry on.
    if (status != MediawayContainerStatus.Ok)
    {
        Console.WriteLine($"frame {i} refused: {status}");
    }
}

Console.WriteLine($"ring holds {ring.Span.TotalSeconds:F1} s");

// ── 2. Cut a clip: an owned snapshot that starts at a keyframe, rebased to zero ─────────────────
using ReplayClip? clip = ring.ClipLast(TimeSpan.FromSeconds(2));
if (clip is null)
{
    Console.WriteLine("no keyframe yet, nothing to cut");
    return;
}

Console.WriteLine($"clip: {clip.Count} packets, {clip.Duration.TotalSeconds:F1} s, " +
                  $"starts at dts {clip[0].Dts} (keyframe: {clip[0].IsKeyframe})");

// ── 3. Write it out through the ordinary muxer: add the track, push each entry ─────────────────
using var muxer = new Muxer();
muxer.AddTrack(new VideoTrackInfo
{
    Id = videoId,
    Codec = CodecKind.H264,
    TimeBase = frameRate,
    Width = 1280,
    Height = 720,
});
using MuxerSession session = muxer.Begin();

foreach (ReplayClipEntry entry in clip)
{
    session.PushPacket(entry.ToPacket()); // the payload is borrowed from the clip: push before disposing it
}

session.Flush();
using var bytes = session.PollBytes();
Console.WriteLine($"standalone MP4: {bytes.Memory.Length} bytes");

// ── 4. Prove it: demux the clip file back ──────────────────────────────────────────────────────
using var demuxer = new Demuxer();
demuxer.PushBytes(bytes.Memory.ToArray());
var recovered = 0;
while (demuxer.PollPacket() is { } packet)
{
    using (packet)
    {
        recovered++;
    }
}

Console.WriteLine($"demuxed {recovered} packets back (clip had {clip.Count})");
