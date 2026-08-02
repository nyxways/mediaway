// screen_record.zig — Mediaway screen + microphone capture -> encode -> fragmented MP4.
//
// ASPIRATIONAL EXAMPLE: no `mediaway-device-ffi` crate exists yet and no
// <mediaway/device.h> header ships today (see docs/spec/c-ffi.md and
// bindings/README.md). This file shows the target ergonomics a future Zig
// binding for Mediaway's device-capture layer, combined with the "auto
// encode -> fMP4" pipeline from encode_to_mp4.zig, should aim for — built
// via `@cImport` of the plain C ABI headers. It mirrors examples/screen_record.rs.
//
// The design point this example exists to demonstrate: `record()` below is
// one small, reusable function written purely against a `VideoCapture` /
// `AudioCapture` *interface* (Zig's usual fat-pointer + vtable pattern for
// runtime polymorphism) — the same role Rust's `&mut dyn VideoCapture` /
// `&mut dyn AudioCapture` trait objects play in the reference example.
// `record()` never sees which concrete OS backend (screen grabber, mic
// driver) is behind the interface; a future backend only needs to provide a
// vtable, not a change to `record()` itself.

const std = @import("std");

const c = @cImport({
    @cInclude("mediaway/device.h");
    @cInclude("mediaway/pipeline.h");
});

const FPS: u32 = 30;
const DURATION_NS: u64 = 3 * std.time.ns_per_s;
const BITRATE_BPS: u32 = 8_000_000;

/// Errors surfaced by the C ABI's `mediaway_status_t` codes, translated into
/// Zig's error-union style instead of raw integers.
const MediawayError = error{
    VideoCaptureUnavailable,
    AudioCaptureUnavailable,
    GeometryUnavailable,
    EncoderUnavailable,
    OpenSessionFailed,
    PollFrameFailed,
    WriteFrameFailed,
    FinishFailed,
};

// ── Small vtable-style interfaces (Zig's usual runtime-polymorphism pattern) ─
//
// `record()` is written only against these two interfaces, never against a
// concrete capture type. Each interface is a fat pointer: an opaque `ptr` to
// the concrete backend plus a vtable of function pointers bound to it.

const VideoCapture = struct {
    ptr: *anyopaque,
    vtable: *const VTable,

    const VTable = struct {
        pollFrame: *const fn (ptr: *anyopaque) MediawayError!?*c.mediaway_captured_video_frame_t,
        releaseFrame: *const fn (ptr: *anyopaque, frame: *c.mediaway_captured_video_frame_t) void,
    };

    fn pollFrame(self: VideoCapture) MediawayError!?*c.mediaway_captured_video_frame_t {
        return self.vtable.pollFrame(self.ptr);
    }

    fn releaseFrame(self: VideoCapture, frame: *c.mediaway_captured_video_frame_t) void {
        self.vtable.releaseFrame(self.ptr, frame);
    }
};

const AudioCapture = struct {
    ptr: *anyopaque,
    vtable: *const VTable,

    const VTable = struct {
        pollFrame: *const fn (ptr: *anyopaque) MediawayError!?c.mediaway_audio_frame_t,
    };

    fn pollFrame(self: AudioCapture) MediawayError!?c.mediaway_audio_frame_t {
        return self.vtable.pollFrame(self.ptr);
    }
};

/// Concrete backend #1: screen grab, behind the `VideoCapture` interface.
const ScreenCapture = struct {
    handle: *c.mediaway_video_capture_t,

    fn pollFrameImpl(ptr: *anyopaque) MediawayError!?*c.mediaway_captured_video_frame_t {
        const self: *ScreenCapture = @ptrCast(@alignCast(ptr));
        var frame: ?*c.mediaway_captured_video_frame_t = null;
        if (c.mediaway_video_capture_poll_frame(self.handle, &frame) != c.MEDIAWAY_OK) {
            return error.PollFrameFailed;
        }
        return frame; // null == nothing new yet, non-null == a frame arrived
    }

    fn releaseFrameImpl(ptr: *anyopaque, frame: *c.mediaway_captured_video_frame_t) void {
        const self: *ScreenCapture = @ptrCast(@alignCast(ptr));
        // Video frames may reference GPU-resident memory (a texture/surface
        // handle owned by the OS compositor) — it must be handed back
        // explicitly, not merely dropped, or the OS runs out of buffers.
        _ = c.mediaway_video_capture_release_frame(self.handle, frame);
    }

    const vtable = VideoCapture.VTable{
        .pollFrame = pollFrameImpl,
        .releaseFrame = releaseFrameImpl,
    };

    fn interface(self: *ScreenCapture) VideoCapture {
        return .{ .ptr = self, .vtable = &vtable };
    }
};

