// ScreenRecord.swift
//
// ASPIRATIONAL EXAMPLE: no `Mediaway` Swift package exists yet — no
// `mediaway-device-ffi` / `mediaway-encoder-ffi` C ABI crates have shipped
// either. This shows the target ergonomics for a future Swift binding over
// Mediaway's C ABI: a C bridging header wrapped in idiomatic Swift (classes
// with `deinit`/explicit `close()`, `throws`/`try` instead of raw C status
// codes). See ../README.md and docs/spec/c-ffi.md.
//
// Mirrors examples/screen_record.rs: open screen + microphone capture, build
// an H.264 auto encoder sized to the display's real geometry, run one small
// platform-agnostic `record` loop that polls both sources and writes
// synthetic grey NV12 placeholder frames into the encode session, then
// finish the session and write the fragmented MP4 bytes to `out_screen.mp4`.
//
// `ScreenCapture`/`Microphone` (concrete backends) and the `VideoCapture`/
// `AudioCapture` protocols they conform to are all part of the imagined
// `Mediaway` package — same as `AutoVideoEncoder`/`EncodeSession`/`VideoFrame`
// in ../EncodeToMp4.swift. Only `record(...)` and the top-level script below
// are example/app code.
//
// Run (once the real package exists):
//     swift run ScreenRecord

import Foundation
import Mediaway

// MARK: - Platform-agnostic record loop

/// Poll `video` and `audio` until `duration` elapses, writing one synthetic
/// grey NV12 placeholder frame into `session` per captured video frame, and
/// draining (but not yet doing anything with) audio frames.
///
/// Typed entirely against the `VideoCapture`/`AudioCapture` protocols — this
/// function does not know or care which concrete OS backend is behind
/// either capture. `audio` is `nil` when the caller could not open a
/// microphone; recording continues video-only in that case.
func record(
    video: VideoCapture,
    audio: AudioCapture?,
    session: EncodeSession,
    width: Int,
    height: Int,
    duration: TimeInterval
) {
    let deadline = Date().addingTimeInterval(duration)
    // Synthetic NV12 placeholder (Y=128, UV=128 → grey), same shape as
    // ../EncodeToMp4.swift: width*height Y bytes + width*height/2
    // interleaved UV bytes.
    let ySize = width * height
    let uvSize = width * height / 2
    let greyNV12 = Data(repeating: 128, count: ySize + uvSize)

    var pts: Int64 = 0
    while Date() < deadline {
        do {
            if try video.pollFrame() != nil {
                // A real backend would convert the captured pixels (often a
                // GPU-resident surface) to NV12 here; this example writes
                // the synthetic placeholder instead and releases the frame
                // back to the OS once it is no longer needed.
                video.releaseFrame()

                let frame = VideoFrame(
                    pts: pts,
                    duration: 1,
                    width: width,
                    height: height,
                    pixelFormat: .nv12,
                    data: greyNV12
                )
                try session.writeFrame(frame)
                pts += 1
            }
        } catch {
            print("screen_record: capture error (\(error))")
            break
        }

        // Audio track not wired in this example — frames are drained only,
        // matching examples/screen_record.rs.
        guard let audio else { continue }
        while (try? audio.pollFrame()) != nil {}
    }
}

// MARK: - Entry point

let fps = 30
let frameRate = Rational(1, fps)

// ── 1. Open screen capture — throws if no screen-capture backend is
//    available on this platform/OS version yet. Handle gracefully instead
//    of crashing. ───────────────────────────────────────────────────────────
let screen: ScreenCapture
do {
    screen = try ScreenCapture(display: 0, frameRate: frameRate)
} catch {
    print("screen_record: screen capture unavailable (\(error)) — platform not supported yet")
    exit(0)
}
print("screen_record: \(screen.width)x\(screen.height) display")

// ── 2. Open microphone capture — also fallible; recording continues
//    video-only if no microphone is available. ─────────────────────────────
var mic: Microphone?
do {
    mic = try Microphone(sampleRate: Rational(1, 48_000))
    print("screen_record: microphone ready")
} catch {
    print("screen_record: microphone unavailable (\(error)) — continuing without audio")
    mic = nil
}

do {
    // ── 3. Build the encoder config at the capture's real geometry, then
    //    open the auto encoder + encode session. ─────────────────────────────
    var config = VideoEncodeConfig.defaults(
        codec: .h264,
        width: screen.width,
        height: screen.height,
        frameRate: frameRate
    )
    config.bitrateBps = 8_000_000

    let encoder = try AutoVideoEncoder(config: config)
    let session = try EncodeSession(encoder: encoder)

    // ── 4. Run the shared record loop for 3 seconds. ─────────────────────────
    record(
        video: screen,
        audio: mic,
        session: session,
        width: screen.width,
        height: screen.height,
        duration: 3
    )

    // ── 5. Close both capture objects. ───────────────────────────────────────
    screen.close()
    mic?.close()

    // ── 6. Finish the encode session and write the fragmented MP4 bytes. ────
    let mp4Bytes = try session.finish()
    try mp4Bytes.write(to: URL(fileURLWithPath: "out_screen.mp4"))

    print("screen_record: -> out_screen.mp4 (\(mp4Bytes.count) bytes)")
} catch {
    screen.close()
    mic?.close()
    print("screen_record: recording failed (\(error))")
    exit(1)
}
