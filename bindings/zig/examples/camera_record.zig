// camera_record.zig — Mediaway camera + mic capture -> encode -> fragmented MP4.
//
// ASPIRATIONAL EXAMPLE: no `mediaway-device-ffi` crate exists yet and no
// <mediaway/device.h> header ships today (see docs/spec/c-ffi.md and
// bindings/README.md). This file shows the target ergonomics a future Zig
// binding covering camera + mic capture and recording should aim for, built
// via `@cImport` of the plain C ABI header. It mirrors bindings/c/screen_record.c
// with a camera source instead of a screen source.
//
// The camera recorder is built from the SAME building blocks as the
// encode-only quick start (bindings/zig/encode_to_mp4.zig): a config -> open
// auto encoder -> open encode session -> write_frame -> finish flow. The
// only new piece is a device-capture layer, glued to that flow by one small,
// platform-agnostic `record()` function below — the exact same function
// would work unchanged for screen capture (bindings/c/screen_record.c).

const std = @import("std");

const c = @cImport({
    @cInclude("mediaway/device.h");
    @cInclude("mediaway/pipeline.h");
});

const FPS: u32 = 30;
const SECONDS: f64 = 3.0;
const BITRATE_BPS: u32 = 4_000_000;

/// Errors surfaced by the C ABI's `mediaway_status_t` codes, translated into
/// Zig's error-union style instead of raw integers.
const MediawayError = error{
    OpenSessionFailed,
    WriteFrameFailed,
    ReleaseFrameFailed,
    PollError,
    CloseFailed,
    GeometryFailed,
    FinishFailed,
};

fn checkStatus(status: c.mediaway_status_t, what: []const u8) MediawayError!void {
    if (status != c.MEDIAWAY_OK) {
        std.debug.print("camera_record: {s} failed (status {d})\n", .{ what, status });
        return error.WriteFrameFailed;
    }
}

/// A minimal vtable-style interface `record()` drives — the exact same shape
/// works for a screen-capture video source, not just a camera. Only the
/// caller (`main`) knows it opened a camera; `record()` never does.
const VideoCapture = *c.mediaway_video_capture_t;
const AudioCapture = *c.mediaway_audio_capture_t;

/// record() — poll video frames from `video` and audio frames from `mic`
/// (mic may be `null`: recording continues without audio) and write a
/// synthetic grey NV12 placeholder frame into `session` for each captured
/// video frame, until `duration_seconds` elapses.
///
/// This function only sees opaque handles: it does not know or care which
/// concrete OS backend or source (camera vs. screen) produced them — that
/// dispatch already happened by the time `video`/`mic` were opened by the
/// caller. The same function drives every platform and every source.
fn record(
    video: VideoCapture,
    mic: ?AudioCapture,
    session: *c.mediaway_encode_session_t,
    width: u32,
    height: u32,
    duration_seconds: f64,
) MediawayError!void {
    // Synthetic grey NV12 source: width*height Y bytes (128) followed by
    // width*height/2 interleaved UV bytes (128). This stands in for a real
    // captured-frame -> NV12 conversion, kept out of scope for this example
    // so it runs without real camera pixels. Reused for every frame.
    const nv12_len: usize = @as(usize, width) * height + @as(usize, width) * height / 2;
    const grey_nv12 = std.heap.page_allocator.alloc(u8, nv12_len) catch {
        std.debug.print("camera_record: out of memory allocating frame buffer\n", .{});
        return error.WriteFrameFailed;
    };
    defer std.heap.page_allocator.free(grey_nv12);
    @memset(grey_nv12, 128);

    var timer = std.time.Timer.start() catch unreachable;
    var pts: i64 = 0;

    while (@as(f64, @floatFromInt(timer.read())) / std.time.ns_per_s < duration_seconds) {
        // -- Video: poll, write a frame on arrival, then release the frame
        // back to the OS (video frames may reference GPU-resident memory
        // that the capture backend needs returned before it can reuse the
        // underlying surface). -----------------------------------------
        switch (c.mediaway_video_capture_poll_frame(video)) {
            c.MEDIAWAY_POLL_FRAME_READY => {
                // A real backend would convert the polled camera frame to
                // NV12 here; this example writes the placeholder buffer
                // instead.
                const frame = c.mediaway_video_frame_t{
                    .pts = pts,
                    .duration = 1,
                    .width = width,
                    .height = height,
                    .pixel_format = c.MEDIAWAY_PIXEL_FORMAT_NV12,
                    .raw_bytes = grey_nv12.ptr,
                    .raw_bytes_len = grey_nv12.len,
                };
                pts += 1;
                try checkStatus(
                    c.mediaway_encode_session_write_frame(session, &frame),
                    "write frame",
                );
                try checkStatus(
                    c.mediaway_video_capture_release_frame(video),
                    "release frame",
                );
            },
            c.MEDIAWAY_POLL_NO_FRAME => {},
            else => {
                std.debug.print("camera_record: video capture poll error\n", .{});
                return error.PollError;
            },
        }

        // -- Audio: drain whatever is pending. Not wired into the encode
        // session yet (no audio track/encoder in this example) — just keep
        // the capture queue from backing up. -----------------------------
        if (mic) |m| {
            while (true) {
                const audio_poll = c.mediaway_audio_capture_poll_frame(m);
                if (audio_poll != c.MEDIAWAY_POLL_FRAME_READY) break;
                // TODO(#issue): push to an audio encoder / second track.
            }
        }
    }
}

