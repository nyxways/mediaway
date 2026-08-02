//! ASPIRATIONAL EXAMPLE — no Zig binding for Mediaway exists yet.
//!
//! This sketches the target ergonomics for a future `mediaway` Zig package
//! wrapping `mediaway-container-ffi` (a C ABI). That package would be a thin
//! `@cImport` of the generated header (`mediaway_container.h`), translating
//! the C ABI's raw integer status codes into Zig error unions and modeling
//! the mux typestate (registration-open -> streaming-live) as two distinct
//! types consumed by value, with `defer`-based cleanup right after each
//! handle is acquired. None of this compiles today; it exists to drive the
//! real binding's API shape from the consumer side.
//!
//! Scenario: build a fragmented MP4 muxer, register one H.264 video track
//! (1920x1080 @ 30 fps) and one AAC audio track (48 kHz stereo), push ~90
//! fake video/audio packets across a simulated 3-second clip, flush, pull
//! out the muxed bytes, then feed those same bytes into a demuxer and count
//! the recovered packets per stream.

const std = @import("std");
const mediaway = @import("mediaway");

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const video_time_base = mediaway.Rational{ .num = 1, .den = 30 };
    const audio_time_base = mediaway.Rational{ .num = 1, .den = 48_000 };
    const frame_count: u32 = 90; // 3 s at 30 fps

    // -- 1. Register tracks while the muxer is in its open state --------------
    var muxer = try mediaway.container.mp4.Muxer.init(allocator);

    const video_track = try muxer.addTrack(.{ .video = .{
        .id = 0,
        .codec = .h264,
        .time_base = video_time_base,
        .width = 1920,
        .height = 1080,
        .extra_data = &[_]u8{},
    } });

    const audio_track = try muxer.addTrack(.{ .audio = .{
        .id = 1,
        .codec = .aac,
        .time_base = audio_time_base,
        .extra_data = &[_]u8{},
        .sample_rate = 48_000,
        .channels = 2,
    } });

    // -- 2. Move to the live state; track registration closes here ------------
    // `begin()` consumes `muxer` by value — the open-state handle must not be
    // used again after this point.
    var live = muxer.begin();
    defer live.deinit();

    const nal_placeholder = [_]u8{ 0x00, 0x00, 0x00, 0x01 };
    const adts_placeholder = [_]u8{ 0xff, 0xf1 };

    var i: u32 = 0;
    while (i < frame_count) : (i += 1) {
        try live.pushPacket(.{
            .stream_id = video_track,
            .pts = @as(i64, i),
            .dts = @as(i64, i),
            .duration = 1,
            .is_keyframe = i % 30 == 0,
            .is_discard = false,
            .payload = &nal_placeholder,
        });

        try live.pushPacket(.{
            .stream_id = audio_track,
            .pts = @as(i64, i) * 1_600,
            .dts = @as(i64, i) * 1_600,
            .duration = 1_600,
            .is_keyframe = true,
            .is_discard = false,
            .payload = &adts_placeholder,
        });
    }
    try live.flush();

    // -- 3. Pull out the muxed bytes — the muxer never touches files/sockets --
    var mp4_bytes = std.ArrayList(u8).init(allocator);
    defer mp4_bytes.deinit();
    try live.pollBytes(&mp4_bytes);
    std.debug.print(
        "mux_roundtrip: {d} frames -> {d} bytes of fMP4\n",
        .{ frame_count, mp4_bytes.items.len },
    );

    // -- 4. Demux the same bytes back ------------------------------------------
    var demuxer = try mediaway.container.mp4.Demuxer.init(allocator);
    defer demuxer.deinit();
    try demuxer.pushBytes(mp4_bytes.items);

    const discovered = demuxer.streams();
    std.debug.print("mux_roundtrip: demuxer sees {d} stream(s)\n", .{discovered.len});
    for (discovered) |stream| {
        if (stream.geometry) |g| {
            std.debug.print(
                "  stream {d} - {s} {d}x{d}\n",
                .{ stream.id, @tagName(stream.codec), g.width, g.height },
            );
        } else {
            std.debug.print(
                "  stream {d} - {s} (no geometry)\n",
                .{ stream.id, @tagName(stream.codec) },
            );
        }
    }

    var n_video: u32 = 0;
    var n_audio: u32 = 0;
    while (try demuxer.pollPacket()) |packet| {
        if (packet.stream_id == video_track) {
            n_video += 1;
        } else {
            n_audio += 1;
        }
    }
    std.debug.print("mux_roundtrip: recovered {d} video + {d} audio packets\n", .{ n_video, n_audio });
}