/// Concrete backend #2: microphone input, behind the `AudioCapture` interface.
const Microphone = struct {
    handle: *c.mediaway_audio_capture_t,

    fn pollFrameImpl(ptr: *anyopaque) MediawayError!?c.mediaway_audio_frame_t {
        const self: *Microphone = @ptrCast(@alignCast(ptr));
        var frame: c.mediaway_audio_frame_t = undefined;
        var has_frame: bool = false;
        if (c.mediaway_audio_capture_poll_frame(self.handle, &frame, &has_frame) != c.MEDIAWAY_OK) {
            return error.PollFrameFailed;
        }
        return if (has_frame) frame else null;
    }

    const vtable = AudioCapture.VTable{
        .pollFrame = pollFrameImpl,
    };

    fn interface(self: *Microphone) AudioCapture {
        return .{ .ptr = self, .vtable = &vtable };
    }
};

fn openScreenCapture(display_index: u32, frame_rate: c.mediaway_rational_t) MediawayError!*c.mediaway_video_capture_t {
    const config = c.mediaway_video_capture_config_screen(display_index, frame_rate);
    var handle: ?*c.mediaway_video_capture_t = null;
    if (c.mediaway_video_capture_open(&config, &handle) != c.MEDIAWAY_OK) {
        return error.VideoCaptureUnavailable;
    }
    return handle.?;
}

fn openMicrophone(sample_rate: c.mediaway_rational_t) MediawayError!*c.mediaway_audio_capture_t {
    const config = c.mediaway_audio_capture_config_microphone(sample_rate);
    var handle: ?*c.mediaway_audio_capture_t = null;
    if (c.mediaway_audio_capture_open(&config, &handle) != c.MEDIAWAY_OK) {
        return error.AudioCaptureUnavailable;
    }
    return handle.?;
}

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

/// Record for `duration_ns` from `video` + optional `audio`, feeding frames
/// into `session`. `video` and `audio` are interface values only — this
/// function compiles and behaves identically regardless of which concrete
/// backend was opened by `main`, and `audio` being absent (mic unavailable)
/// is a plain `null`, not a special code path.
fn record(
    video: VideoCapture,
    audio: ?AudioCapture,
    session: *c.mediaway_encode_session_t,
    width: u32,
    height: u32,
    duration_ns: u64,
) !void {
    var timer = try std.time.Timer.start();

    // NV12 = width*height Y bytes, followed by width*height/2 interleaved UV bytes.
    const nv12_len = @as(usize, width) * @as(usize, height) +
        @as(usize, width) * @as(usize, height) / 2;
    // Synthetic solid-grey placeholder (Y=128, UV=128) written in place of the
    // real captured pixels — see the header comment.
    var nv12 = try std.heap.page_allocator.alloc(u8, nv12_len);
    defer std.heap.page_allocator.free(nv12);
    @memset(nv12, 128);

    var pts: i64 = 0;
    while (timer.read() < duration_ns) {
        // ── Video ────────────────────────────────────────────────────────
        if (try video.pollFrame()) |captured| {
            defer video.releaseFrame(captured);

            // TODO(#issue): convert the real captured surface (`captured`,
            // possibly GPU-resident) to NV12 instead of this grey buffer.
            const frame = c.mediaway_video_frame_t{
                .pts = pts,
                .duration = 1,
                .width = width,
                .height = height,
                .pixel_format = c.MEDIAWAY_PIXEL_FORMAT_NV12,
                .data = nv12.ptr,
                .data_len = nv12.len,
            };
            if (c.mediaway_encode_session_write_frame(session, &frame) != c.MEDIAWAY_OK) {
                return error.WriteFrameFailed;
            }
            pts += 1;
        }

        // ── Audio ────────────────────────────────────────────────────────
        // Drained but not yet wired into an audio track/encoder in this example.
        if (audio) |a| {
            while (try a.pollFrame()) |_| {}
        }
    }
}