pub fn main() !void {
    const tb_video = c.mediaway_rational_t{ .num = 1, .den = @as(i32, @intCast(FPS)) };

    // -- 1. Open camera capture. Opening is fallible — the specific camera
    // device may not be available; handle that gracefully instead of
    // crashing. Device index 0 = default/first camera. ------------------
    const video_cfg = c.mediaway_video_capture_config_camera(0, tb_video);
    var video: ?*c.mediaway_video_capture_t = null;
    const video_status = c.mediaway_video_capture_open(&video_cfg, &video);
    if (video_status != c.MEDIAWAY_OK) {
        std.debug.print(
            "camera_record: camera unavailable (status {d}) - nothing to do\n",
            .{video_status},
        );
        return;
    }
    const cam = video.?;
    defer _ = c.mediaway_video_capture_close(cam);

    // -- 2. Open the microphone. Also fallible; unlike the camera, a missing
    // mic should not stop recording — continue video-only. ---------------
    const tb_audio = c.mediaway_rational_t{ .num = 1, .den = 48_000 };
    const mic_cfg = c.mediaway_audio_capture_config_microphone(tb_audio);
    var mic_handle: ?*c.mediaway_audio_capture_t = null;
    const mic_status = c.mediaway_audio_capture_open(&mic_cfg, &mic_handle);
    var mic: ?*c.mediaway_audio_capture_t = null;
    if (mic_status != c.MEDIAWAY_OK) {
        std.debug.print(
            "camera_record: microphone unavailable (status {d}) - continuing without audio\n",
            .{mic_status},
        );
    } else {
        mic = mic_handle;
    }
    defer if (mic) |m| {
        _ = c.mediaway_audio_capture_close(m);
    };

    // -- 3. Query the stream geometry the camera actually negotiated — do
    // not assume a resolution. --------------------------------------------
    var width: u32 = 0;
    var height: u32 = 0;
    if (c.mediaway_video_capture_geometry(cam, &width, &height) != c.MEDIAWAY_OK) {
        return error.GeometryFailed;
    }
    std.debug.print(
        "camera_record: {d}x{d} camera, mic {s}\n",
        .{ width, height, if (mic != null) "ready" else "unavailable" },
    );

    // -- 4. Config: defaults for H.264 at the capture's real resolution and
    // frame rate, then override bitrate — same shape as the encode-only
    // quick start (bindings/zig/encode_to_mp4.zig). ------------------------
    var enc_cfg = c.mediaway_auto_video_encode_config_h264(width, height, tb_video);
    enc_cfg.bitrate_bps = BITRATE_BPS;

    var encoder: ?*c.mediaway_auto_encoder_t = null;
    const enc_status = c.mediaway_auto_encoder_open(&enc_cfg, &encoder);
    if (enc_status != c.MEDIAWAY_OK) {
        std.debug.print(
            "camera_record: no auto encoder backend available on this platform (status {d}) - nothing to do\n",
            .{enc_status},
        );
        return;
    }

    // Wrap the encoder in an encode session. On success this consumes
    // `encoder` — do not close it separately.
    var session: ?*c.mediaway_encode_session_t = null;
    if (c.mediaway_encode_session_open(encoder.?, &session) != c.MEDIAWAY_OK) {
        return error.OpenSessionFailed;
    }
    const enc_session = session.?;

    // -- 5. Record: one small, reusable function that only ever sees opaque
    // handles — no camera-specific code below this line. -------------------
    try record(cam, mic, enc_session, width, height, SECONDS);

    // -- 6. Flush the encoder, finalize the muxer, get the complete MP4
    // file. This consumes `enc_session` — do not close it separately. ------
    var mp4_ptr: [*c]u8 = null;
    var mp4_len: usize = 0;
    if (c.mediaway_encode_session_finish(enc_session, &mp4_ptr, &mp4_len) != c.MEDIAWAY_OK) {
        return error.FinishFailed;
    }
    defer c.mediaway_buffer_free(mp4_ptr);
    const mp4_bytes = @as([*]const u8, @ptrCast(mp4_ptr))[0..mp4_len];

    var out_file = try std.fs.cwd().createFile("out_camera.mp4", .{});
    defer out_file.close();
    try out_file.writeAll(mp4_bytes);

    std.debug.print(
        "camera_record: -> out_camera.mp4 ({d} bytes)\n",
        .{mp4_bytes.len},
    );
}
