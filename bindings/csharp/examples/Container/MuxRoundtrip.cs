// MuxRoundtrip.cs — Mediaway C# quick start.
//
// The `Mediaway.Common`/`Mediaway.Container` packages this targets are real —
// see bindings/csharp/src/ and docs/adr/0017-csharp-binding-package-layout.md.
// This file mirrors examples/mux_roundtrip.rs and matches the real, shipped
// API: IDisposable wrappers over native handles, exceptions instead of raw
// error codes, PascalCase members.
//
// Revision note: PollBytes()/PollPacket()/Streams return IMemoryOwner<byte>-
// backed types (`Packet`, `StreamDescriptor` are themselves IDisposable) so
// the native buffer is released deterministically on Dispose, never via a
// finalizer — see Mediaway.Common.Interop.NativeOwnedMemoryManager's own
// comment on why a finalizer there would risk a use-after-free (CA2015)
// instead of just a late free.
//
// Scenario: mux one H.264 video track + one AAC audio track into a
// fragmented MP4 (sans-io — the muxer/demuxer never touch files or sockets;
// the caller always owns the bytes), then demux the result back and count
// the recovered packets.

using System;
using System.Buffers;
using Mediaway.Common;
using Mediaway.Container;

var frameRate = new Rational(1, 30);
var audioTimeBase = new Rational(1, 48_000);
const int frameCount = 90; // 3 s at 30 fps

// ── 1. Register tracks (Open state) ─────────────────────────────────────────
using var muxer = new Muxer();

uint videoTrackId = muxer.AddTrack(new VideoTrackInfo
{
    Id = 0,
    Codec = CodecKind.H264,
    TimeBase = frameRate,
    Width = 1920,
    Height = 1080,
    ExtraData = Array.Empty<byte>(),
});

uint audioTrackId = muxer.AddTrack(new AudioTrackInfo
{
    Id = 1,
    Codec = CodecKind.Aac,
    TimeBase = audioTimeBase,
    ExtraData = Array.Empty<byte>(),
    SampleRate = 48_000,
    Channels = 2,
});

// ── 2. Begin() closes track registration and hands back the live session ───
// (AddTrack / PushPacket / Flush throw MediawayException — translated from
// the native C ABI error codes — on failure; omitted here for the happy path.)
using MuxerSession session = muxer.Begin();

byte[] fakeNalUnit = { 0x00, 0x00, 0x00, 0x01 };
byte[] fakeAdtsFrame = { 0xFF, 0xF1 };

for (int i = 0; i < frameCount; i++)
{
    session.PushPacket(new Packet
    {
        StreamId = videoTrackId,
        Pts = i,
        Dts = i,
        Duration = 1,
        IsKeyframe = i % 30 == 0,
        IsDiscard = false,
        Payload = fakeNalUnit,
    });

    session.PushPacket(new Packet
    {
        StreamId = audioTrackId,
        Pts = i * 1_600L,
        Dts = i * 1_600L,
        Duration = 1_600,
        IsKeyframe = true,
        IsDiscard = false,
        Payload = fakeAdtsFrame,
    });
}

session.Flush();

// ── 3. Pull the muxed bytes — the muxer never writes to disk itself ────────
// Zero-Copy over the native buffer; PollBytes() returns an IMemoryOwner<byte>
// (not a byte[]) precisely so this Dispose is where the native free happens —
// never via a finalizer (see this file's header revision note).
using IMemoryOwner<byte> mp4Bytes = session.PollBytes();
Console.WriteLine($"Muxed {frameCount} frames into {mp4Bytes.Memory.Length} bytes of fMP4.");

// ── 4. Demux the same bytes back ────────────────────────────────────────────
using var demuxer = new Demuxer();
demuxer.PushBytes(mp4Bytes.Memory.Span); // PushBytes can also be called incrementally as bytes arrive.

var streams = demuxer.Streams;
Console.WriteLine($"Demuxer discovered {streams.Count} stream(s):");
foreach (StreamDescriptor stream in streams)
{
    using (stream) // StreamDescriptor owns its ExtraData buffer — dispose after reading it.
    {
        string shape = stream.Geometry is { } geometry
            ? $"{geometry.Width}x{geometry.Height}"
            : "no geometry";
        Console.WriteLine($"  stream {stream.Id}: {stream.Codec} ({shape})");
    }
}

int videoPacketCount = 0;
int audioPacketCount = 0;
while (demuxer.PollPacket() is { } packet)
{
    using (packet) // Packet owns its Payload buffer when it comes from PollPacket — dispose it.
    {
        if (packet.StreamId == videoTrackId)
        {
            videoPacketCount++;
        }
        else
        {
            audioPacketCount++;
        }
    }
}

Console.WriteLine($"Recovered {videoPacketCount} video + {audioPacketCount} audio packets.");