pub fn main() !void {
    // ── 1. Open screen capture — bail out gracefully if unsupported here ──
    const video_time_base = c.mediaway_rational_t{ .num = 1, .den = FPS };
    const video_handle = openScreenCapture(0, video_time_base) catch |err| {
        std.debug.print(
            "screen_record: screen capture unavailable ({s}) - platform not supported yet\n",
            .{@errorName(err)},
        );
        return;
    };
    defer c.mediaway_video_capture_close(video_handle);

    var screen_capture = ScreenCapture{ .handle = video_handle };
    const video = screen_capture.interface();

    // ── 2. Real stream geometry the capture settled on ─────────────────────
    var geometry: c.mediaway_video_geometry_t = undefined;
    if (c.mediaway_video_capture_geometry(video_handle, &geometry) != c.MEDIAWAY_OK) {
        return error.GeometryUnavailable;
    }
    const width = geometry.width;
    const height = geometry.height;

    // ── 3. Open the microphone — unavailable is not fatal, just no audio ───
    const audio_time_base = c.mediaway_rational_t{ .num = 1, .den = 48_000 };
    var microphone: ?Microphone = null;
    defer if (microphone) |m| c.mediaway_audio_capture_close(m.handle);

    if (openMicrophone(audio_time_base)) |mic_handle| {
        microphone = Microphone{ .handle = mic_handle };
    } else |err| {
        std.debug.print(
            "screen_record: microphone unavailable ({s}) - continuing without audio\n",
            .{@errorName(err)},
        );
    }
    const audio: ?AudioCapture = if (microphone) |*m| m.interface() else null;

    std.debug.print(
        "screen_record: {d}x{d} display, mic {s}\n",
        .{ width, height, if (audio != null) "ready" else "unavailable" },
    );

    // ── 4. Build the encoder config at the capture's real geometry @30fps ──
    var enc_config = c.mediaway_auto_video_encode_config_default(
        c.MEDIAWAY_CODEC_H264,
        width,
        height,
        video_time_base,
    );
    enc_config.bitrate_bps = BITRATE_BPS;

    const encoder = openAutoEncoder(&enc_config) catch |err| {
        std.debug.print("screen_record: no auto H.264 encoder available ({s})\n", .{@errorName(err)});
        return;
    };
    defer c.mediaway_auto_encoder_close(encoder);

    const session = try openEncodeSession(encoder);
    defer c.mediaway_encode_session_close(session);

    // ── 5. Core pipeline (zero platform code, zero mux wiring below this) ──
    try record(video, audio, session, width, height, DURATION_NS);

    // ── 6. Flush the encoder + finalize the muxer, get the complete MP4 ────
    var mp4_ptr: [*c]u8 = null;
    var mp4_len: usize = 0;
    if (c.mediaway_encode_session_finish(session, &mp4_ptr, &mp4_len) != c.MEDIAWAY_OK) {
        return error.FinishFailed;
    }
    defer c.mediaway_buffer_free(mp4_ptr);
    const mp4_bytes = @as([*]const u8, @ptrCast(mp4_ptr))[0..mp4_len];

    var out_file = try std.fs.cwd().createFile("out_screen.mp4", .{});
    defer out_file.close();
    try out_file.writeAll(mp4_bytes);

    std.debug.print(
        "screen_record: {d}x{d} -> out_screen.mp4 ({d} bytes)\n",
        .{ width, height, mp4_bytes.len },
    );
}
