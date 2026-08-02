// encode_to_mp4.zig — Mediaway auto video encoder -> fragmented MP4.
//
// ASPIRATIONAL EXAMPLE: no `mediaway-pipeline-ffi` crate exists yet and no
// <mediaway/pipeline.h> header ships today (see docs/spec/c-ffi.md and
// bindings/README.md). This file shows the target ergonomics a future Zig
// binding for Mediaway's high-level "auto encode -> fMP4" convenience layer
// should aim for, built via `@cImport` of the plain C ABI header. It mirrors
// examples/encode_to_mp4.rs.
//
// The auto encoder picks the best available OS/GPU H.264 backend at runtime
// (Zero-Copy GPU path preferred, CPU-upload fallback) and wires its packets
// into a fragmented-MP4 muxer; the caller just pushes raw frames and reads
// back MP4 bytes. Opening it is fallible by design — on a platform with no
// suitable backend yet, it returns an error the caller is expected to handle
// gracefully instead of crashing.

const std = @import("std");

const c = @cImport({
    @cInclude("mediaway/pipeline.h");
});

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const FPS: u32 = 30;
const SECONDS: u32 = 3;
const FRAME_COUNT: u32 = FPS * SECONDS;
const BITRATE_BPS: u32 = 2_000_000;

// NV12 = width*height Y bytes, followed by width*height/2 interleaved UV bytes.
const NV12_FRAME_SIZE: usize = @as(usize, WIDTH) * @as(usize, HEIGHT) +
    @as(usize, WIDTH) * @as(usize, HEIGHT) / 2;

/// Errors surfaced by the C ABI's `mediaway_status_t` codes, translated into
/// Zig's error-union style instead of raw integers.
const MediawayError = error{
    EncoderUnavailable,
    OpenSessionFailed,
    WriteFrameFailed,
    FinishFailed,
};

fn checkStatus(status: c.mediaway_status_t, what: []const u8) MediawayError!void {
    if (status != c.MEDIAWAY_OK) {
        std.debug.print("encode_to_mp4: {s} failed (status {d})\n", .{ what, status });
        return error.WriteFrameFailed;
    }
}

/// Opening the auto encoder is the one call this example must handle
/// gracefully: on a platform/GPU with no suitable H.264 backend yet, the
/// underlying `mediaway_auto_encoder_open` returns a non-OK status rather
/// than picking something worse silently.
fn openAutoEncoder(config: *const c.mediaway_auto_video_encode_config_t) MediawayError!*c.mediaway_auto_encoder_t {
    var handle: ?*c.mediaway_auto_encoder_t = null;
    if (c.mediaway_auto_encoder_open(config, &handle) != c.MEDIAWAY_OK) {
        return error.EncoderUnavailable;
    }
    return handle.?;
}

fn openEncodeSession(encoder: *c.mediaway_auto_encoder_t) MediawayError!*c.mediaway_encode_session_t {
    var session: ?*c.mediaway_encode_session_t = null;
    if (c.mediaway_encode_session_open(encoder, &session) != c.MEDIAWAY_OK) {
        return error.OpenSessionFailed;
    }
    return session.?;
}

pub fn main() !void {
    // ── 1. Config: defaults for H.264 at 640x480 @30fps, override bitrate ──
    const frame_rate = c.mediaway_rational_t{ .num = 1, .den = FPS };
    var config = c.mediaway_auto_video_encode_config_default(
        c.MEDIAWAY_CODEC_H264,
        WIDTH,
        HEIGHT,
        frame_rate,
    );
    config.bitrate_bps = BITRATE_BPS;

    // ── 2. Open the auto encoder — bail out gracefully if unsupported here ──
    const encoder = openAutoEncoder(&config) catch |err| {
        std.debug.print(
            "encode_to_mp4: no auto H.264 encoder available on this platform ({s}) - exiting\n",
            .{@errorName(err)},
        );
        return;
    };
    defer c.mediaway_auto_encoder_close(encoder);
    std.debug.print("encode_to_mp4: running on this platform\n", .{});

    // ── 3. Wrap it in an encode session (encoder + muxer wiring) ────────────
    const session = try openEncodeSession(encoder);
    defer c.mediaway_encode_session_close(session);

    // ── 4. Synthetic solid-grey NV12 source (replace with real frames) ──────
    var nv12: [NV12_FRAME_SIZE]u8 = undefined;
    @memset(&nv12, 128);

    var pts: i64 = 0;
    while (pts < FRAME_COUNT) : (pts += 1) {
        const frame = c.mediaway_video_frame_t{
            .pts = pts,
            .duration = 1,
            .width = WIDTH,
            .height = HEIGHT,
            .pixel_format = c.MEDIAWAY_PIXEL_FORMAT_NV12,
            .data = &nv12,
            .data_len = nv12.len,
        };
        try checkStatus(
            c.mediaway_encode_session_write_frame(session, &frame),
            "write frame",
        );
    }

    // ── 5. Flush the encoder + finalize the muxer, get the complete MP4 ─────
    var mp4_ptr: [*c]u8 = null;
    var mp4_len: usize = 0;
    if (c.mediaway_encode_session_finish(session, &mp4_ptr, &mp4_len) != c.MEDIAWAY_OK) {
        return error.FinishFailed;
    }
    defer c.mediaway_buffer_free(mp4_ptr);
    const mp4_bytes = @as([*]const u8, @ptrCast(mp4_ptr))[0..mp4_len];

    var out_file = try std.fs.cwd().createFile("out.mp4", .{});
    defer out_file.close();
    try out_file.writeAll(mp4_bytes);

    std.debug.print(
        "encode_to_mp4: {d} frames -> out.mp4 ({d} bytes)\n",
        .{ FRAME_COUNT, mp4_bytes.len },
    );
}
